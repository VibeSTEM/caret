//! 验证数学树语义对齐与独立源码 sidecar 的边界。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use crate::{MathContentSourceMap, MathInputKind, ProjectedMath, parse_projection};
use caret_doc::{MathContent, MathFlow, MathNode, MathNodeKind, MathTree, NodeId};
use serde_json::json;
use std::collections::BTreeMap;

fn parse_tree(source: &str) -> (MathTree, super::super::source_map::MathTreeSourceMap) {
    let (ProjectedMath::Tree(tree), projection) = parse_projection(source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("inline formula should parse: {error}"))
        .into_parts()
    else {
        panic!("expected inline tree")
    };
    let map = projection
        .tree()
        .cloned()
        .unwrap_or_else(|| panic!("inline tree should have a source map"));
    (tree, map)
}

fn parse_align(source: &str) -> (caret_doc::Align, super::super::AlignSourceMap) {
    let (ProjectedMath::Align(align), projection) = parse_projection(source, MathInputKind::Align)
        .unwrap_or_else(|error| panic!("Align should parse: {error}"))
        .into_parts()
    else {
        panic!("expected Align")
    };
    let map = projection
        .align()
        .cloned()
        .unwrap_or_else(|| panic!("Align should have a source map"));
    (align, map)
}

fn split_first_text_flow(tree: &mut MathTree, offset: usize) {
    let Some(MathContent::Text(text)) = tree.root.content.first() else {
        panic!("expected a root TextFlow")
    };
    assert_eq!(text.runs.len(), 1);
    let run = &text.runs[0];
    assert!(run.text.is_char_boundary(offset));
    let mut left = text.clone();
    let mut right = text.clone();
    left.runs[0].text = run.text[..offset].to_owned();
    right.runs[0].text = run.text[offset..].to_owned();
    tree.root.content = vec![MathContent::Text(left), MathContent::Text(right)];
}

fn wrap_root_contents_in_row(tree: &mut MathTree, flow_count: usize) {
    assert!(flow_count > 0);
    let contents = std::mem::take(&mut tree.root.content);
    let mut flows = Vec::with_capacity(flow_count);
    let mut offset = 0;
    for index in 0..flow_count {
        let remaining_flows = flow_count - index;
        let remaining_contents = contents.len().saturating_sub(offset);
        let count = remaining_contents.div_ceil(remaining_flows);
        flows.push(MathFlow {
            id: NodeId::generate(),
            content: contents[offset..offset + count].to_vec(),
            extra: BTreeMap::new(),
        });
        offset += count;
    }
    tree.root.content = vec![MathContent::Math(MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Row,
        text: None,
        attributes: BTreeMap::new(),
        children: flows,
        extra: BTreeMap::new(),
    })];
}

#[test]
fn tree_reconciliation_preserves_persistent_metadata_without_ranges() {
    let (mut target, _) = parse_tree("x+y");
    let (parsed, _) = parse_tree("x+y");
    target.extra.insert("futureTreeField".into(), json!(1));
    let node_id = match target.root.content.first() {
        Some(MathContent::Math(node)) => node.id.clone(),
        _ => panic!("expected math node"),
    };

    let reconciled = reconcile_tree_ranges(&target, &parsed)
        .unwrap_or_else(|| panic!("matching semantics should reconcile"));
    assert_eq!(reconciled, target);
    assert_eq!(
        match reconciled.root.content.first() {
            Some(MathContent::Math(node)) => &node.id,
            _ => panic!("expected math node"),
        },
        &node_id
    );
}

#[test]
fn tree_reconciliation_rejects_different_semantics() {
    let (target, _) = parse_tree("x+y");
    let (parsed, _) = parse_tree("x-y");
    assert!(reconcile_tree_ranges(&target, &parsed).is_none());
}

#[test]
fn align_reconciliation_preserves_persistent_metadata_only_for_equal_semantics() {
    let (mut target, _) = parse_align(r"\begin{aligned}x&y\\a&b\end{aligned}");
    let (parsed, _) = parse_align(r"\begin{aligned}x&y\\a&b\end{aligned}");
    target.rows[0]
        .extra
        .insert("futureRowField".into(), json!(true));
    let reconciled = reconcile_align_ranges(&target, &parsed)
        .unwrap_or_else(|| panic!("matching semantics should reconcile"));
    assert_eq!(reconciled, target);

    let (different, _) = parse_align(r"\begin{aligned}x&y\\a&c\end{aligned}");
    assert!(reconcile_align_ranges(&target, &different).is_none());
}

#[test]
fn sidecar_reconciliation_maps_equivalent_text_flow_splits() {
    let (target, _) = parse_tree(r"\text{éa}\text{b}");
    let source = r"\text{é}\text{ab}";
    let (parsed, projection_map) = parse_tree(source);
    let reconciled = projection_map
        .reconcile_for_tree(&parsed, &target, None, source, None)
        .unwrap_or_else(|| panic!("equivalent text flows should reconcile"));
    assert!(
        reconciled
            .validates_projection(&target, source, None)
            .is_some()
    );
    assert_eq!(reconciled.root().content().len(), target.root.content.len());
}

#[test]
fn sidecar_reconciliation_preserves_literal_symbol_ranges() {
    let source = r"a\%b";
    let (parsed, parsed_map) = parse_tree(source);
    let mut target = parsed.clone();
    for content in &mut target.root.content {
        if let MathContent::Math(node) = content {
            node.id = NodeId::generate();
        }
    }

    let reconciled = parsed_map
        .reconcile_for_tree(&parsed, &target, None, source, None)
        .unwrap_or_else(|| panic!("literal character boundaries should reconcile"));
    assert!(
        reconciled
            .validates_projection(&target, source, None)
            .is_some()
    );
    assert_eq!(
        reconciled
            .root()
            .content()
            .iter()
            .filter_map(MathContentSourceMap::range)
            .collect::<Vec<_>>(),
        [0..1, 1..3, 3..4]
    );
}

#[test]
fn sidecar_reconciliation_maps_transparent_root_row_text_and_mixed_contents() {
    for source in [r"\text{ab}", r"x+\text{T}"] {
        let (mut target, _) = parse_tree(source);
        let (parsed, parsed_map) = parse_tree(source);
        wrap_root_contents_in_row(&mut target, 1);
        if let Some(MathContent::Math(row)) = target.root.content.first_mut()
            && let Some(MathFlow { content, .. }) = row.children.first_mut()
            && let Some(MathContent::Text(text)) = content
                .iter_mut()
                .find(|item| matches!(item, MathContent::Text(_)))
            && let Some(run) = text.runs.first_mut()
        {
            run.props.insert("underline".into(), json!(true));
            run.extra.insert("vendorRun".into(), json!("kept"));
        }

        let reconciled = parsed_map
            .reconcile_for_tree(&parsed, &target, None, source, None)
            .unwrap_or_else(|| panic!("transparent root Row should map to flattened source"));
        assert!(
            reconciled
                .validates_projection(&target, source, None)
                .is_some()
        );
        let Some(MathContentSourceMap::Math { operands, .. }) = reconciled.root().content().first()
        else {
            panic!("transparent Row should retain its structural sidecar branch");
        };
        assert_eq!(operands.len(), 1);
        if source == r"\text{ab}" {
            assert!(matches!(
                operands[0].content().first(),
                Some(MathContentSourceMap::Text(_))
            ));
        } else {
            assert_eq!(operands[0].content().len(), parsed.root.content.len());
        }
    }
}

#[test]
fn sidecar_reconciliation_maps_transparent_row_without_entering_fraction_slots() {
    let source = r"\frac{\text{x}}{1}";
    let (mut target, _) = parse_tree(source);
    let (parsed, parsed_map) = parse_tree(source);
    wrap_root_contents_in_row(&mut target, 1);

    let reconciled = parsed_map
        .reconcile_for_tree(&parsed, &target, None, source, None)
        .unwrap_or_else(|| panic!("root Row should reconcile around a fixed fraction node"));
    assert!(
        reconciled
            .validates_projection(&target, source, None)
            .is_some()
    );
    let Some(MathContentSourceMap::Math { operands, .. }) = reconciled.root().content().first()
    else {
        panic!("expected root Row sidecar");
    };
    assert!(matches!(
        operands[0].content().first(),
        Some(MathContentSourceMap::Math { .. })
    ));
}

#[test]
fn sidecar_reconciliation_preserves_control_word_separators_inside_transparent_row() {
    let source = r"\alpha x\text{T}";
    let (mut target, _) = parse_tree(source);
    let (parsed, parsed_map) = parse_tree(source);
    wrap_root_contents_in_row(&mut target, 2);

    let reconciled = parsed_map
        .reconcile_for_tree(&parsed, &target, None, source, None)
        .unwrap_or_else(|| panic!("transparent Row should preserve TeX control word boundaries"));
    assert!(
        reconciled
            .validates_projection(&target, source, None)
            .is_some()
    );
}

#[test]
fn sidecar_reconciliation_preserves_color_scope_across_transparent_row_slots() {
    let (mut target, _) = parse_tree(r"\textcolor{red}{x}y\text{T}");
    let Some(MathContent::Math(color)) = target.root.content.first_mut() else {
        panic!("expected a color node");
    };
    assert_eq!(color.kind, MathNodeKind::TextColor);
    color.kind = MathNodeKind::Color;
    wrap_root_contents_in_row(&mut target, 3);

    let source = crate::serialize_tree(&target);
    assert!(source.starts_with(r"\textcolor{red}{"));
    let (parsed, parsed_map) = parse_tree(&source);
    let reconciled = parsed_map
        .reconcile_for_tree(&parsed, &target, None, &source, None)
        .unwrap_or_else(|| panic!("scoped color should reconcile through transparent Row slots"));
    assert!(
        reconciled
            .validates_projection(&target, &source, None)
            .is_some()
    );
}

#[test]
fn sidecar_reconciliation_handles_adjacent_text_flows_across_row_slots() {
    let source = r"\text{a}\text{b}";
    let (mut target, _) = parse_tree(source);
    let (parsed, parsed_map) = parse_tree(source);
    wrap_root_contents_in_row(&mut target, 2);

    let reconciled = parsed_map
        .reconcile_for_tree(&parsed, &target, None, source, None)
        .unwrap_or_else(|| panic!("adjacent text flows should map across transparent Row slots"));
    assert!(
        reconciled
            .validates_projection(&target, source, None)
            .is_some()
    );
}

#[test]
fn sidecar_reconciliation_rejects_stale_ranges_and_ambiguous_transparent_row_mapping() {
    let source = r"x+\text{T}";
    let (mut target, _) = parse_tree(source);
    let (parsed, mut parsed_map) = parse_tree(source);
    wrap_root_contents_in_row(&mut target, 2);
    let Some(MathContentSourceMap::Math { range, .. }) = parsed_map.root.content.first_mut() else {
        panic!("expected first math source range");
    };
    range.range = Some(source.len()..source.len() + 1);
    assert!(
        parsed_map
            .reconcile_for_tree(&parsed, &target, None, source, None)
            .is_none()
    );

    let ambiguous_source = r"\text{T}";
    let (ambiguous_parsed, ambiguous_map) = parse_tree(ambiguous_source);
    let (mut ambiguous_target, _) = parse_tree(ambiguous_source);
    wrap_root_contents_in_row(&mut ambiguous_target, 3);
    assert!(
        ambiguous_map
            .reconcile_for_tree(
                &ambiguous_parsed,
                &ambiguous_target,
                None,
                ambiguous_source,
                None
            )
            .is_none()
    );

    let (parsed, parsed_map) = parse_tree(ambiguous_source);
    let Some(MathContent::Math(row)) = ambiguous_target.root.content.first_mut() else {
        panic!("expected Row");
    };
    row.extra.insert("vendorRow".into(), json!("unowned"));
    assert!(
        parsed_map
            .reconcile_for_tree(&parsed, &ambiguous_target, None, ambiguous_source, None)
            .is_none()
    );
}

#[test]
fn sidecar_reconciliation_assigns_comment_gap_at_text_split() {
    let source = "\\text{a% hidden TeX comment\nb}";
    let (parsed, parsed_map) = parse_tree(source);
    let (mut target, _) = parse_tree(r"\text{ab}");
    split_first_text_flow(&mut target, 1);

    let reconciled = parsed_map
        .reconcile_for_tree(&parsed, &target, None, source, None)
        .unwrap_or_else(|| panic!("comment gap should be assigned consistently"));
    assert_eq!(reconciled.root().content().len(), 2);
    assert!(
        reconciled
            .validates_projection(&target, source, None)
            .is_some()
    );

    let ranges = reconciled
        .root()
        .content()
        .iter()
        .map(|content| match content {
            MathContentSourceMap::Text(text) => text.range().and_then(|range| source.get(range)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ranges, [Some("a% hidden TeX comment\n"), Some("b")]);
}

#[test]
fn sidecar_reconciliation_maps_trailing_empty_text_anchor_after_math() {
    let source = r"\text{abc$x$}";
    let (parsed, parsed_map) = parse_tree(source);
    let mut target = parsed.clone();
    target
        .root
        .content
        .push(MathContent::Text(caret_doc::TextFlow {
            runs: Vec::new(),
            extra: BTreeMap::new(),
        }));

    let reconciled = parsed_map
        .reconcile_for_tree(&parsed, &target, None, source, None)
        .unwrap_or_else(|| panic!("editable trailing empty TextFlow should map"));
    let anchor = reconciled
        .root()
        .content()
        .last()
        .and_then(MathContentSourceMap::range)
        .unwrap_or_else(|| panic!("trailing TextFlow anchor"));
    assert_eq!(anchor.start, anchor.end);
    assert!(source.is_char_boundary(anchor.start));
    assert!(
        reconciled
            .validates_projection(&target, source, None)
            .is_some()
    );
}

#[test]
fn align_cell_sidecar_reconciliation_assigns_comment_gap_at_text_split() {
    let source = "\\begin{aligned}\\text{a% hidden TeX comment\nb}&x\\end{aligned}";
    let (parsed, parsed_map) = parse_align(source);
    let (mut target, _) = parse_align(r"\begin{aligned}\text{ab}&x\end{aligned}");
    split_first_text_flow(&mut target.rows[0].cells[0], 1);

    let range = parsed_map
        .cell_range(0, 0)
        .unwrap_or_else(|| panic!("Align cell range"));
    let parsed_cell = parsed.rows[0].cells[0].clone();
    let target_cell = target.rows[0].cells[0].clone();
    let reconciled = parsed_map
        .cell_tree(0, 0)
        .and_then(|map| {
            map.reconcile_for_tree(&parsed_cell, &target_cell, None, source, Some(&range))
        })
        .unwrap_or_else(|| panic!("Align cell comment gap should be assigned consistently"));
    assert_eq!(reconciled.root().content().len(), 2);
    assert!(
        reconciled
            .validates_projection(&target_cell, source, Some(&range))
            .is_some()
    );
}

#[test]
fn sidecar_reconciliation_rejects_semantic_mismatch_and_bad_ranges() {
    let (target, _) = parse_tree(r"\text{x}");
    let source = r"\text{y}";
    let (parsed, projection_map) = parse_tree(source);
    assert!(
        projection_map
            .reconcile_for_tree(&parsed, &target, None, source, None)
            .is_none()
    );

    let source = r"\text{x}";
    let (parsed, mut projection_map) = parse_tree(source);
    let Some(MathContentSourceMap::Text(text)) = projection_map.root.content.first_mut() else {
        panic!("expected text sidecar")
    };
    text.range.range = Some(std::ops::Range { start: 6, end: 5 });
    assert!(
        projection_map
            .reconcile_for_tree(&parsed, &parsed, None, source, None)
            .is_none()
    );
}

#[test]
fn sidecar_reconciliation_refuses_ambiguous_unknown_extensions_atomically() {
    let source = r"\text{ab}\text{c}";
    let (target, mut previous) = parse_tree(source);
    let Some(MathContentSourceMap::Text(text)) = previous.root.content.first_mut() else {
        panic!("expected text sidecar")
    };
    text.range.extra.insert("vendor".into(), json!(1));

    let next_source = r"\text{a}\text{bc}";
    let (parsed, parsed_map) = parse_tree(next_source);
    let previous_before = previous.clone();
    assert!(
        parsed_map
            .reconcile_for_tree(&parsed, &target, Some(&previous), next_source, None)
            .is_none()
    );
    assert_eq!(previous, previous_before);
}

#[test]
fn align_cell_sidecar_uses_complete_source_coordinates_and_scope() {
    let source = "\\begin{aligned}x&% gap\n \\text{ab}\\\\ y&z\\end{aligned}";
    let (align, map) = parse_align(source);
    let range = map
        .cell_range(0, 1)
        .unwrap_or_else(|| panic!("second cell range"));
    let tree_map = map
        .cell_tree(0, 1)
        .unwrap_or_else(|| panic!("second cell sidecar"));
    assert!(range.start > 0);
    assert!(
        tree_map
            .validates_projection(&align.rows[0].cells[1], source, Some(&range))
            .is_some()
    );
}

#[test]
fn source_environment_matching_accepts_only_tex_expressible_font_classes() {
    let stored = BTreeMap::from([("fontFamily".to_owned(), json!("Noto Serif"))]);
    let parsed = BTreeMap::from([("fontFamily".to_owned(), json!("serif"))]);
    assert!(source_environment_props_match(
        caret_doc::InlineEnvironmentKind::Font,
        &stored,
        &parsed
    ));
    let incompatible = BTreeMap::from([("fontFamily".to_owned(), json!("monospace"))]);
    assert!(!source_environment_props_match(
        caret_doc::InlineEnvironmentKind::Font,
        &stored,
        &incompatible
    ));
}
