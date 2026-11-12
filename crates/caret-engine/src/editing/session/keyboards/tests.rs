//! 验证普通 Backspace 对空 InlineEnvironment 的清理语义。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use crate::editing::{
    Affinity, Command, HostCaretKind, HostCaretPosition, Pos, Selection, StoryId,
};
use caret_doc::{
    Block, Document, InlineEnvironment, InlineEnvironmentKind, NodeId, Paragraph, ParagraphContent,
    Run, TextFlow,
};
use std::collections::BTreeMap;

fn text(text: &str) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![Run {
            text: text.to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }],
        extra: Default::default(),
    })
}

fn environment(
    id: NodeId,
    kind: InlineEnvironmentKind,
    children: Vec<ParagraphContent>,
) -> ParagraphContent {
    let props = match kind {
        InlineEnvironmentKind::Bold | InlineEnvironmentKind::Italic => {
            BTreeMap::from([("enabled".to_owned(), serde_json::Value::Bool(true))])
        }
        InlineEnvironmentKind::Font => BTreeMap::from([(
            "fontFamily".to_owned(),
            serde_json::Value::String("Sans".to_owned()),
        )]),
    };
    ParagraphContent::Environment(InlineEnvironment {
        id,
        kind,
        definition_id: None,
        props,
        children,
        extra: BTreeMap::new(),
    })
}

fn session_with_inlines(nodes: Vec<ParagraphContent>) -> (Session, NodeId) {
    let paragraph = Paragraph {
        inlines: nodes,
        ..Paragraph::empty(NodeId::generate())
    };
    let paragraph_id = paragraph.id.clone();
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session =
        Session::new(document).unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let position = body_position(&paragraph_id, 0, Affinity::Downstream);
    select(&mut session, position.clone(), position);
    (session, paragraph_id)
}

fn body_position(paragraph_id: &NodeId, offset: usize, affinity: Affinity) -> Pos {
    Pos {
        story: StoryId::Body,
        paragraph_id: paragraph_id.clone(),
        offset,
        affinity,
    }
}

fn select(session: &mut Session, anchor: Pos, focus: Pos) {
    session
        .run(Command::SetSelection(Some(Selection { anchor, focus })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
}

fn collapse_at(session: &mut Session, paragraph_id: &NodeId, offset: usize, affinity: Affinity) {
    let position = body_position(paragraph_id, offset, affinity);
    select(session, position.clone(), position);
}

fn innermost_environment_id(session: &Session, position: &Pos) -> NodeId {
    fn collect(nodes: &[ParagraphContent], ids: &mut Vec<NodeId>) {
        for node in nodes {
            if let ParagraphContent::Environment(environment) = node {
                ids.push(environment.id.clone());
                collect(&environment.children, ids);
            }
        }
    }
    let paragraph = session
        .paragraph_for(position)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    let mut ids = Vec::new();
    collect(&paragraph.inlines, &mut ids);
    ids.last()
        .cloned()
        .unwrap_or_else(|| panic!("期望存在行内环境"))
}

fn enter_empty_environment(session: &mut Session) {
    let focus = session.selection.focus.clone();
    session.host_caret_position = Some(HostCaretPosition::Inside {
        host_id: innermost_environment_id(session, &focus),
        kind: HostCaretKind::Inline,
        local: focus,
    });
}

fn leave_environment_after(session: &mut Session) {
    let focus = session.selection.focus.clone();
    session.host_caret_position = Some(HostCaretPosition::After {
        host_id: innermost_environment_id(session, &focus),
        kind: HostCaretKind::Inline,
    });
}

fn leave_environment_before(session: &mut Session) {
    let focus = session.selection.focus.clone();
    session.host_caret_position = Some(HostCaretPosition::Before {
        host_id: innermost_environment_id(session, &focus),
        kind: HostCaretKind::Inline,
    });
}

fn visible_text(nodes: &[ParagraphContent]) -> String {
    nodes
        .iter()
        .map(|node| match node {
            ParagraphContent::Text(flow) => flow.runs.iter().map(|run| run.text.as_str()).collect(),
            ParagraphContent::Environment(environment) => visible_text(&environment.children),
            ParagraphContent::SoftBreak(_)
            | ParagraphContent::Opaque(_)
            | ParagraphContent::Math(_) => String::new(),
        })
        .collect()
}

fn environment_kinds(nodes: &[ParagraphContent]) -> Vec<InlineEnvironmentKind> {
    nodes
        .iter()
        .flat_map(|node| match node {
            ParagraphContent::Environment(environment) => {
                let mut kinds = vec![environment.kind];
                kinds.extend(environment_kinds(&environment.children));
                kinds
            }
            _ => Vec::new(),
        })
        .collect()
}

fn paragraph_text(session: &Session) -> String {
    let position = session.selection.focus.clone();
    visible_text(
        &session
            .paragraph_for(&position)
            .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"))
            .inlines,
    )
}

fn paragraph_inlines(session: &Session) -> Vec<ParagraphContent> {
    let position = session.selection.focus.clone();
    session
        .paragraph_for(&position)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"))
        .inlines
        .clone()
}

fn backspace(session: &mut Session) -> Result<Effect, EditError> {
    session.run(Command::EditorKey(EditorKey::Backspace))
}

#[test]
fn backspace_inside_empty_bold_environment_removes_the_environment() {
    let (mut session, _) = session_with_inlines(vec![environment(
        NodeId::generate(),
        InlineEnvironmentKind::Bold,
        Vec::new(),
    )]);
    enter_empty_environment(&mut session);
    let original_document = session.document().clone();
    let generation_before = session.document_generation();

    let effect =
        backspace(&mut session).unwrap_or_else(|error| panic!("backspace failed: {error}"));

    assert!(effect.document_changed);
    assert_eq!(session.document_generation(), generation_before + 1);
    assert!(environment_kinds(&paragraph_inlines(&session)).is_empty());
    assert_eq!(paragraph_text(&session), "");
    assert!(session.host_caret_position.is_none());
    assert!(session.can_undo());

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    assert_eq!(session.document(), &original_document);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo failed: {error}"));
    assert!(environment_kinds(&paragraph_inlines(&session)).is_empty());
}

#[test]
fn backspace_after_leaving_empty_environment_removes_it_before_preceding_text() {
    let (mut session, paragraph_id) = session_with_inlines(vec![
        text("ab"),
        environment(NodeId::generate(), InlineEnvironmentKind::Bold, Vec::new()),
    ]);
    collapse_at(&mut session, &paragraph_id, 2, Affinity::Downstream);
    leave_environment_after(&mut session);

    backspace(&mut session).unwrap_or_else(|error| panic!("backspace failed: {error}"));

    assert!(environment_kinds(&paragraph_inlines(&session)).is_empty());
    assert_eq!(paragraph_text(&session), "ab");
}

#[test]
fn backspace_deletes_nonempty_environment_text_before_removing_the_environment() {
    let (mut session, paragraph_id) = session_with_inlines(vec![
        text("a"),
        environment(
            NodeId::generate(),
            InlineEnvironmentKind::Bold,
            vec![text("bc")],
        ),
    ]);
    collapse_at(&mut session, &paragraph_id, 3, Affinity::Upstream);

    backspace(&mut session).unwrap_or_else(|error| panic!("first backspace failed: {error}"));
    assert_eq!(paragraph_text(&session), "ab");
    assert_eq!(
        environment_kinds(&paragraph_inlines(&session)),
        vec![InlineEnvironmentKind::Bold]
    );

    backspace(&mut session).unwrap_or_else(|error| panic!("second backspace failed: {error}"));
    assert_eq!(paragraph_text(&session), "a");
    assert_eq!(
        environment_kinds(&paragraph_inlines(&session)),
        vec![InlineEnvironmentKind::Bold]
    );

    backspace(&mut session).unwrap_or_else(|error| panic!("third backspace failed: {error}"));
    assert_eq!(paragraph_text(&session), "a");
    assert!(environment_kinds(&paragraph_inlines(&session)).is_empty());
}

#[test]
fn backspace_removes_nested_empty_environments_one_layer_at_a_time() {
    let (mut session, _) = session_with_inlines(vec![environment(
        NodeId::generate(),
        InlineEnvironmentKind::Italic,
        vec![environment(
            NodeId::generate(),
            InlineEnvironmentKind::Bold,
            Vec::new(),
        )],
    )]);
    enter_empty_environment(&mut session);

    backspace(&mut session).unwrap_or_else(|error| panic!("inner backspace failed: {error}"));
    assert_eq!(
        environment_kinds(&paragraph_inlines(&session)),
        vec![InlineEnvironmentKind::Italic]
    );

    backspace(&mut session).unwrap_or_else(|error| panic!("outer backspace failed: {error}"));
    assert!(environment_kinds(&paragraph_inlines(&session)).is_empty());
}

#[test]
fn backspace_before_empty_environment_deletes_preceding_text_instead() {
    let (mut session, paragraph_id) = session_with_inlines(vec![
        text("ab"),
        environment(NodeId::generate(), InlineEnvironmentKind::Bold, Vec::new()),
    ]);
    collapse_at(&mut session, &paragraph_id, 2, Affinity::Upstream);
    leave_environment_before(&mut session);

    backspace(&mut session).unwrap_or_else(|error| panic!("backspace failed: {error}"));

    assert_eq!(paragraph_text(&session), "a");
    assert_eq!(
        environment_kinds(&paragraph_inlines(&session)),
        vec![InlineEnvironmentKind::Bold]
    );
}

#[test]
fn backspace_cleans_empty_font_environment() {
    let (mut session, _) = session_with_inlines(vec![environment(
        NodeId::generate(),
        InlineEnvironmentKind::Font,
        Vec::new(),
    )]);
    enter_empty_environment(&mut session);

    backspace(&mut session).unwrap_or_else(|error| panic!("backspace failed: {error}"));

    assert!(environment_kinds(&paragraph_inlines(&session)).is_empty());
}

#[test]
fn backspace_cleanup_leaves_run_property_untouched() {
    let paragraph = Paragraph {
        inlines: vec![ParagraphContent::Text(TextFlow {
            runs: vec![Run {
                text: "ab".to_owned(),
                language: None,
                props: BTreeMap::from([("underline".to_owned(), serde_json::Value::Bool(true))]),
                extra: BTreeMap::new(),
            }],
            extra: Default::default(),
        })],
        ..Paragraph::empty(NodeId::generate())
    };
    let paragraph_id = paragraph.id.clone();
    let mut document = Document::blank();
    document.body = vec![Block::Paragraph(paragraph)];
    let mut session =
        Session::new(document).unwrap_or_else(|error| panic!("session creation failed: {error}"));
    collapse_at(&mut session, &paragraph_id, 2, Affinity::Upstream);

    backspace(&mut session).unwrap_or_else(|error| panic!("backspace failed: {error}"));

    assert_eq!(paragraph_text(&session), "a");
    assert!(paragraph_inlines(&session).iter().any(|node| {
        matches!(node, ParagraphContent::Text(flow) if flow.runs.iter().any(|run| run.props.contains_key("underline")))
    }));
}

#[test]
fn backspace_does_not_clean_empty_environment_during_ime_or_command_input() {
    let (mut session, _) = session_with_inlines(vec![environment(
        NodeId::generate(),
        InlineEnvironmentKind::Bold,
        Vec::new(),
    )]);
    enter_empty_environment(&mut session);
    let before = session.document().clone();
    session
        .run(Command::UpdateComposition {
            id: 41,
            preedit: "pre".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition setup failed: {error}"));

    backspace(&mut session).unwrap_or_else(|error| panic!("IME backspace failed: {error}"));

    assert_eq!(session.document(), &before);
    assert!(session.composition().is_some());
    session
        .run(Command::CancelComposition { id: 41 })
        .unwrap_or_else(|error| panic!("composition cancellation failed: {error}"));

    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command input setup failed: {error}"));
    session
        .run(Command::TypeCommandLine("bo".to_owned()))
        .unwrap_or_else(|error| panic!("command input setup failed: {error}"));
    backspace(&mut session).unwrap_or_else(|error| panic!("command backspace failed: {error}"));
    assert_eq!(session.document(), &before);
    assert!(session.command_line().is_some());
}

#[test]
fn backspace_and_mod_backspace_keep_distinct_nonempty_environment_semantics() {
    let (mut session, paragraph_id) = session_with_inlines(vec![environment(
        NodeId::generate(),
        InlineEnvironmentKind::Bold,
        vec![text("ab")],
    )]);
    collapse_at(&mut session, &paragraph_id, 1, Affinity::Downstream);

    backspace(&mut session).unwrap_or_else(|error| panic!("backspace failed: {error}"));
    assert_eq!(paragraph_text(&session), "b");
    assert_eq!(
        environment_kinds(&paragraph_inlines(&session)),
        vec![InlineEnvironmentKind::Bold]
    );

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    session
        .run(Command::EditorKey(EditorKey::ModBackspace))
        .unwrap_or_else(|error| panic!("mod backspace failed: {error}"));
    assert_eq!(paragraph_text(&session), "ab");
    assert!(environment_kinds(&paragraph_inlines(&session)).is_empty());
}

#[test]
fn backspace_keeps_structure_range_deletion_ahead_of_empty_inline_cleanup() {
    let paragraph = Paragraph {
        inlines: vec![
            environment(NodeId::generate(), InlineEnvironmentKind::Bold, Vec::new()),
            text("x"),
        ],
        ..Paragraph::empty(NodeId::generate())
    };
    let environment_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![
        Block::Paragraph(paragraph),
        Block::Environment(caret_doc::BlockEnvironment {
            id: environment_id.clone(),
            kind: caret_doc::BlockEnvironmentKind::Custom,
            definition_id: None,
            props: BTreeMap::new(),
            title: None,
            children: vec![Block::Paragraph(Paragraph::empty(NodeId::generate()))],
            extra: BTreeMap::new(),
        }),
    ];
    let mut session =
        Session::new(document).unwrap_or_else(|error| panic!("session creation failed: {error}"));
    session
        .select_block_environment(environment_id)
        .unwrap_or_else(|error| panic!("structure selection failed: {error}"));
    assert!(session.has_structure_selection());

    backspace(&mut session).unwrap_or_else(|error| panic!("backspace failed: {error}"));

    assert_eq!(session.document().body.len(), 1);
    assert_eq!(
        environment_kinds(&paragraph_inlines(&session)),
        vec![InlineEnvironmentKind::Bold]
    );
}

#[test]
fn backspace_after_pointer_selection_beside_empty_environment_cleans_it() {
    // 鼠标 SetSelection 会清空 host 状态；左外侧 Upstream 光标没有显式 Before 宿主，
    // 普通退格按“就近空环境优先”清理，而不是删除前面的字符。
    let (mut session, paragraph_id) = session_with_inlines(vec![
        text("ab"),
        environment(NodeId::generate(), InlineEnvironmentKind::Bold, vec![]),
    ]);
    select(
        &mut session,
        body_position(&paragraph_id, 2, Affinity::Upstream),
        body_position(&paragraph_id, 2, Affinity::Upstream),
    );

    session
        .run(Command::EditorKey(EditorKey::Backspace))
        .unwrap_or_else(|error| panic!("backspace failed: {error}"));

    let document = session.document();
    let Block::Paragraph(paragraph) = &document.body[0] else {
        panic!("expected a paragraph");
    };
    assert_eq!(paragraph.inlines.len(), 1);
    assert_eq!(paragraph.inlines[0], text("ab"));
}
