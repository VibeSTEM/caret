//! 验证正文与数学内容流格式转换的共同语义和失败边界。

use super::*;
use caret_doc::{InlineMath, MathNode, OpaqueNode, Run, SoftBreak};
use serde_json::json;

fn text<T>(value: &str) -> FlowContent<T> {
    FlowContent::Text(TextFlow {
        runs: vec![Run {
            text: value.to_owned(),
            language: None,
            props: Default::default(),
            extra: Default::default(),
        }],
        extra: Default::default(),
    })
}

fn environment<T>(kind: InlineEnvironmentKind, children: Vec<FlowContent<T>>) -> FlowContent<T> {
    FlowContent::Environment(InlineEnvironment {
        id: NodeId::generate(),
        kind,
        definition_id: None,
        props: Default::default(),
        children,
        extra: Default::default(),
    })
}

fn context() -> InlineFormatContext {
    InlineFormatContext {
        inherited: InheritedInlineStyle::default(),
        content_depth: 1,
        max_depth: 64,
    }
}

fn fixture<T>() -> Vec<FlowContent<T>> {
    vec![
        environment(InlineEnvironmentKind::Bold, vec![text("abc")]),
        text("xy"),
    ]
}

fn projection<T>(contents: &[FlowContent<T>]) -> Value {
    Value::Array(contents.iter().map(|content| match content {
        FlowContent::Text(text) => json!({"text": text}),
        FlowContent::Environment(environment) => json!({"kind": environment.kind, "definition": environment.definition_id, "props": environment.props, "extra": environment.extra, "children": projection(&environment.children)}),
        FlowContent::Math(_) => json!({"atom": true}),
        FlowContent::SoftBreak(value) => json!({"softBreak": value}),
        FlowContent::Opaque(value) => json!({"opaque": value.raw}),
    }).collect())
}

#[test]
fn body_and_math_share_format_shapes_and_rebuilt_forward_and_reverse_endpoints()
-> Result<(), EditError> {
    for change in [
        InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
        InlineFormatChange::State(InlineEnvironmentKind::Italic, true),
        InlineFormatChange::FontFamily("Noto Serif"),
    ] {
        let body = format_flow_range(&fixture::<InlineMath>(), 1..4, change, context())?;
        let math = format_flow_range(&fixture::<MathNode>(), 1..4, change, context())?;
        assert_eq!(projection(&body), projection(&math));
        for (anchor, focus) in [(1, 4), (4, 1)] {
            for (contents, offset, affinity) in [
                (&body, anchor, Affinity::Downstream),
                (&body, focus, Affinity::Upstream),
            ] {
                let (path, local) = text_position_at_offset(contents, offset, affinity)?;
                assert_eq!(text_flow_offset(contents, &path, local)?, offset);
            }
            let (anchor_path, anchor_local) =
                text_position_at_offset(&math, anchor, Affinity::Downstream)?;
            let (focus_path, focus_local) =
                text_position_at_offset(&math, focus, Affinity::Upstream)?;
            assert_eq!(text_flow_offset(&math, &anchor_path, anchor_local)?, anchor);
            assert_eq!(text_flow_offset(&math, &focus_path, focus_local)?, focus);
        }
    }
    Ok(())
}

#[test]
fn body_and_math_share_inherited_inline_format_state() -> Result<(), EditError> {
    let body = fixture::<InlineMath>();
    let math = fixture::<MathNode>();
    for kind in [InlineEnvironmentKind::Bold, InlineEnvironmentKind::Italic] {
        let body_state =
            inline_format_range_state(&body, 1..4, kind, &InheritedInlineStyle::default())?;
        let math_state =
            inline_format_range_state(&math, 1..4, kind, &InheritedInlineStyle::default())?;
        assert_eq!(body_state, (true, true));
        assert_eq!(math_state, body_state);
    }
    Ok(())
}

#[test]
fn body_and_math_share_run_property_range_splitting_and_state() -> Result<(), EditError> {
    let property = crate::editing::environment::inline_adapter::run_property_for_key("underline")
        .copied()
        .ok_or(EditError::UnsupportedInlineFormat)?;
    let body = format_flow_run_property(&fixture::<InlineMath>(), 1..4, property, Some(true))?;
    let math = format_flow_run_property(&fixture::<MathNode>(), 1..4, property, Some(true))?;

    assert_eq!(projection(&body), projection(&math));
    assert_eq!(
        run_property_range_state(&body, 1..4, property)?,
        (true, false)
    );
    assert_eq!(
        run_property_range_state(&body, 0..1, property)?,
        (true, true)
    );
    Ok(())
}

#[test]
fn run_property_range_keeps_unselected_adjacent_environment_ids() -> Result<(), EditError> {
    let property = crate::editing::environment::inline_adapter::run_property_for_key("underline")
        .copied()
        .ok_or(EditError::UnsupportedInlineFormat)?;
    let first = environment(InlineEnvironmentKind::Bold, vec![text::<InlineMath>("a")]);
    let second = environment(InlineEnvironmentKind::Bold, vec![text::<InlineMath>("bc")]);
    let ids = [
        match &first {
            FlowContent::Environment(value) => value.id.clone(),
            _ => return Err(EditError::InvalidPosition),
        },
        match &second {
            FlowContent::Environment(value) => value.id.clone(),
            _ => return Err(EditError::InvalidPosition),
        },
    ];
    let contents = vec![first, second];

    let updated = format_flow_run_property(&contents, 1..2, property, Some(true))?;
    assert_eq!(updated.len(), 2);
    for (content, expected_id) in updated.iter().zip(ids) {
        let FlowContent::Environment(environment) = content else {
            return Err(EditError::InvalidPosition);
        };
        assert_eq!(environment.id, expected_id);
    }
    let FlowContent::Environment(selected) = &updated[1] else {
        return Err(EditError::InvalidPosition);
    };
    let FlowContent::Text(text) = &selected.children[0] else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(text.runs[0].text, "b");
    assert_eq!(text.runs[0].props.get("underline"), Some(&json!(true)));
    assert_eq!(text.runs[1].text, "c");
    assert!(!text.runs[1].props.contains_key("underline"));
    Ok(())
}

#[test]
fn run_property_range_coalesces_compatible_runs_at_selected_seams() -> Result<(), EditError> {
    let property = crate::editing::environment::inline_adapter::run_property_for_key("underline")
        .copied()
        .ok_or(EditError::UnsupportedInlineFormat)?;
    let mut contents = vec![text::<InlineMath>("abc")];
    for range in [0..1, 1..2, 2..3] {
        contents = format_flow_run_property(&contents, range, property, Some(true))?;
    }

    let [FlowContent::Text(text)] = contents.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(text.runs.len(), 1);
    assert_eq!(text.runs[0].text, "abc");
    assert_eq!(text.runs[0].props.get("underline"), Some(&json!(true)));
    Ok(())
}

#[test]
fn large_selected_run_range_coalesces_in_source_order() -> Result<(), EditError> {
    let property = crate::editing::environment::inline_adapter::run_property_for_key("underline")
        .copied()
        .ok_or(EditError::UnsupportedInlineFormat)?;
    let contents = vec![FlowContent::<MathNode>::Text(TextFlow {
        runs: (0..20_000)
            .map(|_| Run {
                text: "x".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            })
            .collect(),
        extra: Default::default(),
    })];
    let updated = format_flow_run_property(&contents, 0..20_000, property, Some(true))?;
    let [FlowContent::Text(text)] = updated.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(text.runs.len(), 1);
    assert_eq!(text.runs[0].text, "x".repeat(20_000));
    assert_eq!(
        text.runs[0].props.get("underline"),
        Some(&serde_json::Value::Bool(true))
    );
    Ok(())
}

#[test]
fn run_property_range_preserves_run_boundaries_with_text_flow_extensions() -> Result<(), EditError>
{
    let property = crate::editing::environment::inline_adapter::run_property_for_key("underline")
        .copied()
        .ok_or(EditError::UnsupportedInlineFormat)?;
    let flow = TextFlow {
        runs: vec![
            Run {
                text: "a".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            },
            Run {
                text: "b".to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            },
        ],
        extra: [("futureRunBoundaries".to_owned(), json!([1]))]
            .into_iter()
            .collect(),
    };
    let contents = [FlowContent::<InlineMath>::Text(flow)];
    let updated = format_flow_run_property(&contents, 0..2, property, Some(true))?;
    let [FlowContent::Text(updated)] = updated.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(updated.runs.len(), 2);
    assert_eq!(updated.extra.get("futureRunBoundaries"), Some(&json!([1])));
    assert!(
        updated
            .runs
            .iter()
            .all(|run| run.props.get("underline") == Some(&json!(true)))
    );
    Ok(())
}

#[test]
fn run_property_range_preserves_empty_runs_at_selection_edges() -> Result<(), EditError> {
    let property = crate::editing::environment::inline_adapter::run_property_for_key("underline")
        .copied()
        .ok_or(EditError::UnsupportedInlineFormat)?;
    fn fixture<T>() -> Vec<FlowContent<T>> {
        vec![FlowContent::Text(TextFlow {
            runs: vec![
                Run {
                    text: "a".to_owned(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                },
                Run {
                    text: String::new(),
                    language: Some("fr".to_owned()),
                    props: Default::default(),
                    extra: [("futureRunField".to_owned(), json!(true))]
                        .into_iter()
                        .collect(),
                },
                Run {
                    text: "b".to_owned(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                },
            ],
            extra: Default::default(),
        })]
    }

    let body = format_flow_run_property(&fixture::<InlineMath>(), 0..1, property, Some(true))?;
    let math = format_flow_run_property(&fixture::<MathNode>(), 0..1, property, Some(true))?;
    assert_eq!(projection(&body), projection(&math));
    let FlowContent::Text(text) = &body[0] else {
        return Err(EditError::InvalidPosition);
    };
    assert!(text.runs[0].props.contains_key("underline"));
    assert_eq!(text.runs[1].language.as_deref(), Some("fr"));
    assert!(!text.runs[1].props.contains_key("underline"));
    assert_eq!(text.runs[1].extra.get("futureRunField"), Some(&json!(true)));
    Ok(())
}

#[test]
fn run_property_toggle_ignores_empty_runs_inside_the_selection() -> Result<(), EditError> {
    let property = crate::editing::environment::inline_adapter::run_property_for_key("underline")
        .copied()
        .ok_or(EditError::UnsupportedInlineFormat)?;
    fn format_all<T: Clone>(
        property: RunPropertyEnvironment,
    ) -> Result<Vec<FlowContent<T>>, EditError> {
        let mut contents = vec![FlowContent::Text(TextFlow {
            runs: vec![
                Run {
                    text: "a".to_owned(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                },
                Run {
                    text: String::new(),
                    language: Some("fr".to_owned()),
                    props: Default::default(),
                    extra: [("futureRunField".to_owned(), json!(true))]
                        .into_iter()
                        .collect(),
                },
                Run {
                    text: "b".to_owned(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                },
            ],
            extra: Default::default(),
        })];
        contents = format_flow_run_property(&contents, 0..1, property, Some(true))?;
        format_flow_run_property(&contents, 1..2, property, Some(true))
    }
    let body = format_all::<InlineMath>(property)?;
    let math = format_all::<MathNode>(property)?;

    assert_eq!(projection(&body), projection(&math));
    assert_eq!(
        run_property_range_state(&body, 0..2, property)?,
        (true, false)
    );
    let FlowContent::Text(text) = &body[0] else {
        return Err(EditError::InvalidPosition);
    };
    assert!(!text.runs[1].props.contains_key("underline"));
    assert_eq!(text.runs[1].extra.get("futureRunField"), Some(&json!(true)));
    Ok(())
}

#[test]
fn inherited_partial_no_op_preserves_exact_tree_and_ids_at_the_depth_limit() -> Result<(), EditError>
{
    let contents = vec![text::<()>("abc")];
    let result = format_flow_range(
        &contents,
        1..2,
        InlineFormatChange::FontFamily("Same"),
        InlineFormatContext {
            inherited: InheritedInlineStyle {
                font_family: Some("Same".to_owned()),
                ..Default::default()
            },
            content_depth: 64,
            max_depth: 64,
        },
    )?;
    assert_eq!(result, contents);
    let bold = vec![environment::<()>(
        InlineEnvironmentKind::Bold,
        vec![text("abc")],
    )];
    assert_eq!(
        format_flow_range(
            &bold,
            1..2,
            InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
            context()
        )?,
        bold
    );
    Ok(())
}

#[test]
fn whole_unknown_metadata_is_preserved_and_lossy_splits_are_rejected() -> Result<(), EditError> {
    let mut contents = vec![text::<()>("abc")];
    let FlowContent::Text(flow) = &mut contents[0] else {
        return Err(EditError::InvalidPosition);
    };
    flow.extra.insert("futureFlow".to_owned(), json!({"v": 1}));
    flow.runs[0].extra.insert("futureRun".to_owned(), json!(2));
    let original = contents.clone();
    let wrapped = format_flow_range(
        &contents,
        0..3,
        InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
        context(),
    )?;
    let [FlowContent::Environment(bold)] = wrapped.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(bold.children, original);
    assert!(matches!(
        format_flow_range(
            &contents,
            1..2,
            InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
            context()
        ),
        Err(EditError::UnsupportedOpaqueContent)
    ));
    assert_eq!(contents, original);
    let mut environment = environment::<()>(InlineEnvironmentKind::Italic, vec![text("abc")]);
    let FlowContent::Environment(value) = &mut environment else {
        return Err(EditError::InvalidPosition);
    };
    value.extra.insert("futureEnvironment".to_owned(), json!(3));
    assert!(matches!(
        format_flow_range(
            &[environment],
            1..2,
            InlineFormatChange::FontFamily("Noto"),
            context()
        ),
        Err(EditError::UnsupportedOpaqueContent)
    ));
    Ok(())
}

#[test]
fn leading_empty_runs_do_not_split_whole_or_unselected_flows_with_extra() -> Result<(), EditError> {
    fn verify<T: Clone + PartialEq + std::fmt::Debug>(
        change: InlineFormatChange<'_>,
    ) -> Result<Vec<Value>, EditError> {
        let mut protected = text::<T>("abc");
        let FlowContent::Text(flow) = &mut protected else {
            return Err(EditError::InvalidPosition);
        };
        flow.extra
            .insert("futureFlow".to_owned(), json!({"keep": true}));
        flow.runs.insert(
            0,
            Run {
                text: String::new(),
                language: Some("en".to_owned()),
                props: Default::default(),
                extra: Default::default(),
            },
        );
        let mut projections = Vec::new();
        for (contents, range) in [
            (vec![protected.clone()], 0..3),
            (vec![text("L"), protected.clone()], 0..1),
            (vec![text("L"), protected.clone(), text("R")], 1..4),
        ] {
            let original = contents.clone();
            let result = format_flow_range(&contents, range.clone(), change, context())?;
            if range == (0..1) {
                assert_eq!(result.last(), Some(&protected));
            } else {
                let selected = result
                    .iter()
                    .find_map(|content| match content {
                        FlowContent::Environment(environment) => Some(environment),
                        _ => None,
                    })
                    .ok_or(EditError::InvalidPosition)?;
                assert_eq!(selected.children, vec![protected.clone()]);
            }
            assert_eq!(contents, original);
            projections.push(projection(&result));
        }
        assert!(matches!(
            format_flow_range(&[protected], 1..2, change, context()),
            Err(EditError::UnsupportedOpaqueContent)
        ));
        Ok(projections)
    }
    for change in [
        InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
        InlineFormatChange::State(InlineEnvironmentKind::Italic, true),
        InlineFormatChange::FontFamily("New"),
    ] {
        assert_eq!(verify::<InlineMath>(change)?, verify::<MathNode>(change)?);
    }
    Ok(())
}

#[test]
fn same_kind_shell_keeps_atomic_descendant_styles_for_body_and_math() -> Result<(), EditError> {
    fn fixture<T>(atom: T, change: InlineFormatChange<'_>, nested: bool) -> FlowContent<T> {
        let atom = FlowContent::Math(atom);
        let atom = if nested {
            environment(
                if change.kind() == InlineEnvironmentKind::Bold {
                    InlineEnvironmentKind::Italic
                } else {
                    InlineEnvironmentKind::Bold
                },
                vec![atom],
            )
        } else {
            atom
        };
        let mut shell = environment(change.kind(), vec![text("a"), atom]);
        let FlowContent::Environment(environment) = &mut shell else {
            unreachable!();
        };
        match change {
            InlineFormatChange::State(_, _) => {
                environment
                    .props
                    .insert("enabled".to_owned(), Value::Bool(false));
            }
            InlineFormatChange::FontFamily(_) => {
                environment
                    .props
                    .insert("fontFamily".to_owned(), Value::String("Old".to_owned()));
            }
        }
        shell
    }
    fn verify<T: Clone + PartialEq + std::fmt::Debug>(
        shell: FlowContent<T>,
        change: InlineFormatChange<'_>,
    ) -> Result<Value, EditError> {
        fn states<T>(
            contents: &[FlowContent<T>],
            inherited: &InheritedInlineStyle,
            change: InlineFormatChange<'_>,
            output: &mut Vec<(String, bool)>,
        ) {
            for content in contents {
                match content {
                    FlowContent::Text(text) => output.push((
                        text.runs.iter().map(|run| run.text.as_str()).collect(),
                        change.matches(inherited),
                    )),
                    FlowContent::Math(_) => {
                        output.push(("atom".to_owned(), change.matches(inherited)))
                    }
                    FlowContent::Environment(environment) => states(
                        &environment.children,
                        &inherited.with_environment(environment),
                        change,
                        output,
                    ),
                    _ => {}
                }
            }
        }
        fn atom<T>(contents: &[FlowContent<T>]) -> Option<&T> {
            contents.iter().find_map(|content| match content {
                FlowContent::Math(atom) => Some(atom),
                FlowContent::Environment(environment) => atom(&environment.children),
                _ => None,
            })
        }
        let FlowContent::Environment(original) = &shell else {
            return Err(EditError::InvalidPosition);
        };
        let result = format_flow_range(std::slice::from_ref(&shell), 0..4, change, context())?;
        let [FlowContent::Environment(updated)] = result.as_slice() else {
            return Err(EditError::InvalidPosition);
        };
        assert_eq!(updated.id, original.id);
        assert_eq!(updated.props, original.props);
        assert_eq!(atom(&result), atom(std::slice::from_ref(&shell)));
        let mut actual = Vec::new();
        states(
            &result,
            &InheritedInlineStyle::default(),
            change,
            &mut actual,
        );
        assert_eq!(
            actual,
            vec![("a".to_owned(), true), ("atom".to_owned(), false)]
        );
        Ok(projection(&result))
    }
    let caret_math::ParsedMath::Tree(tree) =
        caret_math::parse("x", caret_math::MathInputKind::Inline)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?
    else {
        return Err(EditError::InvalidPosition);
    };
    let Some(FlowContent::Math(node)) = tree.root.content.first().cloned() else {
        return Err(EditError::InvalidPosition);
    };
    let inline = InlineMath {
        id: NodeId::generate(),
        math_tree: tree,
        extra: Default::default(),
    };
    for change in [
        InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
        InlineFormatChange::State(InlineEnvironmentKind::Italic, true),
        InlineFormatChange::FontFamily("New"),
    ] {
        for nested in [false, true] {
            assert_eq!(
                verify(fixture(inline.clone(), change, nested), change)?,
                verify(fixture(node.clone(), change, nested), change)?
            );
        }
    }
    Ok(())
}

#[test]
fn empty_run_metadata_and_unselected_environment_ids_survive_range_formatting()
-> Result<(), EditError> {
    let mut middle = text::<()>("abc");
    let FlowContent::Text(flow) = &mut middle else {
        return Err(EditError::InvalidPosition);
    };
    flow.runs.insert(
        0,
        Run {
            text: String::new(),
            language: Some("en".to_owned()),
            props: Default::default(),
            extra: Default::default(),
        },
    );
    flow.runs.push(Run {
        text: String::new(),
        language: None,
        props: Default::default(),
        extra: [("future".to_owned(), json!(1))].into(),
    });
    let left = environment(InlineEnvironmentKind::Italic, vec![text("L")]);
    let right = environment(InlineEnvironmentKind::Italic, vec![text("R")]);
    let contents = vec![left.clone(), middle, right.clone()];
    let result = format_flow_range(
        &contents,
        2..3,
        InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
        context(),
    )?;
    assert_eq!(result.first(), Some(&left));
    assert_eq!(result.last(), Some(&right));
    fn collect<T>(contents: &[FlowContent<T>], runs: &mut Vec<Run>) {
        for content in contents {
            match content {
                FlowContent::Text(text) => runs.extend(text.runs.clone()),
                FlowContent::Environment(env) => collect(&env.children, runs),
                _ => {}
            }
        }
    }
    let mut runs = Vec::new();
    collect(&result, &mut runs);
    assert!(
        runs.iter()
            .any(|run| run.text.is_empty() && run.language.as_deref() == Some("en"))
    );
    assert!(
        runs.iter()
            .any(|run| run.text.is_empty() && run.extra.get("future") == Some(&json!(1)))
    );
    Ok(())
}

#[test]
fn adding_depth_preserves_existing_ancestors_and_rejects_overflow_atomically()
-> Result<(), EditError> {
    let contents = vec![text::<()>("a")];
    assert!(matches!(
        format_flow_range(
            &contents,
            0..1,
            InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
            InlineFormatContext {
                content_depth: 64,
                ..context()
            }
        ),
        Err(EditError::NestingLimit)
    ));
    assert_eq!(contents, vec![text::<()>("a")]);
    let mut original = environment::<()>(InlineEnvironmentKind::Bold, vec![text("a")]);
    let FlowContent::Environment(env) = &mut original else {
        return Err(EditError::InvalidPosition);
    };
    let id = env.id.clone();
    env.props.insert("enabled".to_owned(), Value::Bool(false));
    let result = format_flow_range(
        std::slice::from_ref(&original),
        0..1,
        InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
        InlineFormatContext {
            content_depth: 62,
            ..context()
        },
    )?;
    let [FlowContent::Environment(env)] = result.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(env.id, id);
    assert_eq!(env.props.get("enabled"), Some(&Value::Bool(false)));
    let [FlowContent::Environment(inner)] = env.children.as_slice() else {
        return Err(EditError::InvalidPosition);
    };
    assert_eq!(inner.props.get("enabled"), Some(&Value::Bool(true)));
    assert!(matches!(
        format_flow_range(
            std::slice::from_ref(&original),
            0..1,
            InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
            InlineFormatContext {
                content_depth: 63,
                ..context()
            }
        ),
        Err(EditError::NestingLimit)
    ));
    Ok(())
}

#[test]
fn atoms_break_wrapping_and_opaque_nodes_are_preserved_outside_selection() -> Result<(), EditError>
{
    let contents = vec![
        text("a"),
        FlowContent::Math(7u32),
        FlowContent::SoftBreak(SoftBreak::default()),
        text("b"),
    ];
    let result = format_flow_range(
        &contents,
        0..6,
        InlineFormatChange::FontFamily("Noto"),
        context(),
    )?;
    assert!(
        result
            .iter()
            .any(|content| matches!(content, FlowContent::Math(7)))
    );
    assert!(matches!(
        format_flow_range(
            &contents,
            1..2,
            InlineFormatChange::FontFamily("Noto"),
            context()
        ),
        Err(EditError::InvalidPosition)
    ));
    let opaque = FlowContent::Opaque(OpaqueNode {
        raw: json!({"type": "future"}),
    });
    let contents = vec![opaque.clone(), text::<()>("abc")];
    let result = format_flow_range(
        &contents,
        3..6,
        InlineFormatChange::FontFamily("Noto"),
        context(),
    )?;
    assert_eq!(result.first(), Some(&opaque));
    assert!(matches!(
        format_flow_range(
            &contents,
            0..6,
            InlineFormatChange::FontFamily("Noto"),
            context()
        ),
        Err(EditError::UnsupportedOpaqueContent)
    ));
    Ok(())
}

#[test]
fn protected_flow_run_boundaries_survive_whole_wrapping_and_selection_seams()
-> Result<(), EditError> {
    fn verify<T: Clone + PartialEq + std::fmt::Debug>() -> Result<Vec<Value>, EditError> {
        fn find<T>(contents: &[FlowContent<T>]) -> Option<&TextFlow> {
            contents.iter().find_map(|content| match content {
                FlowContent::Text(text) if text.extra.contains_key("futureFlow") => Some(text),
                FlowContent::Environment(environment) => find(&environment.children),
                _ => None,
            })
        }
        fn bold<T>(content: FlowContent<T>) -> FlowContent<T> {
            let mut node = environment(InlineEnvironmentKind::Bold, vec![content]);
            if let FlowContent::Environment(environment) = &mut node {
                environment
                    .props
                    .insert("enabled".to_owned(), Value::Bool(true));
            }
            node
        }
        let mut protected = text::<T>("a");
        let FlowContent::Text(flow) = &mut protected else {
            return Err(EditError::InvalidPosition);
        };
        let mut second = flow.runs[0].clone();
        second.text = "bc".to_owned();
        flow.runs.push(second);
        flow.extra
            .insert("futureFlow".to_owned(), json!({"runBoundaries": [1, 3]}));
        let expected = flow.clone();
        let mut results = Vec::new();
        for (contents, range) in [
            (vec![protected.clone()], 0..3),
            (
                vec![bold(text("L")), protected.clone(), bold(text("R"))],
                1..4,
            ),
            (vec![bold(protected), text("d")], 3..4),
        ] {
            let result = format_flow_range(
                &contents,
                range,
                InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
                context(),
            )?;
            assert_eq!(find(&result), Some(&expected));
            assert_eq!(find(&result).map(|text| text.runs.len()), Some(2));
            results.push(projection(&result));
        }
        Ok(results)
    }
    assert_eq!(verify::<InlineMath>()?, verify::<MathNode>()?);
    Ok(())
}

#[test]
fn zero_length_environment_boundaries_are_unchanged_for_body_and_math() -> Result<(), EditError> {
    fn verify<T: Clone + PartialEq + std::fmt::Debug>(
        change: InlineFormatChange<'_>,
    ) -> Result<Value, EditError> {
        let empty = |label: &str| {
            let mut node = environment::<T>(change.kind(), Vec::new());
            if let FlowContent::Environment(environment) = &mut node {
                match change {
                    InlineFormatChange::State(_, _) => {
                        environment
                            .props
                            .insert("enabled".to_owned(), Value::Bool(false));
                    }
                    InlineFormatChange::FontFamily(_) => {
                        environment
                            .props
                            .insert("fontFamily".to_owned(), Value::String("Old".to_owned()));
                    }
                }
                environment
                    .extra
                    .insert("futureHint".to_owned(), json!(label));
            }
            node
        };
        let start = empty("start");
        let end = empty("end");
        let original = vec![start.clone(), text("a"), end.clone()];
        let result = format_flow_range(&original, 0..1, change, context())?;
        assert_eq!(result.first(), Some(&start));
        assert_eq!(result.last(), Some(&end));
        assert_eq!(original.first(), Some(&start));
        assert_eq!(original.last(), Some(&end));
        Ok(projection(&result))
    }
    for change in [
        InlineFormatChange::State(InlineEnvironmentKind::Bold, true),
        InlineFormatChange::State(InlineEnvironmentKind::Italic, true),
        InlineFormatChange::FontFamily("New"),
    ] {
        assert_eq!(verify::<InlineMath>(change)?, verify::<MathNode>(change)?);
    }
    Ok(())
}

#[test]
fn explicit_false_is_not_confused_with_unset_or_enabled_inheritance() -> Result<(), EditError> {
    fn verify<T: Clone + PartialEq + std::fmt::Debug>(
        kind: InlineEnvironmentKind,
    ) -> Result<Vec<Value>, EditError> {
        let original = vec![text::<T>("a")];
        let mut results = Vec::new();
        for inherited in [None, Some(true), Some(false)] {
            let mut style = InheritedInlineStyle::default();
            match kind {
                InlineEnvironmentKind::Bold => style.bold = inherited,
                InlineEnvironmentKind::Italic => style.italic = inherited,
                InlineEnvironmentKind::Font => return Err(EditError::UnsupportedInlineFormat),
            }
            let result = format_flow_range(
                &original,
                0..1,
                InlineFormatChange::State(kind, false),
                InlineFormatContext {
                    inherited: style,
                    ..context()
                },
            )?;
            if inherited == Some(false) {
                assert_eq!(result, original);
            } else {
                let [FlowContent::Environment(off)] = result.as_slice() else {
                    return Err(EditError::InvalidPosition);
                };
                assert_eq!(off.kind, kind);
                assert_eq!(off.props.get("enabled"), Some(&Value::Bool(false)));
                assert_eq!(off.children, original);
            }
            results.push(projection(&result));
        }
        Ok(results)
    }
    for kind in [InlineEnvironmentKind::Bold, InlineEnvironmentKind::Italic] {
        assert_eq!(verify::<InlineMath>(kind)?, verify::<MathNode>(kind)?);
    }
    Ok(())
}

#[test]
fn inherited_style_uses_shared_environment_delta_and_preserves_outer_font() {
    let mut outer = InlineEnvironment {
        id: NodeId::generate(),
        kind: InlineEnvironmentKind::Font,
        definition_id: None,
        props: Default::default(),
        children: Vec::<FlowContent<InlineMath>>::new(),
        extra: Default::default(),
    };
    outer.props.insert(
        "fontFamily".to_owned(),
        Value::String("Outer Family".to_owned()),
    );
    let inherited = InheritedInlineStyle::default().with_environment(&outer);
    assert_eq!(inherited.font_family.as_deref(), Some("Outer Family"));

    outer
        .props
        .insert("fontFamily".to_owned(), Value::String("  ".to_owned()));
    let inherited = inherited.with_environment(&outer);
    assert_eq!(inherited.font_family.as_deref(), Some("Outer Family"));

    outer.kind = InlineEnvironmentKind::Bold;
    outer.props.clear();
    assert!(
        InheritedInlineStyle::default()
            .with_environment(&outer)
            .bold
            .unwrap_or(false)
    );
}
