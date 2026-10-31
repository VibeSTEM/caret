//! 验证编辑会话缓存与修订代际。

use super::*;
use crate::editing::{EditorKey, MathDirection, MathPos};
use caret_doc::{
    Block, BlockEnvironment, BlockEnvironmentKind, Document, DocumentFontTarget, InlineEnvironment,
    InlineEnvironmentKind, InlineMath, InlineNode, MATH_TREE_SCHEMA_VERSION, MathNode,
    MathNodeKind, MathTree, NodeId, Paragraph, Run,
};
use std::collections::BTreeMap;
use unicode_segmentation::UnicodeSegmentation;

fn paragraph_text(session: &Session, block_index: usize) -> String {
    let Some(Block::Paragraph(paragraph)) = session.document.body.get(block_index) else {
        panic!("expected paragraph at block index {block_index}");
    };
    TextMap::new(paragraph)
        .unwrap_or_else(|error| panic!("paragraph text map failed: {error}"))
        .text
}

fn current_math_position(session: &Session) -> &crate::editing::MathPos {
    &session
        .math_focus()
        .unwrap_or_else(|| panic!("expected active math focus"))
        .position
}

fn paragraph_math_node_count(session: &Session) -> usize {
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing inline math");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math");
    };
    math.math_tree.root.children.len()
}

#[test]
fn checkpoint_restores_edit_history_and_identity_after_failed_layout_transaction() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank document should create a session: {error}"));
    session
        .run(crate::Command::InsertText("before".to_owned()))
        .unwrap_or_else(|error| panic!("seed edit should succeed: {error}"));
    let session_id = session.session_id().clone();
    let generation = session.document_generation();
    let document = session.document().clone();
    let selection = session.selection().clone();
    let checkpoint = session.checkpoint();

    session
        .run(crate::Command::InsertText(" after".to_owned()))
        .unwrap_or_else(|error| panic!("transient edit should succeed: {error}"));
    assert!(session.restore_checkpoint(checkpoint));

    assert_eq!(session.session_id(), &session_id);
    assert!(session.document_generation() > generation);
    assert_eq!(session.document(), &document);
    assert_eq!(session.selection(), &selection);
    session
        .run(crate::Command::InsertText(" again".to_owned()))
        .unwrap_or_else(|error| panic!("history should accept an edit after rollback: {error}"));
    session
        .run(crate::Command::Undo)
        .unwrap_or_else(|error| panic!("undo should work after rollback: {error}"));
    assert_eq!(paragraph_text(&session, 0), "");
    session
        .run(crate::Command::Redo)
        .unwrap_or_else(|error| panic!("redo should work after rollback: {error}"));
    assert_eq!(paragraph_text(&session, 0), "before again");
}

#[test]
fn checkpoint_cannot_restore_another_session_or_duplicate_its_identity() {
    let mut first = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("first session should initialize: {error}"));
    let mut second = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("second session should initialize: {error}"));
    let first_id = first.session_id().clone();
    let second_id = second.session_id().clone();
    assert!(!first.restore_checkpoint(second.checkpoint()));
    assert!(!second.restore_checkpoint(first.checkpoint()));
    assert_eq!(first.session_id(), &first_id);
    assert_eq!(second.session_id(), &second_id);
}

#[test]
fn checkpoint_restore_without_document_transition_preserves_generation() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank document should create a session: {error}"));
    let generation = session.document_generation();
    let checkpoint = session.checkpoint();

    assert!(session.restore_checkpoint(checkpoint));

    assert_eq!(session.document_generation(), generation);
}

#[test]
fn composition_preview_has_distinct_identity_and_trusted_provenance() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank document should create a session: {error}"));
    let paragraph_id = session.selection().focus.paragraph_id.clone();
    let candidate = session
        .composition_update_candidate(1, "preedit".to_owned(), None, 3)
        .unwrap_or_else(|error| panic!("composition candidate should be valid: {error}"));

    assert_ne!(
        candidate.preview_session().session_id(),
        session.session_id()
    );
    assert!(
        candidate
            .preview_session()
            .is_composition_preview_of(&session, &paragraph_id)
    );
    assert_eq!(candidate.range().anchor.paragraph_id, paragraph_id);
}

#[test]
fn stale_composition_candidate_cannot_replace_a_newer_session_state() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank document should create a session: {error}"));
    let candidate = session
        .composition_update_candidate(1, "preedit".to_owned(), None, 3)
        .unwrap_or_else(|error| panic!("composition candidate should be valid: {error}"));
    session
        .run(crate::Command::InsertText("newer".to_owned()))
        .unwrap_or_else(|error| panic!("newer edit should succeed: {error}"));
    let document = session.document().clone();
    let generation = session.document_generation();

    assert!(session.install_composition_candidate(candidate).is_err());

    assert_eq!(session.document(), &document);
    assert_eq!(session.document_generation(), generation);
    assert_eq!(paragraph_text(&session, 0), "newer");
}

#[test]
fn older_composition_candidate_cannot_replace_a_newer_candidate_in_the_same_generation() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank document should create a session: {error}"));
    let older = session
        .composition_update_candidate(1, "a".to_owned(), None, 1)
        .unwrap_or_else(|error| panic!("first candidate should be valid: {error}"));
    let newer = session
        .composition_update_candidate(2, "ab".to_owned(), None, 2)
        .unwrap_or_else(|error| panic!("second candidate should be valid: {error}"));
    let generation = session.document_generation();

    assert!(session.install_composition_candidate(older).is_err());
    let _preview = session
        .install_composition_candidate(newer)
        .unwrap_or_else(|error| panic!("latest candidate should install: {error}"));

    assert_eq!(session.document_generation(), generation);
    assert_eq!(
        session
            .composition()
            .map(|composition| composition.preedit.as_str()),
        Some("ab")
    );
}

#[test]
fn composition_candidate_is_rejected_after_selection_moves() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank document should create a session: {error}"));
    let candidate = session
        .composition_update_candidate(1, "preedit".to_owned(), None, 3)
        .unwrap_or_else(|error| panic!("candidate should be valid: {error}"));
    session
        .run(crate::Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("selection movement should succeed: {error}"));
    let selection = session.selection().clone();

    assert!(session.install_composition_candidate(candidate).is_err());
    assert_eq!(session.selection(), &selection);
    assert!(session.composition().is_none());
}

#[test]
fn composition_candidate_revision_covers_command_line_save_checkpoint_and_rollback() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank document should create a session: {error}"));
    let checkpoint = session.checkpoint();
    let stale_after_restore = session
        .composition_update_candidate(1, "restore".to_owned(), None, 3)
        .unwrap_or_else(|error| panic!("candidate should be valid: {error}"));
    assert!(session.restore_checkpoint(checkpoint));
    assert!(
        session
            .install_composition_candidate(stale_after_restore)
            .is_err()
    );

    let stale_after_command_line = session
        .composition_update_candidate(2, "command".to_owned(), None, 3)
        .unwrap_or_else(|error| panic!("candidate should be valid: {error}"));
    session
        .run(crate::Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line should start: {error}"));
    assert!(
        session
            .install_composition_candidate(stale_after_command_line)
            .is_err()
    );
    assert!(session.command_line().is_some());

    let stale_after_save = session
        .composition_update_candidate(3, "save".to_owned(), None, 2)
        .unwrap_or_else(|error| panic!("candidate should be valid: {error}"));
    session.begin_save_checkpoint();
    assert!(
        session
            .install_composition_candidate(stale_after_save)
            .is_err()
    );
}

#[test]
fn current_inline_environment_respects_nested_scope_and_boundary_affinity() {
    let mut document = Document::blank();
    let outer_id = NodeId::generate();
    let inner_id = NodeId::generate();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = vec![InlineNode::Environment(InlineEnvironment {
        id: outer_id,
        kind: InlineEnvironmentKind::Bold,
        definition_id: None,
        props: BTreeMap::new(),
        children: vec![InlineNode::Environment(InlineEnvironment {
            id: inner_id.clone(),
            kind: InlineEnvironmentKind::Italic,
            definition_id: None,
            props: BTreeMap::new(),
            children: vec![
                InlineNode::Run(Run {
                    text: "under".to_owned(),
                    language: None,
                    props: BTreeMap::from([("underline".into(), serde_json::Value::Bool(true))]),
                    extra: BTreeMap::new(),
                }),
                InlineNode::Run(Run {
                    text: " rest".to_owned(),
                    language: None,
                    props: BTreeMap::new(),
                    extra: BTreeMap::new(),
                }),
            ],
            extra: BTreeMap::new(),
        })],
        extra: BTreeMap::new(),
    })];
    let mut session =
        Session::new(document).unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let focus = session.selection().focus.clone();
    let paragraph_id = focus.paragraph_id.clone();

    let inside = session
        .current_inline_environment()
        .unwrap_or_else(|error| panic!("inline environment lookup failed: {error}"))
        .unwrap_or_else(|| panic!("caret should inherit nested inline environments"));

    assert_eq!(inside.environment_id, inner_id);
    assert_eq!(inside.paragraph_id, paragraph_id);
    assert_eq!(inside.start_utf8, 0);
    assert_eq!(inside.end_utf8, "under rest".len());
    let highlight = session
        .current_inline_highlight_range()
        .unwrap_or_else(|error| panic!("inline highlight lookup failed: {error}"))
        .unwrap_or_else(|| panic!("inline environment should produce a highlight range"));
    assert_eq!(highlight.anchor.offset, 0);
    assert_eq!(highlight.focus.offset, "under rest".len());

    let outside_start = Pos {
        affinity: Affinity::Upstream,
        ..focus.clone()
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: outside_start.clone(),
            focus: outside_start,
        })))
        .unwrap_or_else(|error| panic!("moving outside the environment failed: {error}"));
    assert!(
        session
            .current_inline_environment()
            .unwrap_or_else(|error| panic!("inline environment lookup failed: {error}"))
            .is_none()
    );
}

#[test]
fn current_inline_highlight_uses_underlined_run_and_boundary_affinity() {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    let paragraph_id = paragraph.id.clone();
    paragraph.inlines = vec![
        InlineNode::Run(Run {
            text: "before".into(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
        InlineNode::Run(Run {
            text: "underlined".into(),
            language: None,
            props: BTreeMap::from([("underline".into(), serde_json::Value::Bool(true))]),
            extra: BTreeMap::new(),
        }),
        InlineNode::Run(Run {
            text: "after".into(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
    ];
    let mut session =
        Session::new(document).unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let mut focus = session.selection().focus.clone();
    focus.offset = "before".len();
    focus.affinity = Affinity::Downstream;
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: focus.clone(),
            focus: focus.clone(),
        })))
        .unwrap_or_else(|error| panic!("moving caret into underline run failed: {error}"));
    let generation = session.document_generation();

    let range = session
        .current_inline_highlight_range()
        .unwrap_or_else(|error| panic!("underline lookup failed: {error}"))
        .unwrap_or_else(|| panic!("caret in an underlined run should expose its range"));
    assert_eq!(range.anchor.paragraph_id, paragraph_id);
    assert_eq!(range.anchor.offset, "before".len());
    assert_eq!(range.focus.offset, "beforeunderlined".len());
    assert_eq!(session.document_generation(), generation);

    let mut opposite_affinity = focus.clone();
    opposite_affinity.affinity = Affinity::Upstream;
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: focus.clone(),
            focus: opposite_affinity,
        })))
        .unwrap_or_else(|error| panic!("setting an affinity-only selection failed: {error}"));
    assert!(
        session
            .current_inline_highlight_range()
            .is_ok_and(|range| range.is_none())
    );
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: focus.clone(),
            focus: focus.clone(),
        })))
        .unwrap_or_else(|error| panic!("restoring collapsed caret failed: {error}"));

    let mut selected_focus = focus.clone();
    selected_focus.offset += "under".len();
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                offset: 0,
                affinity: Affinity::Downstream,
                ..selected_focus.clone()
            },
            focus: selected_focus,
        })))
        .unwrap_or_else(|error| panic!("selecting underlined text failed: {error}"));
    assert!(
        session
            .current_inline_highlight_range()
            .is_ok_and(|range| range.is_none())
    );

    focus.offset = "beforeunderlined".len();
    focus.affinity = Affinity::Upstream;
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: focus.clone(),
            focus: focus.clone(),
        })))
        .unwrap_or_else(|error| panic!("moving caret to underline end failed: {error}"));
    assert!(
        session
            .current_inline_highlight_range()
            .is_ok_and(|range| range.is_some())
    );

    focus.affinity = Affinity::Downstream;
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: focus.clone(),
            focus: focus.clone(),
        })))
        .unwrap_or_else(|error| panic!("moving caret past underline end failed: {error}"));
    assert!(
        session
            .current_inline_highlight_range()
            .is_ok_and(|range| range.is_none())
    );
}

#[test]
fn move_caret_can_leave_direct_underline_at_its_right_edge() {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = vec![InlineNode::Run(Run {
        text: "underlined".into(),
        language: None,
        props: BTreeMap::from([("underline".into(), serde_json::Value::Bool(true))]),
        extra: BTreeMap::new(),
    })];
    let mut session =
        Session::new(document).unwrap_or_else(|error| panic!("session rejected: {error}"));
    let run_end = "underlined".len();

    for _ in 0.."underlined".chars().count() {
        session
            .run(Command::MoveCaret(Direction::Forward))
            .unwrap_or_else(|error| panic!("moving through underlined text failed: {error}"));
    }
    assert_eq!(session.selection().focus.offset, run_end);
    assert_eq!(session.selection().focus.affinity, Affinity::Upstream);
    assert!(
        session
            .current_inline_highlight_range()
            .unwrap_or_else(|error| panic!("underline highlight lookup failed: {error}"))
            .is_some()
    );

    session
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("moving out of the underline failed: {error}"));
    assert_eq!(session.selection().focus.offset, run_end);
    assert_eq!(session.selection().focus.affinity, Affinity::Downstream);
    assert!(
        session
            .current_inline_highlight_range()
            .unwrap_or_else(|error| panic!("underline highlight lookup failed: {error}"))
            .is_none()
    );

    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("moving back into the underline failed: {error}"));
    assert_eq!(session.selection().focus.offset, run_end - 1);
    assert!(
        session
            .current_inline_highlight_range()
            .unwrap_or_else(|error| panic!("underline highlight lookup failed: {error}"))
            .is_some()
    );
}

#[test]
fn text_map_cache_keeps_both_paragraphs_of_a_cross_paragraph_selection() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("session creation failed: {error}"));
    session
        .run(Command::InsertParagraphBreak)
        .unwrap_or_else(|error| panic!("paragraph split failed: {error}"));
    let Block::Paragraph(first) = &session.document.body[0] else {
        panic!("first block should be a paragraph");
    };
    let Block::Paragraph(second) = &session.document.body[1] else {
        panic!("second block should be a paragraph");
    };
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    let first_position = Pos {
        story: StoryId::Body,
        paragraph_id: first_id.clone(),
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let second_position = Pos {
        paragraph_id: second_id.clone(),
        ..first_position.clone()
    };

    session
        .text_map_for(&first_position)
        .unwrap_or_else(|error| panic!("first text map failed: {error}"));
    session
        .text_map_for(&second_position)
        .unwrap_or_else(|error| panic!("second text map failed: {error}"));
    session
        .text_map_for(&first_position)
        .unwrap_or_else(|error| panic!("cached first text map failed: {error}"));

    assert_eq!(session.text_map_cache.len(), TEXT_MAP_CACHE_CAPACITY);
    assert!(
        session
            .text_map_cache
            .iter()
            .any(|(id, _, _)| id == &first_id)
    );
    assert!(
        session
            .text_map_cache
            .iter()
            .any(|(id, _, _)| id == &second_id)
    );
}

#[test]
fn paragraph_split_after_inline_math_uses_utf8_placeholder_offset() {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    let first_id = paragraph.id.clone();
    paragraph.inlines = vec![
        InlineNode::Math(InlineMath {
            id: NodeId::generate(),
            math_tree: MathTree {
                schema_version: MATH_TREE_SCHEMA_VERSION,
                source: "x".to_owned(),
                root: MathNode {
                    id: NodeId::generate(),
                    kind: MathNodeKind::Symbol,
                    text: Some("x".to_owned()),
                    attributes: BTreeMap::new(),
                    source_range: None,
                    children: Vec::new(),
                    extra: BTreeMap::new(),
                },
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        }),
        InlineNode::Run(Run {
            text: "x".to_owned(),
            language: None,
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        }),
    ];
    let mut second = Paragraph::empty(NodeId::generate());
    second.inlines.push(InlineNode::Run(Run {
        text: "b".to_owned(),
        language: None,
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }));
    let second_id = second.id.clone();
    document.body.push(Block::Paragraph(second));

    let mut session =
        Session::new(document).unwrap_or_else(|error| panic!("session creation failed: {error}"));
    let selection = Selection {
        anchor: Pos {
            paragraph_id: first_id,
            offset: '\u{fffc}'.len_utf8(),
            ..session.selection().focus.clone()
        },
        focus: Pos {
            paragraph_id: second_id,
            offset: 0,
            ..session.selection().focus.clone()
        },
    };
    session
        .run(Command::SetSelection(Some(selection)))
        .unwrap_or_else(|error| panic!("cross-paragraph selection failed: {error}"));
    session
        .run(Command::InsertParagraphBreak)
        .unwrap_or_else(|error| panic!("split after math failed: {error}"));

    assert_eq!(session.document.body.len(), 2);
    assert_eq!(paragraph_text(&session, 0), "\u{fffc}");
    assert_eq!(paragraph_text(&session, 1), "b");
}

#[test]
fn revision_overflow_rotates_session_identity_before_reusing_generation_zero() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    let old_session_id = session.session_id().clone();
    session.document_generation = u64::MAX;

    let effect = session
        .run(Command::InsertText("new revision".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));

    assert!(effect.document_changed);
    assert_eq!(session.document_generation(), 0);
    assert_ne!(session.session_id(), &old_session_id);
    assert_eq!(session.selection().focus.offset, "new revision".len());
}

#[test]
fn checkpoint_restore_accepts_identity_rotation_after_revision_overflow() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session.document_generation = u64::MAX;
    session.composition_candidate_revision = u64::MAX;
    let original_document = session.document().clone();
    let checkpoint = session.checkpoint();

    session
        .run(Command::InsertText("transient".to_owned()))
        .unwrap_or_else(|error| panic!("transient edit failed: {error}"));
    assert_eq!(session.document_generation(), 0);

    assert!(session.restore_checkpoint(checkpoint));
    assert_eq!(session.document(), &original_document);
    assert_eq!(session.document_generation(), 1);
    assert_eq!(session.composition_candidate_revision, 1);
}

#[test]
fn composition_preview_checkpoints_cannot_cross_restore_session_state() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::UpdateComposition {
            id: 4,
            preedit: "preedit".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition update failed: {error}"));
    let mut preview = session
        .composition_preview(7)
        .unwrap_or_else(|error| panic!("composition preview failed: {error}"));
    let base_document = session.document().clone();
    let preview_document = preview.document().clone();

    assert!(!session.restore_checkpoint(preview.checkpoint()));
    assert!(!preview.restore_checkpoint(session.checkpoint()));
    assert_eq!(session.document(), &base_document);
    assert_eq!(preview.document(), &preview_document);
}

#[test]
fn failed_editor_command_keeps_the_active_command_query() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("sec".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    let expected_command_line = session
        .command_line()
        .cloned()
        .unwrap_or_else(|| panic!("command query should remain active"));
    let invalid_position = Pos {
        offset: usize::MAX,
        ..session.selection().focus.clone()
    };

    let result = session.run(Command::SetSelection(Some(Selection {
        anchor: invalid_position.clone(),
        focus: invalid_position,
    })));

    assert!(matches!(result, Err(EditError::InvalidPosition)));
    assert_eq!(session.command_line(), Some(&expected_command_line));
}

#[test]
fn escape_cancels_command_query_without_changing_document_or_structure() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("sec".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    let revision = session.document_generation();

    let effect = session
        .run(Command::EditorKey(EditorKey::Escape))
        .unwrap_or_else(|error| panic!("Escape failed: {error}"));

    assert!(session.command_line().is_none());
    assert!(!effect.document_changed);
    assert_eq!(session.document_generation(), revision);
    assert_eq!(paragraph_text(&session, 0), "");
}

#[test]
fn inline_math_insertion_routes_text_and_history_restores_math_focus() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));

    let effect = session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    assert!(effect.document_changed);
    assert!(effect.selection_changed);
    let focus = session
        .math_focus()
        .cloned()
        .unwrap_or_else(|| panic!("math insertion should enter MathFocus"));
    assert_eq!(focus.position.path.len(), 1);
    assert_eq!(session.selection().focus.offset, "\u{fffc}".len());

    let text_effect = session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("text should route into formula: {error}"));
    assert!(text_effect.selection_changed);
    assert_eq!(paragraph_text(&session, 0), "\u{fffc}");
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing the inline formula");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math as the only paragraph child");
    };
    assert_eq!(math.math_tree.root.children[0].text.as_deref(), Some("x"));

    let undo_effect = session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("formula text undo failed: {error}"));
    assert!(undo_effect.selection_changed);
    let focused = session
        .math_focus()
        .unwrap_or_else(|| panic!("undo should preserve formula focus"));
    {
        let Block::Paragraph(paragraph) = &session.document.body[0] else {
            panic!("expected paragraph containing the inline formula");
        };
        let InlineNode::Math(math) = &paragraph.inlines[0] else {
            panic!("expected inline math after undo");
        };
        assert!(math.math_tree.root.children.is_empty());
        assert_eq!(focused.position.path, vec![math.math_tree.root.id.clone()]);
    }

    let redo_effect = session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("formula text redo failed: {error}"));
    assert!(redo_effect.selection_changed);
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph after redo");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math after redo");
    };
    assert_eq!(math.math_tree.root.children[0].text.as_deref(), Some("x"));
    let focus = session
        .math_focus()
        .unwrap_or_else(|| panic!("redo should restore formula focus"));
    assert!(focus.position.resolve(&math.math_tree).is_some());
}

#[test]
fn charwise_latex_command_input_buffers_prefix_and_commits_as_one_edit() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));

    let command = r"\alpha";
    for (index, character) in command.chars().enumerate() {
        let effect = session
            .run(Command::InsertText(character.to_string()))
            .unwrap_or_else(|error| panic!("math command prefix was rejected: {error}"));
        assert!(!effect.document_changed);
        let expected = command.chars().take(index + 1).collect::<String>();
        assert_eq!(session.pending_math_input(), Some(expected.as_str()));
    }
    let commit = session
        .run(Command::InsertText("+".to_owned()))
        .unwrap_or_else(|error| panic!("complete math chunk failed: {error}"));
    assert!(commit.document_changed);
    assert!(commit.math_input_changed);
    assert!(session.pending_math_input().is_none());

    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing formula");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math");
    };
    assert_eq!(math.math_tree.root.children.len(), 2);
    assert_eq!(math.math_tree.root.children[0].kind, MathNodeKind::Symbol);
    assert_eq!(
        math.math_tree.root.children[1]
            .attributes
            .get("atomClass")
            .map(String::as_str),
        Some("bin")
    );

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo of buffered command failed: {error}"));
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph after undo");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math after undo");
    };
    assert!(math.math_tree.root.children.is_empty());
    assert!(session.math_focus().is_some());
}

#[test]
fn command_line_prefix_routes_into_math_parser_when_formula_has_focus() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));

    let effect = session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("math command prefix was rejected: {error}"));
    assert!(effect.math_input_changed);
    assert_eq!(session.pending_math_input(), Some("\\"));
    assert!(session.command_line().is_none());

    for character in "alpha+".chars() {
        session
            .run(Command::InsertText(character.to_string()))
            .unwrap_or_else(|error| panic!("math command continuation failed: {error}"));
    }
    assert!(session.pending_math_input().is_none());
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing formula");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline formula");
    };
    assert_eq!(math.math_tree.root.children.len(), 2);
    assert_eq!(
        math.math_tree.root.children[0].text.as_deref(),
        Some("alpha")
    );
    assert_eq!(math.math_tree.root.children[1].text.as_deref(), Some("+"));
}

#[test]
fn incomplete_math_input_is_preserved_for_backspace_and_escape_recovery() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    let document = session.document().clone();
    let history_state_id = session.history_state_id();

    let result = session.run(Command::InsertText(r"\unknown ".to_owned()));
    let error_effect =
        result.unwrap_or_else(|error| panic!("invalid math input was not retained: {error}"));
    assert!(error_effect.math_input_changed);
    assert!(error_effect.math_input_error);
    assert_eq!(session.pending_math_input(), Some(r"\unknown "));
    assert!(session.math_input_error());
    assert_eq!(session.document(), &document);
    assert_eq!(session.history_state_id(), history_state_id);

    let backspace = session
        .run(Command::Delete(Direction::Backward))
        .unwrap_or_else(|error| panic!("backspace did not edit pending formula input: {error}"));
    assert!(backspace.math_input_changed);
    assert!(!backspace.math_input_error);
    assert_eq!(session.pending_math_input(), Some(r"\unknown"));
    assert_eq!(session.document(), &document);

    let escape = session
        .run(Command::EditorKey(EditorKey::Escape))
        .unwrap_or_else(|error| panic!("Escape did not cancel pending math input: {error}"));
    assert!(escape.math_input_changed);
    assert!(!escape.math_input_error);
    assert!(!session.math_input_error());
    assert!(session.pending_math_input().is_none());
    assert!(session.math_focus().is_some());
    assert_eq!(session.document(), &document);

    session
        .run(Command::EditorKey(EditorKey::Escape))
        .unwrap_or_else(|error| panic!("second Escape did not leave formula focus: {error}"));
    assert!(session.math_focus().is_none());
}

#[test]
fn unknown_command_with_closed_group_is_reported_as_invalid_not_incomplete() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));

    let effect = session
        .run(Command::TypeMathText(r"\unknown{x}".to_owned()))
        .unwrap_or_else(|error| panic!("invalid formula input was not retained: {error}"));

    assert!(effect.math_input_changed);
    assert!(effect.math_input_error);
    assert!(session.math_input_error());
    assert_eq!(session.pending_math_input(), Some(r"\unknown{x}"));

    session
        .run(Command::EditorKey(EditorKey::Escape))
        .unwrap_or_else(|error| panic!("Escape did not clear invalid formula: {error}"));
    let open_group = session
        .run(Command::TypeMathText(r"\unknown{".to_owned()))
        .unwrap_or_else(|error| panic!("invalid open group was not retained: {error}"));
    assert!(open_group.math_input_error);
    assert!(session.math_input_error());
    assert_eq!(session.pending_math_input(), Some(r"\unknown{"));
}

#[test]
fn backspace_commits_repaired_valid_math_input() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    let generation_before_input = session.document_generation();

    let invalid = session
        .run(Command::TypeMathText(r"\alpha+}".to_owned()))
        .unwrap_or_else(|error| panic!("invalid formula input was not retained: {error}"));
    assert!(invalid.math_input_error);
    assert_eq!(session.document_generation(), generation_before_input);

    let repaired = session
        .run(Command::Delete(Direction::Backward))
        .unwrap_or_else(|error| panic!("backspace did not commit repaired formula: {error}"));
    assert!(repaired.math_input_changed);
    assert!(!repaired.math_input_error);
    assert!(session.pending_math_input().is_none());
    assert!(!session.math_input_error());
    assert_eq!(session.document_generation(), generation_before_input + 1);

    let Block::Paragraph(paragraph) = &session.document().body[0] else {
        panic!("expected paragraph after repaired formula commit");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline formula after repaired formula commit");
    };
    assert!(!math.math_tree.root.children.is_empty());

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo did not remove repaired formula input: {error}"));
    assert_eq!(paragraph_math_node_count(&session), 0);
}

#[test]
fn valid_command_name_prefix_is_not_committed_before_its_longer_name() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));

    for character in r"\sin".chars() {
        session
            .run(Command::InsertText(character.to_string()))
            .unwrap_or_else(|error| panic!("valid command prefix failed: {error}"));
    }
    assert_eq!(session.pending_math_input(), Some(r"\sin"));
    for character in "h".chars() {
        session
            .run(Command::InsertText(character.to_string()))
            .unwrap_or_else(|error| panic!("longer command prefix failed: {error}"));
    }
    assert_eq!(session.pending_math_input(), Some(r"\sinh"));
    assert_eq!(paragraph_math_node_count(&session), 0);

    session
        .run(Command::InsertText("+".to_owned()))
        .unwrap_or_else(|error| panic!("command terminator failed: {error}"));
    assert!(session.pending_math_input().is_none());
    assert_eq!(paragraph_math_node_count(&session), 2);
}

#[test]
fn math_paste_uses_history_and_advances_document_generation() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    let generation = session.document_generation();
    let state_id = session.history_state_id();

    let effect = session
        .run(Command::PasteText(r"\alpha+".to_owned()))
        .unwrap_or_else(|error| panic!("math paste failed: {error}"));

    assert!(effect.document_changed);
    assert!(effect.selection_changed);
    assert_eq!(session.document_generation(), generation + 1);
    assert_ne!(session.history_state_id(), state_id);
    assert_eq!(paragraph_math_node_count(&session), 2);

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("math paste undo failed: {error}"));
    assert_eq!(paragraph_math_node_count(&session), 0);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("math paste redo failed: {error}"));
    assert_eq!(paragraph_math_node_count(&session), 2);
}

#[test]
fn math_input_commands_without_math_focus_do_not_create_stale_buffers() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    assert!(matches!(
        session.run(Command::TypeMathText("\\".to_owned())),
        Err(EditError::InvalidPosition)
    ));
    assert!(session.pending_math_input().is_none());
    assert!(!session.math_input_error());
}

#[test]
fn charwise_fraction_input_remains_editable_between_its_argument_groups() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));

    for character in r"\frac{x}{y}".chars() {
        session
            .run(Command::InsertText(character.to_string()))
            .unwrap_or_else(|error| panic!("fraction prefix was not retained: {error}"));
    }

    assert!(session.pending_math_input().is_none());
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing formula");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math");
    };
    assert_eq!(math.math_tree.root.children[0].kind, MathNodeKind::Fraction);
    assert_eq!(
        math.math_tree.root.children[0].children[0].text.as_deref(),
        Some("x")
    );
    assert_eq!(
        math.math_tree.root.children[0].children[1].text.as_deref(),
        Some("y")
    );
}

#[test]
fn oversized_math_input_returns_error_without_changing_buffer_state() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    let oversized = "x".repeat(caret_math::MAX_MATH_SOURCE_BYTES + 1);
    let document = session.document().clone();
    let history_state_id = session.history_state_id();

    assert!(matches!(
        session.run(Command::InsertMathText(oversized.clone())),
        Err(EditError::InvalidMathText)
    ));
    assert!(session.pending_math_input().is_none());
    assert!(!session.math_input_error());
    assert_eq!(session.document(), &document);
    assert_eq!(session.history_state_id(), history_state_id);

    session
        .run(Command::TypeMathText("\\".to_owned()))
        .unwrap_or_else(|error| panic!("incomplete prefix failed: {error}"));
    assert_eq!(session.pending_math_input(), Some("\\"));
    assert!(matches!(
        session.run(Command::InsertMathText(oversized)),
        Err(EditError::InvalidMathText)
    ));
    assert_eq!(session.pending_math_input(), Some("\\"));
    assert!(!session.math_input_error());
}

#[test]
fn parser_expansion_budget_failure_keeps_session_document_and_history_atomic() {
    let mut seed = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    seed.run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    let mut document = seed.document().clone();
    let mut focus = seed
        .math_focus()
        .cloned()
        .unwrap_or_else(|| panic!("expected math focus"));
    let selection = seed.selection().clone();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("expected paragraph containing inline math");
    };
    let InlineNode::Math(math) = &mut paragraph.inlines[0] else {
        panic!("expected inline math");
    };
    let root_id = math.math_tree.root.id.clone();
    let source = "x".repeat(caret_math::MAX_MATH_SYNTAX_NODES - 1);
    math.math_tree = MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: source.clone(),
        root: MathNode {
            id: root_id.clone(),
            kind: MathNodeKind::Symbol,
            text: Some(source),
            attributes: BTreeMap::from([("atomClass".to_owned(), "ord".to_owned())]),
            source_range: None,
            children: Vec::new(),
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    };
    let position = MathPos {
        path: vec![root_id],
        offset: caret_math::MAX_MATH_SYNTAX_NODES - 1,
    };
    focus.position = position;
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("budget boundary document rejected: {error}"));
    session.selection = selection;
    session.math_focus = Some(focus);
    assert!(session.document.validate().is_ok());

    let document = session.document().clone();
    let selection = session.selection().clone();
    let focus = session.math_focus().cloned();
    let history_state_id = session.history_state_id();
    let generation = session.document_generation();
    let can_undo = session.can_undo();
    let can_redo = session.can_redo();
    let history_merge_epoch = session.history_merge_epoch;
    assert_eq!(
        session.run(Command::TypeMathText("x".to_owned())),
        Err(EditError::InvalidMathTree)
    );
    assert_eq!(session.document(), &document);
    assert_eq!(session.selection(), &selection);
    assert_eq!(session.math_focus(), focus.as_ref());
    assert_eq!(session.history_state_id(), history_state_id);
    assert_eq!(session.document_generation(), generation);
    assert_eq!(session.can_undo(), can_undo);
    assert_eq!(session.can_redo(), can_redo);
    assert_eq!(session.history_merge_epoch, history_merge_epoch);
    assert!(session.pending_math_input().is_none());
    assert!(!session.math_input_error());
}

#[test]
fn backspace_keeps_open_fraction_argument_as_incomplete_input() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    session
        .run(Command::TypeMathText(r"\frac{a}{".to_owned()))
        .unwrap_or_else(|error| panic!("fraction prefix failed: {error}"));
    assert_eq!(session.pending_math_input(), Some(r"\frac{a}{"));

    let effect = session
        .run(Command::Delete(Direction::Backward))
        .unwrap_or_else(|error| panic!("backspace failed: {error}"));

    assert!(effect.math_input_changed);
    assert!(!effect.math_input_error);
    assert_eq!(session.pending_math_input(), Some(r"\frac{a}"));
    assert!(!session.math_input_error());
}

#[test]
fn inline_math_command_candidate_stays_hidden_until_app_can_render_math_focus() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command query did not start: {error}"));
    session
        .run(Command::TypeCommandLine("math".to_owned()))
        .unwrap_or_else(|error| panic!("math command query failed: {error}"));
    assert!(
        session
            .command_line_candidates()
            .iter()
            .all(|candidate| candidate.command_id != "insert.inline_math" || !candidate.available)
    );

    session
        .run(Command::CancelCommandLine)
        .unwrap_or_else(|error| panic!("math command query cancellation failed: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("engine formula insertion failed: {error}"));
    assert!(session.math_focus().is_some());
    assert_eq!(paragraph_text(&session, 0), "\u{fffc}");

    session
        .run(Command::EditorKey(EditorKey::Escape))
        .unwrap_or_else(|error| panic!("Escape did not exit math focus: {error}"));
    assert!(session.math_focus().is_none());
    assert_eq!(paragraph_text(&session, 0), "\u{fffc}");
}

#[test]
fn fraction_insertion_routes_text_to_numerator_and_rejects_invalid_text_atomically() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    session
        .run(Command::InsertMathFraction)
        .unwrap_or_else(|error| panic!("fraction insertion failed: {error}"));
    session
        .run(Command::PasteText("n".to_owned()))
        .unwrap_or_else(|error| panic!("pasted text should enter numerator: {error}"));

    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing the inline formula");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math after insertion");
    };
    assert_eq!(math.math_tree.root.children[0].kind, MathNodeKind::Fraction);
    assert_eq!(
        math.math_tree.root.children[0].children[0].text.as_deref(),
        Some("n")
    );
    assert!(math.math_tree.root.children[0].children[1].text.is_none());

    let before_document = session.document().clone();
    let before_focus = session.math_focus().cloned();
    let before_history_state = session.history_state_id();
    let result = session.run(Command::InsertMathText("\n".to_owned()));

    let error_effect =
        result.unwrap_or_else(|error| panic!("invalid math text was not retained: {error}"));
    assert!(error_effect.math_input_changed);
    assert!(error_effect.math_input_error);
    assert_eq!(session.document(), &before_document);
    assert_eq!(session.math_focus(), before_focus.as_ref());
    assert_eq!(session.history_state_id(), before_history_state);
}

#[test]
fn unsupported_body_navigation_and_ime_are_rejected_while_math_has_focus() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    let document = session.document().clone();
    let math_focus = session.math_focus().cloned();
    let selection = session.selection().clone();

    let commands = [
        Command::Delete(Direction::Backward),
        Command::SetSelection(Some(Selection {
            anchor: selection.focus.clone(),
            focus: selection.focus.clone(),
        })),
        Command::UpdateComposition {
            id: 77,
            preedit: "中".to_owned(),
        },
    ];
    for command in commands {
        assert!(matches!(
            session.run(command),
            Err(EditError::UnsupportedMathTarget)
        ));
        assert_eq!(session.document(), &document);
        assert_eq!(session.math_focus(), math_focus.as_ref());
        assert_eq!(session.selection(), &selection);
        assert!(session.composition().is_none());
    }
}

#[test]
fn inline_math_edge_stays_a_grapheme_boundary_before_combining_text() {
    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = vec![InlineNode::Run(Run {
        text: "\u{301}".to_owned(),
        language: None,
        props: Default::default(),
        extra: Default::default(),
    })];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("document with combining text rejected: {error}"));

    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    assert_eq!(session.selection().focus.offset, "\u{fffc}".len());
    session
        .run(Command::ExitMath)
        .unwrap_or_else(|error| panic!("math exit failed: {error}"));
    session
        .run(Command::Delete(Direction::Backward))
        .unwrap_or_else(|error| panic!("formula edge deletion failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "\u{301}");
}

#[test]
fn math_horizontal_navigation_moves_inside_text_and_exits_at_object_edges() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("formula input failed: {error}"));

    let effect = session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("left movement inside formula failed: {error}"));
    assert!(effect.selection_changed);
    let math_focus = session
        .math_focus()
        .unwrap_or_else(|| panic!("left movement should retain formula focus"));
    assert_eq!(math_focus.position.offset, 0);

    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("left movement should exit at formula edge: {error}"));
    assert!(session.math_focus().is_none());
    assert_eq!(session.selection().focus.offset, 0);

    let mut rightward = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    rightward
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    rightward
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("right movement should exit at formula edge: {error}"));
    assert!(rightward.math_focus().is_none());
    assert_eq!(rightward.selection().focus.offset, "\u{fffc}".len());
}

#[test]
fn math_vertical_navigation_switches_fraction_and_radical_slots() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    session
        .run(Command::InsertMathFraction)
        .unwrap_or_else(|error| panic!("fraction insertion failed: {error}"));
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing formula");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math");
    };
    let fraction = &math.math_tree.root.children[0];
    let numerator_id = fraction.children[0].id.clone();
    let denominator_id = fraction.children[1].id.clone();

    session
        .run(Command::MoveMathCaret(MathDirection::Down))
        .unwrap_or_else(|error| panic!("move to denominator failed: {error}"));
    assert_eq!(
        current_math_position(&session).path.last(),
        Some(&denominator_id)
    );
    session
        .run(Command::MoveMathCaret(MathDirection::Up))
        .unwrap_or_else(|error| panic!("move to numerator failed: {error}"));
    assert_eq!(
        current_math_position(&session).path.last(),
        Some(&numerator_id)
    );

    session
        .run(Command::InsertMathRadical)
        .unwrap_or_else(|error| panic!("radical insertion into numerator failed: {error}"));
    let focus_path = current_math_position(&session).path.clone();
    let (index_id, radicand_id) = {
        let Block::Paragraph(paragraph) = &session.document.body[0] else {
            panic!("expected paragraph containing formula");
        };
        let InlineNode::Math(math) = &paragraph.inlines[0] else {
            panic!("expected inline math");
        };
        assert!(focus_path.last().is_some());
        let radical = &math.math_tree.root.children[0].children[0];
        assert_eq!(radical.kind, MathNodeKind::Radical);
        (
            radical.children[0].id.clone(),
            radical.children[1].id.clone(),
        )
    };
    session
        .run(Command::MoveMathCaret(MathDirection::Up))
        .unwrap_or_else(|error| panic!("radical index navigation failed: {error}"));
    assert_eq!(current_math_position(&session).path.last(), Some(&index_id));
    session
        .run(Command::MoveMathCaret(MathDirection::Down))
        .unwrap_or_else(|error| panic!("radicand navigation failed: {error}"));
    assert_eq!(
        current_math_position(&session).path.last(),
        Some(&radicand_id)
    );
}

#[test]
fn math_tab_moves_to_the_next_placeholder_without_wrapping() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("math prefix insertion failed: {error}"));
    session
        .run(Command::InsertMathFraction)
        .unwrap_or_else(|error| panic!("fraction insertion failed: {error}"));

    let (root_id, numerator_id, denominator_id) = {
        let Block::Paragraph(paragraph) = &session.document.body[0] else {
            panic!("expected paragraph containing formula");
        };
        let InlineNode::Math(math) = &paragraph.inlines[0] else {
            panic!("expected inline math");
        };
        assert_eq!(math.math_tree.root.children.len(), 2);
        let fraction = &math.math_tree.root.children[1];
        (
            math.math_tree.root.id.clone(),
            fraction.children[0].id.clone(),
            fraction.children[1].id.clone(),
        )
    };
    assert_eq!(
        current_math_position(&session).path.last(),
        Some(&numerator_id)
    );

    let effect = session
        .run(Command::EditorKey(EditorKey::Tab))
        .unwrap_or_else(|error| panic!("Tab should navigate to denominator: {error}"));
    assert!(effect.selection_changed);
    assert_eq!(
        current_math_position(&session).path.last(),
        Some(&denominator_id)
    );

    let effect = session
        .run(Command::EditorKey(EditorKey::Tab))
        .unwrap_or_else(|error| panic!("Tab after the final slot should be a no-op: {error}"));
    assert!(!effect.selection_changed);
    assert_eq!(
        current_math_position(&session).path.last(),
        Some(&denominator_id)
    );

    let Some(focus) = session.math_focus.as_mut() else {
        panic!("formula focus should remain active");
    };
    focus.position = MathPos {
        path: vec![root_id],
        offset: 1,
    };
    session
        .run(Command::EditorKey(EditorKey::Tab))
        .unwrap_or_else(|error| {
            panic!("Tab from the root boundary should find numerator: {error}")
        });
    assert_eq!(
        current_math_position(&session).path.last(),
        Some(&numerator_id)
    );
}

#[test]
fn math_tab_preserves_pending_input_and_rejects_navigation() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    session
        .run(Command::TypeMathText(r"\frac{".to_owned()))
        .unwrap_or_else(|error| panic!("incomplete fraction input should be buffered: {error}"));
    let before_document = session.document.clone();
    let before_focus = session.math_focus().cloned();

    assert!(matches!(
        session.run(Command::EditorKey(EditorKey::Tab)),
        Err(EditError::InvalidMathText)
    ));
    assert_eq!(session.pending_math_input(), Some(r"\frac{"));
    assert_eq!(session.math_focus(), before_focus.as_ref());
    assert_eq!(session.document, before_document);
}

#[test]
fn math_tab_visits_nested_placeholders_before_outer_slots() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    session
        .run(Command::InsertMathFraction)
        .unwrap_or_else(|error| panic!("fraction insertion failed: {error}"));
    session
        .run(Command::InsertMathRadical)
        .unwrap_or_else(|error| panic!("radical insertion failed: {error}"));

    let (root_id, fraction_id, index_id, radical_id, radicand_id, denominator_id) = {
        let Block::Paragraph(paragraph) = &session.document.body[0] else {
            panic!("expected paragraph containing formula");
        };
        let InlineNode::Math(math) = &paragraph.inlines[0] else {
            panic!("expected inline math");
        };
        let fraction = &math.math_tree.root.children[0];
        let radical = &fraction.children[0];
        assert_eq!(radical.kind, MathNodeKind::Radical);
        (
            math.math_tree.root.id.clone(),
            fraction.id.clone(),
            radical.children[0].id.clone(),
            radical.id.clone(),
            radical.children[1].id.clone(),
            fraction.children[1].id.clone(),
        )
    };
    let Some(focus) = session.math_focus.as_mut() else {
        panic!("formula focus should remain active");
    };
    focus.position = MathPos {
        path: vec![root_id, fraction_id, radical_id, index_id],
        offset: 0,
    };

    session
        .run(Command::EditorKey(EditorKey::Tab))
        .unwrap_or_else(|error| panic!("Tab should visit the nested radicand: {error}"));
    assert_eq!(
        current_math_position(&session).path.last(),
        Some(&radicand_id),
        "position={:?}",
        current_math_position(&session)
    );
    session
        .run(Command::EditorKey(EditorKey::Tab))
        .unwrap_or_else(|error| panic!("Tab should continue to the outer denominator: {error}"));
    assert_eq!(
        current_math_position(&session).path.last(),
        Some(&denominator_id)
    );
}

#[test]
fn fraction_can_be_inserted_after_a_typed_symbol() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("formula input failed: {error}"));
    session
        .run(Command::InsertMathFraction)
        .unwrap_or_else(|error| panic!("fraction should insert at the symbol edge: {error}"));

    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing formula");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math");
    };
    assert_eq!(math.math_tree.root.children.len(), 2);
    assert_eq!(math.math_tree.root.children[0].text.as_deref(), Some("x"));
    assert_eq!(math.math_tree.root.children[1].kind, MathNodeKind::Fraction);
}

#[test]
fn nested_math_structure_edit_is_undoable_without_losing_operand_text() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    session
        .run(Command::InsertMathFraction)
        .unwrap_or_else(|error| panic!("fraction insertion failed: {error}"));
    session
        .run(Command::InsertText("n".to_owned()))
        .unwrap_or_else(|error| panic!("numerator input failed: {error}"));
    session
        .run(Command::InsertMathRadical)
        .unwrap_or_else(|error| panic!("radical insertion failed: {error}"));

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo nested structure failed: {error}"));
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing formula");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math");
    };
    assert_eq!(
        math.math_tree.root.children[0].children[0].text.as_deref(),
        Some("n")
    );
    assert!(session.math_focus().is_some());
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo nested structure failed: {error}"));
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing formula after redo");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math after redo");
    };
    assert_eq!(
        math.math_tree.root.children[0].children[0].kind,
        MathNodeKind::Row
    );
    assert!(session.math_focus().is_some());
}

#[test]
fn moving_within_math_separates_history_typing_groups() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("inline math insertion failed: {error}"));
    session
        .run(Command::InsertText("x".to_owned()))
        .unwrap_or_else(|error| panic!("first formula input failed: {error}"));
    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("left movement failed: {error}"));
    session
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("right movement failed: {error}"));
    session
        .run(Command::InsertText("y".to_owned()))
        .unwrap_or_else(|error| panic!("second formula input failed: {error}"));

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo after movement failed: {error}"));
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing formula");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math");
    };
    assert_eq!(math.math_tree.root.children[0].text.as_deref(), Some("x"));
}

#[test]
fn math_horizontal_navigation_treats_non_ordinary_symbols_as_single_atoms() {
    let mut document = Document::blank();
    let math_id = NodeId::generate();
    let root_id = NodeId::generate();
    let symbol_id = NodeId::generate();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines = vec![InlineNode::Math(InlineMath {
        id: math_id.clone(),
        math_tree: MathTree {
            schema_version: MATH_TREE_SCHEMA_VERSION,
            source: "\\alpha".to_owned(),
            root: MathNode {
                id: root_id.clone(),
                kind: MathNodeKind::Row,
                text: None,
                attributes: Default::default(),
                source_range: None,
                children: vec![MathNode {
                    id: symbol_id.clone(),
                    kind: MathNodeKind::Symbol,
                    text: Some("alpha".to_owned()),
                    attributes: BTreeMap::from([("atomClass".to_owned(), "rel".to_owned())]),
                    source_range: None,
                    children: Vec::new(),
                    extra: Default::default(),
                }],
                extra: Default::default(),
            },
            extra: Default::default(),
        },
        extra: Default::default(),
    })];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("document with named math atom rejected: {error}"));
    session.math_focus = Some(crate::editing::MathFocus {
        paragraph_id: session.selection.focus.paragraph_id.clone(),
        inline_math_id: math_id,
        position: crate::editing::MathPos {
            path: vec![root_id, symbol_id],
            offset: 0,
        },
    });
    session.selection.focus.offset = "\u{fffc}".len();
    session.selection.anchor = session.selection.focus.clone();

    session
        .run(Command::MoveMathCaret(MathDirection::Right))
        .unwrap_or_else(|error| panic!("named atom right movement failed: {error}"));
    let focus = session
        .math_focus()
        .unwrap_or_else(|| panic!("named atom movement should retain math focus"));
    assert_eq!(focus.position.offset, 1);
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph containing formula");
    };
    let InlineNode::Math(math) = &paragraph.inlines[0] else {
        panic!("expected inline math");
    };
    assert!(focus.position.resolve(&math.math_tree).is_some());
}

#[test]
fn enter_accepts_command_query_while_shift_enter_cannot_edit_behind_it() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("newparagraph".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    let revision = session.document_generation();

    let shifted = session
        .run(Command::EditorKey(EditorKey::ShiftEnter))
        .unwrap_or_else(|error| panic!("Shift+Enter dispatch failed: {error}"));
    assert!(!shifted.document_changed);
    assert_eq!(session.document_generation(), revision);
    assert!(session.command_line().is_some());

    let accepted = session
        .run(Command::EditorKey(EditorKey::Enter))
        .unwrap_or_else(|error| panic!("Enter dispatch failed: {error}"));
    assert!(accepted.document_changed);
    assert!(session.command_line().is_none());
    assert_eq!(session.document().body.len(), 2);
}

#[test]
fn escape_cancels_ime_preedit_before_collapsing_an_existing_selection() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    let focus = session.selection().focus.clone();
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                offset: 0,
                ..focus.clone()
            },
            focus: focus.clone(),
        })))
        .unwrap_or_else(|error| panic!("selection setup failed: {error}"));
    let selected = session.selection().clone();
    session
        .run(Command::UpdateComposition {
            id: 4,
            preedit: "にほん".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition update failed: {error}"));

    let effect = session
        .run(Command::EditorKey(EditorKey::Escape))
        .unwrap_or_else(|error| panic!("Escape failed: {error}"));

    assert!(effect.composition_changed);
    assert!(!effect.selection_changed);
    assert!(session.composition().is_none());
    assert_eq!(*session.selection(), selected);
    assert_eq!(paragraph_text(&session, 0), "hello");
}

#[test]
fn tab_completes_a_filtered_command_without_editing_the_document() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("sec".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    let command_line = session
        .command_line()
        .unwrap_or_else(|| panic!("command query should remain active"));
    assert_eq!(
        command_line
            .selected_candidate()
            .map(|candidate| candidate.alias),
        Some(std::borrow::Cow::Borrowed("\\section"))
    );
    let revision = session.document_generation();

    session
        .run(Command::TabCommandLine)
        .unwrap_or_else(|error| panic!("Tab completion failed: {error}"));

    let command_line = session
        .command_line()
        .unwrap_or_else(|| panic!("Tab should keep command mode active"));
    assert_eq!(command_line.query, "section");
    assert_eq!(session.document_generation(), revision);
    assert_eq!(paragraph_text(&session, 0), "");

    session
        .run(Command::TypeCommandLine(" ".to_owned()))
        .unwrap_or_else(|error| panic!("space handling failed: {error}"));
    assert_eq!(
        session
            .command_line()
            .map(|command_line| command_line.query.as_str()),
        Some("section")
    );
}

#[test]
fn command_line_preedit_blocks_tab_and_cancel_reports_composition_change() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .input_method_command_line("ni".to_owned(), String::new(), 0, 0, 2)
        .unwrap_or_else(|error| panic!("command-line preedit failed: {error}"));
    let generation = session.document_generation();

    session
        .run(Command::TabCommandLine)
        .unwrap_or_else(|error| panic!("Tab during preedit should be ignored: {error}"));

    let line = session
        .command_line()
        .unwrap_or_else(|| panic!("preedit should keep command line active"));
    assert_eq!(line.preedit, "ni");
    assert_eq!(line.query, "");
    assert_eq!(session.document_generation(), generation);
    assert_eq!(paragraph_text(&session, 0), "");

    let effect = session
        .run(Command::CancelCommandLine)
        .unwrap_or_else(|error| panic!("command-line cancellation failed: {error}"));
    assert!(effect.composition_changed);
    assert!(session.command_line().is_none());
    assert_eq!(session.document_generation(), generation);
}

#[test]
fn font_picker_cursor_never_stops_inside_a_combining_grapheme() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("font".to_owned()))
        .unwrap_or_else(|error| panic!("font command query failed: {error}"));
    session
        .run_command_line_with_font_families(
            Command::AcceptCommandLine,
            vec!["Avenir Next".to_owned()],
        )
        .unwrap_or_else(|error| panic!("font picker did not open: {error}"));
    session
        .run(Command::TypeCommandLine("\u{301}".to_owned()))
        .unwrap_or_else(|error| panic!("combining mark input failed: {error}"));
    session
        .run(Command::MoveCommandLineCursor(Direction::Backward))
        .unwrap_or_else(|error| panic!("font query cursor move failed: {error}"));
    session
        .run(Command::TypeCommandLine("e".to_owned()))
        .unwrap_or_else(|error| panic!("base character input failed: {error}"));

    let line = session
        .command_line()
        .unwrap_or_else(|| panic!("font picker should remain active"));
    assert_eq!(line.query, "e\u{301}");
    assert_eq!(line.cursor, line.query.len());
    assert!(line.query.is_char_boundary(line.cursor));
    assert!(!line.query[..line.cursor].is_empty());
    assert_eq!(line.query[..line.cursor].graphemes(true).count(), 1);
}

#[test]
fn default_font_picker_navigates_script_and_script_language_targets() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    let font_targets = vec![
        (
            "Latn · Latin".to_owned(),
            DocumentFontTarget::Script("Latn".to_owned()),
        ),
        (
            "Cyrl · Cyrillic".to_owned(),
            DocumentFontTarget::Script("Cyrl".to_owned()),
        ),
    ];
    let fonts = vec!["Avenir Next".to_owned()];
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("defaultfont".to_owned()))
        .unwrap_or_else(|error| panic!("default-font query failed: {error}"));
    session
        .run_command_line_with_font_choices(Command::AcceptCommandLine, fonts.clone(), font_targets)
        .unwrap_or_else(|error| panic!("default-font picker did not open: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(crate::editing::CommandLineMode::DefaultFontTarget)
    );
    assert_eq!(
        session.command_line_candidates()[0].command_id,
        "command_line.close_font_picker"
    );

    session
        .run(Command::SelectCommandLineCandidate(2))
        .unwrap_or_else(|error| panic!("Cyrillic script selection failed: {error}"));
    session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("script scope did not open: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(crate::editing::CommandLineMode::DefaultFontScope)
    );

    session
        .run(Command::SelectCommandLineCandidate(2))
        .unwrap_or_else(|error| panic!("Script+ selection failed: {error}"));
    session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("language override input did not open: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(crate::editing::CommandLineMode::DefaultFontLanguage)
    );
    assert_eq!(
        session.command_line().map(|line| line.prefix()),
        Some("\\defaultfont Cyrl: ".to_owned())
    );

    session
        .run(Command::TypeCommandLine("sr".to_owned()))
        .unwrap_or_else(|error| panic!("language tag input failed: {error}"));
    session
        .run(Command::MoveCommandLineSelection(Direction::Forward))
        .unwrap_or_else(|error| panic!("language target selection failed: {error}"));
    session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("language font list did not open: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.prefix()),
        Some("\\defaultfont Cyrl:sr ".to_owned())
    );

    session
        .run(Command::TypeCommandLine("Avenir".to_owned()))
        .unwrap_or_else(|error| panic!("family query failed: {error}"));
    session
        .run(Command::MoveCommandLineSelection(Direction::Forward))
        .unwrap_or_else(|error| panic!("family selection failed: {error}"));
    session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("language font preference failed: {error}"));

    assert_eq!(
        session
            .document()
            .global_settings
            .language_script_font_families["Cyrl"]["sr"],
        "Avenir Next"
    );
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(crate::editing::CommandLineMode::DefaultFontTarget)
    );
}

#[test]
fn font_command_opens_unicode_family_picker_and_applies_selected_family() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("font".to_owned()))
        .unwrap_or_else(|error| panic!("font command query failed: {error}"));

    session
        .run_command_line_with_font_families(
            Command::TabCommandLine,
            vec!["Avenir Next".to_owned(), "思源 宋体".to_owned()],
        )
        .unwrap_or_else(|error| panic!("font command alias completion failed: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.mode),
        Some(crate::editing::CommandLineMode::Command)
    );

    session
        .run_command_line_with_font_families(
            Command::AcceptCommandLine,
            vec!["Avenir Next".to_owned(), "思源 宋体".to_owned()],
        )
        .unwrap_or_else(|error| panic!("font picker did not open: {error}"));
    let command_line = session
        .command_line()
        .unwrap_or_else(|| panic!("font picker should stay active"));
    assert_eq!(
        command_line.mode,
        crate::editing::CommandLineMode::FontFamily
    );
    assert_eq!(command_line.prefix(), "\\font ");

    session
        .run(Command::TypeCommandLine("\\".to_owned()))
        .unwrap_or_else(|error| panic!("font picker slash input failed: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.query.as_str()),
        Some("\\")
    );
    session
        .run(Command::BackspaceCommandLine)
        .unwrap_or_else(|error| panic!("font picker slash removal failed: {error}"));

    session
        .input_method_command_line("拼音".to_owned(), String::new(), 0, 0, 1)
        .unwrap_or_else(|error| panic!("font picker preedit failed: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.preedit.as_str()),
        Some("拼音")
    );
    session
        .input_method_command_line(String::new(), "思源 宋体".to_owned(), 0, 0, 0)
        .unwrap_or_else(|error| panic!("font picker IME commit failed: {error}"));
    assert_eq!(
        session.command_line().map(|line| line.query.as_str()),
        Some("思源 宋体")
    );

    let candidate = session
        .command_line_candidates()
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("matching family should be suggested"));
    assert_eq!(candidate.command_id, "text.format.font_family");
    assert_eq!(candidate.value.as_deref(), Some("思源 宋体"));

    session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("font family command failed: {error}"));
    assert!(session.command_line().is_none());
    session
        .run(Command::InsertText("字".to_owned()))
        .unwrap_or_else(|error| panic!("font-styled input failed: {error}"));
    let paragraph = session
        .paragraph_for(&session.selection.focus)
        .unwrap_or_else(|error| panic!("paragraph lookup failed: {error}"));
    let Some(InlineNode::Environment(environment)) = paragraph.inlines.first() else {
        panic!("font command should create a Font environment");
    };
    assert_eq!(environment.kind, InlineEnvironmentKind::Font);
}

#[test]
fn literal_slash_at_empty_query_does_not_open_highlighted_font_picker() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    let font_candidate_index = session
        .command_line_candidates()
        .iter()
        .position(|candidate| candidate.command_id == "text.format.font")
        .unwrap_or_else(|| panic!("font command candidate is missing"));
    session
        .run(Command::SelectCommandLineCandidate(font_candidate_index))
        .unwrap_or_else(|error| panic!("font candidate selection failed: {error}"));

    session
        .run_command_line_with_font_families(
            Command::TypeCommandLine("\\".to_owned()),
            vec!["Avenir Next".to_owned()],
        )
        .unwrap_or_else(|error| panic!("literal slash input failed: {error}"));

    assert!(session.command_line().is_none());
    assert_eq!(paragraph_text(&session, 0), "\\");
}

#[test]
fn font_picker_ime_replacement_replaces_the_requested_query_range() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run_command_line_with_font_families(
            Command::BeginCommandLine,
            vec!["Avenir Next".to_owned()],
        )
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("font".to_owned()))
        .unwrap_or_else(|error| panic!("font command query failed: {error}"));
    session
        .run_command_line_with_font_families(
            Command::AcceptCommandLine,
            vec!["Avenir Next".to_owned()],
        )
        .unwrap_or_else(|error| panic!("font picker did not open: {error}"));
    session
        .run(Command::TypeCommandLine("ABCD".to_owned()))
        .unwrap_or_else(|error| panic!("font picker query failed: {error}"));
    session
        .run(Command::MoveCommandLineCursor(Direction::Backward))
        .unwrap_or_else(|error| panic!("font picker cursor move failed: {error}"));
    session
        .input_method_command_line(String::new(), "界".to_owned(), -1, 1, 0)
        .unwrap_or_else(|error| panic!("font picker replacement failed: {error}"));

    assert_eq!(
        session.command_line().map(|line| line.query.as_str()),
        Some("AB界D")
    );
}

#[test]
fn unavailable_candidates_are_hidden_until_prefix_matches_and_cannot_execute() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    let empty_candidates = session
        .command_line()
        .unwrap_or_else(|| panic!("command line should remain active"))
        .candidates();
    assert!(empty_candidates.iter().all(|candidate| candidate.available));
    assert!(
        !empty_candidates
            .iter()
            .any(|candidate| candidate.alias == "\\chapter")
    );

    session
        .run(Command::TypeCommandLine("chapter".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    let command_line = session
        .command_line()
        .unwrap_or_else(|| panic!("command query should remain active"));
    let candidate = command_line
        .selected_candidate()
        .unwrap_or_else(|| panic!("matching unavailable candidate should be visible"));
    assert_eq!(candidate.alias, "\\chapter");
    assert!(!candidate.available);
    let generation = session.document_generation();

    session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("disabled command should be safely ignored: {error}"));

    assert!(session.command_line().is_some());
    assert_eq!(session.document_generation(), generation);
    assert_eq!(paragraph_text(&session, 0), "");
}

#[test]
fn custom_environment_is_not_registered_as_a_user_command_candidate() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank document rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    assert!(
        session
            .command_line_candidates()
            .iter()
            .all(|candidate| candidate.alias != "\\environment")
    );
    session
        .run(Command::TypeCommandLine("env".to_owned()))
        .unwrap_or_else(|error| panic!("command prefix failed: {error}"));
    assert!(
        session
            .command_line_candidates()
            .iter()
            .all(|candidate| candidate.alias != "\\environment")
    );
}

#[test]
fn environment_creation_candidates_are_disabled_in_section_titles() {
    let mut document = Document::blank();
    let title = Paragraph::empty(NodeId::generate());
    let title_id = title.id.clone();
    let section = BlockEnvironment {
        id: NodeId::generate(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: BTreeMap::new(),
        title: Some(title),
        children: vec![Block::Paragraph(Paragraph::empty(NodeId::generate()))],
        extra: BTreeMap::new(),
    };
    document.body = vec![Block::Environment(section)];
    let mut session = Session::new(document)
        .unwrap_or_else(|error| panic!("valid section document rejected: {error}"));
    assert_eq!(session.selection().focus.paragraph_id, title_id);

    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("sec".to_owned()))
        .unwrap_or_else(|error| panic!("command prefix failed: {error}"));

    let candidate = session
        .command_line_candidates()
        .into_iter()
        .find(|candidate| candidate.alias == "\\section")
        .unwrap_or_else(|| panic!("section command is missing from prefix results"));
    assert!(!candidate.available);
    let document_generation = session.document_generation();
    session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("disabled command acceptance failed: {error}"));
    assert!(session.command_line().is_some());
    assert_eq!(session.document_generation(), document_generation);
}

#[test]
fn accepting_the_newline_command_inserts_a_soft_break_and_closes_the_query() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("new".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    session
        .run(Command::SelectCommandLineCandidate(1))
        .unwrap_or_else(|error| panic!("newline candidate selection failed: {error}"));

    let effect = session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("newline command failed: {error}"));

    assert!(effect.document_changed);
    assert_eq!(paragraph_text(&session, 0), "hello\n");
    assert!(session.command_line().is_none());

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undoing the newline command failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hello");

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redoing the newline command failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hello\n");
}

#[test]
fn accepting_newparagraph_command_creates_a_paragraph_and_is_one_undo_step() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("newparagraph".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    session
        .run(Command::SelectCommandLineCandidate(0))
        .unwrap_or_else(|error| panic!("new paragraph candidate selection failed: {error}"));

    let effect = session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("new paragraph command failed: {error}"));

    assert!(effect.document_changed);
    assert_eq!(paragraph_text(&session, 0), "hello");
    assert_eq!(paragraph_text(&session, 1), "");
    assert!(session.command_line().is_none());

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undoing new paragraph failed: {error}"));
    assert_eq!(session.document().body.len(), 1);
    assert_eq!(paragraph_text(&session, 0), "hello");

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redoing new paragraph failed: {error}"));
    assert_eq!(paragraph_text(&session, 1), "");
}

#[test]
fn first_line_indent_command_is_available_anywhere_and_is_undoable() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    let text_before = paragraph_text(&session, 0);
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    session
        .run(Command::TypeCommandLine("firstlineindent".to_owned()))
        .unwrap_or_else(|error| panic!("command query failed: {error}"));
    assert!(
        session
            .command_line_candidates()
            .iter()
            .any(|candidate| { candidate.alias == "\\firstlineindent" && candidate.available })
    );
    let applied = session
        .run(Command::AcceptCommandLine)
        .unwrap_or_else(|error| panic!("first-line indent command failed: {error}"));

    assert!(applied.document_changed);
    assert_eq!(paragraph_text(&session, 0), text_before);
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(
        paragraph
            .layout
            .as_ref()
            .and_then(|layout| layout.first_line_indent_chars),
        Some(2)
    );

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(paragraph.layout, None);
    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo failed: {error}"));
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(
        paragraph
            .layout
            .as_ref()
            .and_then(|layout| layout.first_line_indent_chars),
        Some(2)
    );
}

#[test]
fn tab_inserts_two_character_width_spaces_at_the_current_caret_position() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("abcd".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("first caret move failed: {error}"));
    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("second caret move failed: {error}"));
    let effect = session
        .run(Command::EditorKey(EditorKey::Tab))
        .unwrap_or_else(|error| panic!("Tab insertion failed: {error}"));
    assert!(effect.document_changed);
    assert_eq!(paragraph_text(&session, 0), "ab\u{2003}\u{2003}cd");
}

#[test]
fn width_commands_insert_spaces_with_their_named_widths() {
    for (query, expected_text) in [
        ("singlecharspace", "\u{2003}"),
        ("doublecharspace", "\u{2003}\u{2003}"),
    ] {
        let mut session = Session::new(Document::blank())
            .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
        session
            .run(Command::BeginCommandLine)
            .unwrap_or_else(|error| panic!("command line did not start: {error}"));
        session
            .run(Command::TypeCommandLine(query.to_owned()))
            .unwrap_or_else(|error| panic!("space command query failed: {error}"));
        let candidate = session
            .command_line_candidates()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("\\{query} candidate missing"));
        assert!(candidate.available);
        session
            .run(Command::AcceptCommandLine)
            .unwrap_or_else(|error| panic!("{query} command failed: {error}"));
        assert_eq!(paragraph_text(&session, 0), expected_text);
    }
}

#[test]
fn multiline_paste_rebuilds_structure_navigation_for_created_paragraphs() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));

    session
        .run(Command::PasteText("first\nsecond".to_owned()))
        .unwrap_or_else(|error| panic!("multiline paste failed: {error}"));

    let Block::Paragraph(first) = &session.document.body[0] else {
        panic!("first pasted block should be a paragraph");
    };
    let Block::Paragraph(second) = &session.document.body[1] else {
        panic!("second pasted block should be a paragraph");
    };
    let first_id = first.id.clone();
    let second_id = second.id.clone();
    assert_eq!(
        session.structure_index().navigation_paragraphs(),
        &[first_id.clone(), second_id]
    );

    session
        .run(Command::MoveStructure(Direction::Backward))
        .unwrap_or_else(|error| panic!("structure navigation after paste failed: {error}"));
    assert_eq!(session.selection().focus.paragraph_id, first_id);
}

#[test]
fn structure_navigation_ends_the_current_deletion_undo_group() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertParagraphBreak)
        .unwrap_or_else(|error| panic!("paragraph split failed: {error}"));
    session
        .run(Command::InsertText("ab".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    let Block::Paragraph(second) = &session.document.body[1] else {
        panic!("second block should be a paragraph");
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                paragraph_id: second.id.clone(),
                offset: 0,
                ..session.selection().focus.clone()
            },
            focus: Pos {
                paragraph_id: second.id.clone(),
                offset: 0,
                ..session.selection().focus.clone()
            },
        })))
        .unwrap_or_else(|error| {
            panic!("moving the caret to the second paragraph start failed: {error}")
        });
    session
        .run(Command::Delete(Direction::Forward))
        .unwrap_or_else(|error| panic!("first forward deletion failed: {error}"));
    session
        .run(Command::MoveStructure(Direction::Backward))
        .unwrap_or_else(|error| panic!("moving to the previous paragraph failed: {error}"));
    session
        .run(Command::MoveStructure(Direction::Forward))
        .unwrap_or_else(|error| panic!("moving to the next paragraph failed: {error}"));
    session
        .run(Command::Delete(Direction::Forward))
        .unwrap_or_else(|error| panic!("second forward deletion failed: {error}"));

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undoing the second deletion failed: {error}"));

    assert_eq!(paragraph_text(&session, 1), "b");
}

#[test]
fn empty_command_tab_inserts_a_literal_prefix() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));
    let revision = session.document_generation();

    session
        .run(Command::TabCommandLine)
        .unwrap_or_else(|error| panic!("empty-query Tab failed: {error}"));

    assert_eq!(paragraph_text(&session, 0), "\\");
    assert!(session.command_line().is_none());
    assert_eq!(session.document_generation(), revision + 1);
}

#[test]
fn repeating_the_command_prefix_with_an_empty_query_inserts_a_literal_prefix() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("command line did not start: {error}"));

    session
        .run(Command::TypeCommandLine("\\".to_owned()))
        .unwrap_or_else(|error| panic!("repeated command prefix failed: {error}"));

    assert_eq!(paragraph_text(&session, 0), "\\");
    assert!(session.command_line().is_none());
}

#[test]
fn command_line_does_not_start_during_input_composition() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::UpdateComposition {
            id: 1,
            preedit: "にほん".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition update failed: {error}"));

    session
        .run(Command::BeginCommandLine)
        .unwrap_or_else(|error| panic!("begin command during composition failed: {error}"));

    assert!(session.command_line().is_none());
    assert_eq!(
        session
            .composition()
            .map(|composition| composition.preedit.as_str()),
        Some("にほん")
    );
    assert_eq!(paragraph_text(&session, 0), "");
}

#[test]
fn undo_and_redo_restore_typed_text_and_selection() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::InsertText(" world".to_owned()))
        .unwrap_or_else(|error| panic!("second insertion failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hello world");

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "");
    assert_eq!(session.selection().focus.offset, 0);

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("redo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hello world");
    assert_eq!(session.selection().focus.offset, "hello world".len());
}

#[test]
fn coalesced_typing_counts_retained_history_memory_instead_of_temporary_snapshots() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    for _ in 0..1_000 {
        session
            .run(Command::InsertText("x".to_owned()))
            .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    }
    assert!(session.history_estimated_bytes < history::history_memory_limit());

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("coalesced typing undo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "");
}

#[test]
fn undoing_a_paragraph_split_restores_the_original_paragraph_id_and_text() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("alpha beta".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    let original_id = session.selection().focus.paragraph_id.clone();
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                offset: 5,
                ..session.selection().focus.clone()
            },
            focus: Pos {
                offset: 5,
                ..session.selection().focus.clone()
            },
        })))
        .unwrap_or_else(|error| panic!("selection failed: {error}"));
    session
        .run(Command::InsertParagraphBreak)
        .unwrap_or_else(|error| panic!("paragraph split failed: {error}"));
    assert_eq!(session.document.body.len(), 2);

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("split undo failed: {error}"));
    assert_eq!(session.document.body.len(), 1);
    assert_eq!(paragraph_text(&session, 0), "alpha beta");
    assert_eq!(session.selection().focus.paragraph_id, original_id);

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("split redo failed: {error}"));
    assert_eq!(session.document.body.len(), 2);
    assert_eq!(paragraph_text(&session, 0), "alpha");
    assert_eq!(paragraph_text(&session, 1), " beta");
}

#[test]
fn editing_after_undo_keeps_the_old_redo_branch() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("first".to_owned()))
        .unwrap_or_else(|error| panic!("first insertion failed: {error}"));
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo failed: {error}"));
    session
        .run(Command::InsertText("second".to_owned()))
        .unwrap_or_else(|error| panic!("branch insertion failed: {error}"));

    assert_eq!(paragraph_text(&session, 0), "second");
    assert!(session.edit_history.branches().count() > 0);
}

#[test]
fn undo_then_new_typing_does_not_merge_into_the_old_branch_ancestor() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("a".to_owned()))
        .unwrap_or_else(|error| panic!("first insertion failed: {error}"));
    session
        .run(Command::Delete(Direction::Backward))
        .unwrap_or_else(|error| panic!("deletion failed: {error}"));
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("deletion undo failed: {error}"));
    session
        .run(Command::InsertText("b".to_owned()))
        .unwrap_or_else(|error| panic!("branch insertion failed: {error}"));

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("branch insertion undo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "a");
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("ancestor insertion undo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "");
    assert!(session.edit_history.branches().count() > 0);
}

#[test]
fn ime_replacement_is_one_undoable_transaction_that_restores_replaced_text() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("hello".to_owned()))
        .unwrap_or_else(|error| panic!("initial insertion failed: {error}"));
    let position = session.selection().focus.clone();
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                offset: 1,
                ..position.clone()
            },
            focus: Pos {
                offset: 4,
                ..position
            },
        })))
        .unwrap_or_else(|error| panic!("selection failed: {error}"));

    session
        .commit_composition_replacement(None, 10, 11, 0, 0, "i")
        .unwrap_or_else(|error| panic!("IME replacement failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hio");

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("IME replacement undo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hello");

    session
        .run(Command::Redo)
        .unwrap_or_else(|error| panic!("IME replacement redo failed: {error}"));
    assert_eq!(paragraph_text(&session, 0), "hio");
}

#[test]
fn undo_restores_a_cross_paragraph_deletion_and_original_paragraph_ids() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("alpha".to_owned()))
        .unwrap_or_else(|error| panic!("first insertion failed: {error}"));
    session
        .run(Command::InsertParagraphBreak)
        .unwrap_or_else(|error| panic!("paragraph split failed: {error}"));
    session
        .run(Command::InsertText("gamma".to_owned()))
        .unwrap_or_else(|error| panic!("second insertion failed: {error}"));
    let first_id = match &session.document.body[0] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => panic!("first block should be a paragraph"),
    };
    let second_id = match &session.document.body[1] {
        Block::Paragraph(paragraph) => paragraph.id.clone(),
        _ => panic!("second block should be a paragraph"),
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                paragraph_id: first_id.clone(),
                offset: 2,
                ..session.selection().focus.clone()
            },
            focus: Pos {
                paragraph_id: second_id.clone(),
                offset: 3,
                ..session.selection().focus.clone()
            },
        })))
        .unwrap_or_else(|error| panic!("cross-paragraph selection failed: {error}"));
    session
        .run(Command::Delete(Direction::Backward))
        .unwrap_or_else(|error| panic!("cross-paragraph delete failed: {error}"));

    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("cross-paragraph undo failed: {error}"));
    assert_eq!(session.document.body.len(), 2);
    assert_eq!(paragraph_text(&session, 0), "alpha");
    assert_eq!(paragraph_text(&session, 1), "gamma");
    assert!(matches!(&session.document.body[0], Block::Paragraph(p) if p.id == first_id));
    assert!(matches!(&session.document.body[1], Block::Paragraph(p) if p.id == second_id));
}

#[test]
fn undo_is_rejected_while_input_composition_is_active() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::UpdateComposition {
            id: 1,
            preedit: "にほん".to_owned(),
        })
        .unwrap_or_else(|error| panic!("composition update failed: {error}"));
    assert_eq!(
        session.run(Command::Undo),
        Err(EditError::CompositionPending)
    );
    assert_eq!(paragraph_text(&session, 0), "");
}

fn inline_math_ids(session: &Session) -> Vec<NodeId> {
    let Block::Paragraph(paragraph) = &session.document.body[0] else {
        panic!("expected a paragraph");
    };
    paragraph
        .inlines
        .iter()
        .filter_map(|node| match node {
            InlineNode::Math(math) => Some(math.id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn arrow_enters_and_exits_consecutive_inline_math() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("a".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("first formula insertion failed: {error}"));
    session
        .run(Command::ExitMath)
        .unwrap_or_else(|error| panic!("first formula exit failed: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("second formula insertion failed: {error}"));
    session
        .run(Command::ExitMath)
        .unwrap_or_else(|error| panic!("second formula exit failed: {error}"));
    let ids = inline_math_ids(&session);
    assert_eq!(ids.len(), 2);
    let first = &ids[0];
    let second = &ids[1];
    let between = "a".len() + "\u{fffc}".len();

    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("entering the second formula failed: {error}"));
    assert_eq!(
        session.math_focus().map(|focus| &focus.inline_math_id),
        Some(second)
    );
    assert_eq!(session.selection().focus.offset, between + "\u{fffc}".len());
    assert_eq!(session.selection().focus.affinity, Affinity::Downstream);

    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("leaving the second formula failed: {error}"));
    assert!(session.math_focus().is_none());
    assert_eq!(session.selection().focus.offset, between);

    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("entering the first formula failed: {error}"));
    assert_eq!(
        session.math_focus().map(|focus| &focus.inline_math_id),
        Some(first)
    );

    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("leaving the first formula failed: {error}"));
    assert!(session.math_focus().is_none());
    assert_eq!(session.selection().focus.offset, "a".len());

    session
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("re-entering the first formula failed: {error}"));
    assert_eq!(
        session.math_focus().map(|focus| &focus.inline_math_id),
        Some(first)
    );
    session
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("leaving toward the second formula failed: {error}"));
    assert!(session.math_focus().is_none());
    assert_eq!(session.selection().focus.offset, between);
    session
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| {
            panic!("entering the second formula from the left failed: {error}")
        });
    assert_eq!(
        session.math_focus().map(|focus| &focus.inline_math_id),
        Some(second)
    );
}

#[test]
fn inline_math_entry_does_not_create_an_undo_step() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("a".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("formula insertion failed: {error}"));
    session
        .run(Command::ExitMath)
        .unwrap_or_else(|error| panic!("formula exit failed: {error}"));
    let history = session.history_state_id();
    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("formula entry failed: {error}"));
    assert!(session.math_focus().is_some());
    assert_eq!(session.history_state_id(), history);
    session
        .run(Command::Undo)
        .unwrap_or_else(|error| panic!("undo after entry failed: {error}"));
    assert!(session.math_focus().is_none());
    assert_eq!(paragraph_text(&session, 0), "a");
}

#[test]
fn body_selection_wrapper_clears_math_focus_and_direct_set_selection_stays_rejected() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("ab".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("formula insertion failed: {error}"));
    let focus = session.math_focus().cloned();
    let selection = session.selection().clone();
    let invalid = Selection {
        focus: Pos {
            offset: 99,
            ..selection.focus.clone()
        },
        ..selection.clone()
    };
    assert!(
        session
            .set_selection_with_visual_affinity(
                Some(invalid),
                caret_layout::VisualAffinity::Downstream
            )
            .is_err()
    );
    assert_eq!(session.math_focus(), focus.as_ref());
    assert_eq!(session.selection(), &selection);
    assert!(matches!(
        session.run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                offset: 0,
                ..selection.focus.clone()
            },
            focus: selection.focus.clone(),
        }))),
        Err(EditError::UnsupportedMathTarget)
    ));
    assert_eq!(session.math_focus(), focus.as_ref());

    session
        .set_selection_with_visual_affinity(
            Some(Selection {
                anchor: Pos {
                    offset: 0,
                    affinity: Affinity::Upstream,
                    ..selection.focus.clone()
                },
                focus: selection.focus.clone(),
            }),
            caret_layout::VisualAffinity::Downstream,
        )
        .unwrap_or_else(|error| panic!("body selection should leave the formula: {error}"));
    assert!(session.math_focus().is_none());
    assert_eq!(session.selection().anchor.offset, 0);
    assert_eq!(session.selection().focus.offset, selection.focus.offset);
    assert_ne!(session.selection().anchor, session.selection().focus);
}

#[test]
fn inline_math_reenters_after_cdoc_roundtrip() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertText("ab".to_owned()))
        .unwrap_or_else(|error| panic!("text insertion failed: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("formula insertion failed: {error}"));
    session
        .run(Command::ExitMath)
        .unwrap_or_else(|error| panic!("formula exit failed: {error}"));
    let math_id = inline_math_ids(&session)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("formula should persist"));
    let bytes = caret_doc::CdocFile::new(session.document().clone())
        .and_then(|file| file.to_bytes())
        .unwrap_or_else(|error| panic!("formula document should save: {error}"));
    let opened = caret_doc::open(&bytes)
        .unwrap_or_else(|error| panic!("formula document should reopen: {error}"));
    let caret_doc::OpenedDocument::Editable(file) = opened else {
        panic!("inline math should remain editable");
    };
    let mut reopened = Session::new(file.document().clone())
        .unwrap_or_else(|error| panic!("reopened document rejected: {error}"));
    assert!(reopened.math_focus().is_none());
    let paragraph_id = reopened.selection().focus.paragraph_id.clone();
    let story = reopened.selection().focus.story.clone();
    reopened
        .run(Command::SetSelection(Some(Selection {
            anchor: Pos {
                story: story.clone(),
                paragraph_id: paragraph_id.clone(),
                offset: "ab".len(),
                affinity: Affinity::Downstream,
            },
            focus: Pos {
                story,
                paragraph_id,
                offset: "ab".len(),
                affinity: Affinity::Downstream,
            },
        })))
        .unwrap_or_else(|error| panic!("caret before the reopened formula failed: {error}"));
    reopened
        .run(Command::MoveCaret(Direction::Forward))
        .unwrap_or_else(|error| panic!("reopened formula should accept entry: {error}"));
    assert_eq!(
        reopened.math_focus().map(|focus| &focus.inline_math_id),
        Some(&math_id)
    );
    assert_eq!(
        reopened.selection().focus.offset,
        "ab".len() + "\u{fffc}".len()
    );
}

#[test]
fn entering_inline_math_clears_block_environment_navigation_state() {
    let mut session = Session::new(Document::blank())
        .unwrap_or_else(|error| panic!("blank session rejected: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("formula insertion failed: {error}"));
    session
        .run(Command::ExitMath)
        .unwrap_or_else(|error| panic!("formula exit failed: {error}"));
    let environment_id = NodeId::generate();
    session.block_environment_exit = Some((environment_id.clone(), Direction::Forward));
    session.selected_block_environment = Some(environment_id.clone());
    session.selected_block_environments = vec![environment_id];
    session.structure_selection = Some(StructureSelection {
        anchor: session.selection.focus.clone(),
        focus: session.selection.focus.clone(),
    });
    session
        .run(Command::MoveCaret(Direction::Backward))
        .unwrap_or_else(|error| panic!("formula entry failed: {error}"));
    assert!(session.math_focus().is_some());
    assert!(session.block_environment_exit.is_none());
    assert!(session.selected_block_environment.is_none());
    assert!(session.selected_block_environments.is_empty());
    assert!(session.structure_selection.is_none());
}

#[test]
fn selecting_a_block_environment_clears_math_focus() {
    let body_id = NodeId::generate();
    let environment_id = NodeId::generate();
    let mut document = Document::blank();
    document.body = vec![Block::Environment(BlockEnvironment {
        id: environment_id.clone(),
        kind: BlockEnvironmentKind::Section,
        definition_id: None,
        props: Default::default(),
        title: Some(Paragraph {
            id: NodeId::generate(),
            language: None,
            layout: None,
            inlines: vec![InlineNode::Run(Run {
                text: "title".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            })],
            mark_style: Default::default(),
            props: Default::default(),
            extra: Default::default(),
        }),
        children: vec![Block::Paragraph(Paragraph {
            id: body_id.clone(),
            language: None,
            layout: None,
            inlines: Vec::new(),
            mark_style: Default::default(),
            props: Default::default(),
            extra: Default::default(),
        })],
        extra: Default::default(),
    })];
    let mut session =
        Session::new(document).unwrap_or_else(|error| panic!("section document rejected: {error}"));
    let body = Pos {
        story: StoryId::Body,
        paragraph_id: body_id,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    session
        .run(Command::SetSelection(Some(Selection {
            anchor: body.clone(),
            focus: body,
        })))
        .unwrap_or_else(|error| panic!("body caret failed: {error}"));
    session
        .run(Command::InsertInlineMath)
        .unwrap_or_else(|error| panic!("formula insertion failed: {error}"));
    session.math_input_buffer = Some("\\frac".to_owned());
    session.math_input_error = true;
    session
        .select_block_environment(environment_id.clone())
        .unwrap_or_else(|error| panic!("environment selection failed: {error}"));
    assert!(session.math_focus().is_none());
    assert!(session.math_input_buffer.is_none());
    assert!(!session.math_input_error);
    assert_eq!(session.selected_block_environment(), Some(&environment_id));
}
