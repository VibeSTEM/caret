//! 验证 TeX MathBox 到稳定 Caret 几何快照的转换。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{MathLayoutBackend, MathLayoutError, MathLayoutItem};
use crate::{
    MathInputKind, MathSyntaxKind, MathSyntaxNode, MathTextMetrics, MathTextShaper, parse,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};

#[test]
fn layout_errors_keep_stable_diagnostics_and_upstream_error_context() {
    assert_eq!(
        MathLayoutError::InvalidFontSize.to_string(),
        "math.layout.invalid_font_size"
    );
    assert_eq!(
        MathLayoutError::UnsupportedTree("unmapped node".into()).to_string(),
        "math.layout.unsupported_tree: unmapped node"
    );
    assert_eq!(
        MathLayoutError::Backend("provider rejected text".into()).to_string(),
        "math.layout.backend: provider rejected text"
    );
    assert_eq!(
        MathLayoutError::GeometryOverflow.to_string(),
        "math.layout.geometry_overflow"
    );

    let error = MathLayoutError::from(latex_rust::Error::Unsupported {
        what: "test-only construct".into(),
    });
    assert_eq!(
        error,
        MathLayoutError::Backend("unsupported: test-only construct".into())
    );
}

struct FixedTextShaper;

impl MathTextShaper for FixedTextShaper {
    fn shape_text(
        &self,
        text: &str,
        _style: latex_rust::TextStyle,
        font_size_micro_pt: i64,
    ) -> Result<MathTextMetrics, String> {
        Ok(MathTextMetrics {
            handle: u64::try_from(text.chars().count()).map_err(|error| error.to_string())?,
            width_micro_pt: font_size_micro_pt,
            ascent_micro_pt: font_size_micro_pt * 3 / 4,
            descent_micro_pt: font_size_micro_pt / 4,
        })
    }
}

#[test]
fn inline_fraction_and_caller_shaped_text_share_one_layout_snapshot() {
    let parsed = parse(r"\frac{a}{b}+\text{中 文}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("formula should parse: {error}"));
    let mut tree = parsed
        .syntax_tree()
        .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
    tree.root.provenance = Some("math-root".into());

    let backend = MathLayoutBackend::new()
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));
    let snapshot = backend
        .layout(&tree, 1200, &FixedTextShaper)
        .unwrap_or_else(|error| panic!("formula should lay out: {error}"));

    assert_eq!(snapshot.font_resource_sha256, crate::MATH_FONT_SHA256);
    assert!(snapshot.width_micro_pt > 0);
    assert!(snapshot.ascent_micro_pt > 0);
    assert!(
        snapshot
            .items
            .iter()
            .any(|item| matches!(item, MathLayoutItem::Rule { .. }))
    );
    assert!(
        snapshot
            .items
            .iter()
            .any(|item| matches!(item, MathLayoutItem::Glyph { color: None, .. }))
    );
    assert!(
        snapshot
            .items
            .iter()
            .any(|item| matches!(item, MathLayoutItem::ExternalText { handle: 3, .. }))
    );
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.node_id == "math-root")
    );
}

#[test]
fn explicit_formula_color_is_preserved_separately_from_theme_default() {
    let parsed = parse(r"\textcolor{red}{x}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("colored formula should parse: {error}"));
    let tree = parsed
        .syntax_tree()
        .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
    let backend =
        MathLayoutBackend::new().unwrap_or_else(|error| panic!("math font should load: {error}"));
    let snapshot = backend
        .layout(&tree, 1200, &FixedTextShaper)
        .unwrap_or_else(|error| panic!("colored formula should lay out: {error}"));

    assert!(snapshot.items.iter().any(|item| matches!(
        item,
        MathLayoutItem::Glyph {
            color: Some([255, 0, 0]),
            ..
        }
    )));
}

#[test]
fn optional_placeholder_keeps_a_zero_size_provenance_anchor() {
    let parsed = parse(r"\sqrt{x}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("formula should parse: {error}"));
    let mut tree = parsed
        .syntax_tree()
        .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
    tree.root.children[0].provenance = Some("radical-index-slot".into());
    let backend =
        MathLayoutBackend::new().unwrap_or_else(|error| panic!("math font should load: {error}"));
    let snapshot = backend
        .layout(&tree, 1200, &FixedTextShaper)
        .unwrap_or_else(|error| panic!("formula should lay out: {error}"));

    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| { node.node_id == "radical-index-slot" && node.width_micro_pt == 0 })
    );
}

#[test]
fn display_aligned_rows_layout_as_one_provenance_snapshot() {
    let parsed = parse(
        r"\begin{aligned}a &= b \\ c &= d\end{aligned}",
        MathInputKind::Align,
    )
    .unwrap_or_else(|error| panic!("aligned formula should parse: {error}"));
    let mut tree = parsed
        .syntax_tree()
        .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
    tree.root.provenance = Some("aligned-root".into());
    let backend =
        MathLayoutBackend::new().unwrap_or_else(|error| panic!("math font should load: {error}"));
    let snapshot = backend
        .layout(&tree, 1200, &FixedTextShaper)
        .unwrap_or_else(|error| panic!("aligned formula should lay out: {error}"));

    assert!(snapshot.width_micro_pt > 0);
    assert!(snapshot.ascent_micro_pt > 0);
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.node_id == "aligned-root")
    );
}

#[test]
fn node_provenance_does_not_change_math_placement_or_draw_primitives() {
    let backend = MathLayoutBackend::new()
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));
    for source in [
        r"x^2 + \hat{f}",
        r"\widehat{abcdef}+\overrightarrow{xy}",
        r"\phantom{x}+y",
        r"\phantom{\frac{a}{b}}+y",
        r"\phantom{xy}+z",
        r"\vphantom{x}+y",
        r"\hphantom{x}+y",
        r"\begin{array}{c|c}a&b\\c&d\end{array}",
    ] {
        let plain = parse(source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("formula should parse: {error}"))
            .syntax_tree()
            .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
        let mut tagged = parse(source, MathInputKind::Inline)
            .unwrap_or_else(|error| panic!("formula should parse: {error}"))
            .syntax_tree()
            .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
        let mut next_id = 0;
        let mut expected_node_ids = Vec::new();
        add_provenance(
            &mut tagged.root,
            &mut next_id,
            &mut expected_node_ids,
            false,
        );

        let plain = backend
            .layout(&plain, 1200, &FixedTextShaper)
            .unwrap_or_else(|error| panic!("formula should lay out: {error}"));
        let tagged = backend
            .layout(&tagged, 1200, &FixedTextShaper)
            .unwrap_or_else(|error| panic!("formula with provenance should lay out: {error}"));

        assert_eq!(
            (
                plain.width_micro_pt,
                plain.ascent_micro_pt,
                plain.descent_micro_pt,
                plain.items,
            ),
            (
                tagged.width_micro_pt,
                tagged.ascent_micro_pt,
                tagged.descent_micro_pt,
                tagged.items,
            ),
            "provenance changed layout for {source}"
        );
        let actual_node_ids: Vec<_> = tagged
            .nodes
            .iter()
            .map(|node| node.node_id.as_str())
            .collect();
        assert_eq!(
            actual_node_ids,
            expected_node_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            "provenance IDs were lost for {source}"
        );
    }
}

#[test]
fn phantom_keeps_shifted_child_node_geometry_without_drawing_its_primitives() {
    let backend = MathLayoutBackend::new()
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));
    let mut plain = parse(r"\sum_{i=0}^{n}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("sum should parse: {error}"))
        .syntax_tree()
        .unwrap_or_else(|error| panic!("sum syntax should convert: {error}"));
    let mut phantom = parse(r"\phantom{\sum_{i=0}^{n}}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("phantom sum should parse: {error}"))
        .syntax_tree()
        .unwrap_or_else(|error| panic!("phantom sum syntax should convert: {error}"));
    assert!(set_kind_provenance(
        &mut plain.root,
        MathSyntaxKind::Sum,
        "sum-node"
    ));
    assert!(set_kind_provenance(
        &mut phantom.root,
        MathSyntaxKind::Sum,
        "sum-node"
    ));

    let plain = backend
        .layout(&plain, 1200, &FixedTextShaper)
        .unwrap_or_else(|error| panic!("sum should lay out: {error}"));
    let phantom = backend
        .layout(&phantom, 1200, &FixedTextShaper)
        .unwrap_or_else(|error| panic!("phantom sum should lay out: {error}"));
    let plain_sum = plain
        .nodes
        .iter()
        .find(|node| node.node_id == "sum-node")
        .unwrap_or_else(|| panic!("plain sum geometry should be present"));
    let phantom_sum = phantom
        .nodes
        .iter()
        .find(|node| node.node_id == "sum-node")
        .unwrap_or_else(|| panic!("phantom sum geometry should be present"));

    assert_eq!(phantom_sum, plain_sum);
    assert!(phantom.items.is_empty());
}

fn add_provenance(
    node: &mut MathSyntaxNode,
    next_id: &mut usize,
    ids: &mut Vec<String>,
    is_matrix_row: bool,
) {
    let id = format!("node-{}", *next_id);
    node.provenance = Some(id.clone());
    // latex-rust represents matrix rows as EnvRow values rather than MathNode boxes.
    if !is_matrix_row {
        ids.push(id);
    }
    *next_id += 1;
    for child in &mut node.children {
        add_provenance(
            child,
            next_id,
            ids,
            node.kind == MathSyntaxKind::Matrix && child.kind == MathSyntaxKind::Row,
        );
    }
}

fn set_kind_provenance(node: &mut MathSyntaxNode, kind: MathSyntaxKind, id: &str) -> bool {
    if node.kind == kind {
        node.provenance = Some(id.to_owned());
        return true;
    }
    node.children
        .iter_mut()
        .any(|child| set_kind_provenance(child, kind, id))
}

#[test]
fn unsupported_math_font_preference_is_retained_in_layout_diagnostics() {
    let parsed = parse("x", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("formula should parse: {error}"));
    let tree = parsed
        .syntax_tree()
        .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
    let backend = MathLayoutBackend::for_requested_font_family(Some("Future Math Font"))
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));
    let snapshot = backend
        .layout(&tree, 1200, &FixedTextShaper)
        .unwrap_or_else(|error| panic!("formula should lay out: {error}"));

    assert_eq!(
        snapshot.diagnostics,
        vec![super::MathLayoutDiagnostic::UnsupportedFontFamily {
            requested_family: "Future Math Font".into(),
            used_family: "STIX Two Math",
        }]
    );
}

#[test]
fn text_metrics_are_applied_before_fraction_packing() {
    struct WidthShaper(i64);

    impl MathTextShaper for WidthShaper {
        fn shape_text(
            &self,
            _text: &str,
            _style: latex_rust::TextStyle,
            _font_size_micro_pt: i64,
        ) -> Result<MathTextMetrics, String> {
            Ok(MathTextMetrics {
                handle: 1,
                width_micro_pt: self.0,
                ascent_micro_pt: self.0 / 2,
                descent_micro_pt: 0,
            })
        }
    }

    let parsed = parse(r"\frac{\text{x}}{b}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("formula should parse: {error}"));
    let tree = parsed
        .syntax_tree()
        .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
    let backend = MathLayoutBackend::new()
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));
    let narrow = backend
        .layout(&tree, 1200, &WidthShaper(1_000_000))
        .unwrap_or_else(|error| panic!("narrow formula should lay out: {error}"));
    let wide = backend
        .layout(&tree, 1200, &WidthShaper(20_000_000))
        .unwrap_or_else(|error| panic!("wide formula should lay out: {error}"));
    assert!(wide.width_micro_pt > narrow.width_micro_pt);
}

#[test]
fn nested_text_styles_flush_before_resuming_the_parent_style() {
    struct RecordingShaper(RefCell<Vec<(String, latex_rust::TextStyle)>>);

    impl MathTextShaper for RecordingShaper {
        fn shape_text(
            &self,
            text: &str,
            style: latex_rust::TextStyle,
            font_size_micro_pt: i64,
        ) -> Result<MathTextMetrics, String> {
            self.0.borrow_mut().push((text.to_owned(), style));
            Ok(MathTextMetrics {
                handle: 1,
                width_micro_pt: font_size_micro_pt,
                ascent_micro_pt: font_size_micro_pt / 2,
                descent_micro_pt: 0,
            })
        }
    }

    let parsed = parse(r"\text{a\textbf{b}c}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("formula should parse: {error}"));
    let tree = parsed
        .syntax_tree()
        .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
    let backend =
        MathLayoutBackend::new().unwrap_or_else(|error| panic!("math font should load: {error}"));
    let shaper = RecordingShaper(RefCell::new(Vec::new()));
    backend
        .layout(&tree, 1200, &shaper)
        .unwrap_or_else(|error| panic!("formula should lay out: {error}"));

    assert_eq!(
        *shaper.0.borrow(),
        vec![
            ("a".into(), latex_rust::TextStyle::Text),
            ("b".into(), latex_rust::TextStyle::Bf),
            ("c".into(), latex_rust::TextStyle::Text),
        ]
    );
}

#[test]
fn invalid_size_and_provider_failure_return_no_snapshot() {
    let parsed = parse(r"\text{x}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("formula should parse: {error}"));
    let tree = parsed
        .syntax_tree()
        .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
    let backend = MathLayoutBackend::new()
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));

    assert_eq!(
        backend.layout(&tree, 0, &FixedTextShaper),
        Err(MathLayoutError::InvalidFontSize)
    );

    struct FailingShaper {
        committed: Cell<Option<bool>>,
    }

    impl MathTextShaper for FailingShaper {
        fn finish_layout(&self, committed: bool) {
            self.committed.set(Some(committed));
        }

        fn shape_text(
            &self,
            _text: &str,
            _style: latex_rust::TextStyle,
            _font_size_micro_pt: i64,
        ) -> Result<MathTextMetrics, String> {
            Err("font unavailable".into())
        }
    }
    let shaper = FailingShaper {
        committed: Cell::new(None),
    };
    assert!(matches!(
        backend.layout(&tree, 1200, &shaper),
        Err(MathLayoutError::Backend(_))
    ));
    assert_eq!(shaper.committed.get(), Some(false));
}

#[test]
fn a_later_shaping_failure_rolls_back_earlier_handles() {
    struct FailsAfterFirst {
        calls: Cell<usize>,
        committed: Cell<Option<bool>>,
    }

    impl MathTextShaper for FailsAfterFirst {
        fn finish_layout(&self, committed: bool) {
            self.committed.set(Some(committed));
        }

        fn shape_text(
            &self,
            _text: &str,
            _style: latex_rust::TextStyle,
            font_size_micro_pt: i64,
        ) -> Result<MathTextMetrics, String> {
            let call = self.calls.get();
            self.calls.set(call + 1);
            if call > 0 {
                return Err("second run failed".into());
            }
            Ok(MathTextMetrics {
                handle: 42,
                width_micro_pt: font_size_micro_pt,
                ascent_micro_pt: font_size_micro_pt / 2,
                descent_micro_pt: 0,
            })
        }
    }

    let parsed = parse(r"\text{a\textbf{b}}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("formula should parse: {error}"));
    let tree = parsed
        .syntax_tree()
        .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
    let backend =
        MathLayoutBackend::new().unwrap_or_else(|error| panic!("math font should load: {error}"));
    let shaper = FailsAfterFirst {
        calls: Cell::new(0),
        committed: Cell::new(None),
    };

    assert!(backend.layout(&tree, 1200, &shaper).is_err());
    assert_eq!(shaper.calls.get(), 2);
    assert_eq!(shaper.committed.get(), Some(false));
}

#[test]
fn successful_layout_commits_provider_handles() {
    struct CommittingShaper(Cell<Option<bool>>);

    impl MathTextShaper for CommittingShaper {
        fn finish_layout(&self, committed: bool) {
            self.0.set(Some(committed));
        }

        fn shape_text(
            &self,
            _text: &str,
            _style: latex_rust::TextStyle,
            font_size_micro_pt: i64,
        ) -> Result<MathTextMetrics, String> {
            Ok(MathTextMetrics {
                handle: 7,
                width_micro_pt: font_size_micro_pt,
                ascent_micro_pt: font_size_micro_pt / 2,
                descent_micro_pt: 0,
            })
        }
    }

    let parsed = parse(r"\text{a}", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("formula should parse: {error}"));
    let tree = parsed
        .syntax_tree()
        .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
    let backend =
        MathLayoutBackend::new().unwrap_or_else(|error| panic!("math font should load: {error}"));
    let shaper = CommittingShaper(Cell::new(None));

    backend
        .layout(&tree, 1200, &shaper)
        .unwrap_or_else(|error| panic!("formula should lay out: {error}"));
    assert_eq!(shaper.0.get(), Some(true));
}

#[test]
fn public_syntax_tree_is_budget_checked_before_recursive_conversion() {
    let parsed = parse("x", MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("formula should parse: {error}"));
    let mut tree = parsed
        .syntax_tree()
        .unwrap_or_else(|error| panic!("syntax conversion should succeed: {error}"));
    for _ in 0..crate::MAX_MATH_NESTING_DEPTH {
        tree.root = MathSyntaxNode {
            kind: MathSyntaxKind::Row,
            text: None,
            attributes: BTreeMap::new(),
            source_range: None,
            provenance: None,
            children: vec![tree.root],
        };
    }
    let backend = MathLayoutBackend::new()
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));
    assert!(matches!(
        backend.layout(&tree, 1200, &FixedTextShaper),
        Err(MathLayoutError::UnsupportedTree(_))
    ));
}

#[test]
fn multi_character_atom_expansion_counts_toward_the_layout_budget() {
    let tree = crate::MathSyntaxTree {
        input_kind: MathInputKind::Inline,
        root: MathSyntaxNode {
            kind: MathSyntaxKind::Symbol,
            text: Some("x".repeat(crate::MAX_MATH_SYNTAX_NODES + 1)),
            attributes: BTreeMap::from([("atomClass".into(), "ord".into())]),
            source_range: None,
            provenance: None,
            children: Vec::new(),
        },
    };
    let backend =
        MathLayoutBackend::new().unwrap_or_else(|error| panic!("math font should load: {error}"));
    assert!(matches!(
        backend.layout(&tree, 1200, &FixedTextShaper),
        Err(MathLayoutError::UnsupportedTree(_))
    ));
}
