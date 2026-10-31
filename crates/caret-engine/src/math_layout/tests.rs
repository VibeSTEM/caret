//! 验证持久化数学节点映射、透明分组与布局 provenance。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{layout_math_tree, syntax_tree_from_model};
use crate::{ImportedMath, import_parsed_math};
use caret_doc::{
    DocumentSettings, MATH_TREE_SCHEMA_VERSION, MathNode, MathNodeKind, MathTree, NodeId,
};
use caret_layout::{
    FontCatalog, ParagraphLayouter, ParleyMathTextShaper, ResolvedDocumentTextSettings,
    ShapingStyle,
};
use caret_math::{
    MathInputKind, MathLayoutBackend, MathLayoutItem, MathTextMetrics, MathTextShaper,
    MathTextStyle, parse,
};
use std::collections::BTreeMap;

#[test]
fn persisted_group_is_a_transparent_layout_scope_with_provenance() {
    struct EmptyTextShaper;
    impl MathTextShaper for EmptyTextShaper {
        fn shape_text(
            &self,
            _text: &str,
            _style: MathTextStyle,
            _font_size_micro_pt: i64,
        ) -> Result<MathTextMetrics, String> {
            Err("unexpected text shaping".into())
        }
    }

    fn node(kind: MathNodeKind, text: Option<&str>, children: Vec<MathNode>) -> MathNode {
        let attributes = if kind == MathNodeKind::Symbol {
            BTreeMap::from([("atomClass".into(), "ord".into())])
        } else {
            BTreeMap::new()
        };
        MathNode {
            id: NodeId::generate(),
            kind,
            text: text.map(str::to_owned),
            attributes,
            source_range: None,
            children,
            extra: BTreeMap::new(),
        }
    }
    fn tree(base: MathNode, superscript: bool) -> MathTree {
        let root = if superscript {
            node(
                MathNodeKind::Superscript,
                None,
                vec![base, node(MathNodeKind::Symbol, Some("2"), Vec::new())],
            )
        } else {
            let mut accent = node(MathNodeKind::Accent, None, vec![base]);
            accent.attributes.insert("accent".into(), "hat".into());
            accent
        };
        MathTree {
            schema_version: MATH_TREE_SCHEMA_VERSION,
            source: if superscript { "x^2" } else { r"\hat{x}" }.into(),
            root,
            extra: BTreeMap::new(),
        }
    }

    let backend = MathLayoutBackend::new()
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));
    let shaper = EmptyTextShaper;
    for superscript in [true, false] {
        let symbol = node(MathNodeKind::Symbol, Some("x"), Vec::new());
        let symbol_id = symbol.id.to_string();
        let direct = tree(symbol, superscript);
        let group_id = NodeId::generate();
        let grouped_symbol = node(MathNodeKind::Symbol, Some("x"), Vec::new());
        let grouped_symbol_id = grouped_symbol.id.to_string();
        let grouped = tree(
            MathNode {
                id: group_id.clone(),
                kind: MathNodeKind::Group,
                text: None,
                attributes: BTreeMap::new(),
                source_range: None,
                children: vec![grouped_symbol],
                extra: BTreeMap::new(),
            },
            superscript,
        );
        let direct_snapshot =
            layout_math_tree(&backend, &direct, MathInputKind::Inline, 1200, &shaper)
                .unwrap_or_else(|error| panic!("ungrouped expression should lay out: {error}"));
        let grouped_snapshot =
            layout_math_tree(&backend, &grouped, MathInputKind::Inline, 1200, &shaper)
                .unwrap_or_else(|error| panic!("grouped expression should lay out: {error}"));
        assert_eq!(grouped_snapshot.items, direct_snapshot.items);
        assert_eq!(
            grouped_snapshot.width_micro_pt,
            direct_snapshot.width_micro_pt
        );
        assert_eq!(
            grouped_snapshot.ascent_micro_pt,
            direct_snapshot.ascent_micro_pt
        );
        assert_eq!(
            grouped_snapshot.descent_micro_pt,
            direct_snapshot.descent_micro_pt
        );
        let inner_geometry = grouped_snapshot
            .nodes
            .iter()
            .find(|geometry| geometry.node_id == grouped_symbol_id)
            .unwrap_or_else(|| panic!("grouped symbol geometry should be present"));
        let group_geometry = grouped_snapshot
            .nodes
            .iter()
            .find(|geometry| geometry.node_id == group_id.to_string())
            .unwrap_or_else(|| panic!("transparent group should alias child geometry"));
        assert_eq!(
            (
                group_geometry.x_micro_pt,
                group_geometry.y_micro_pt,
                group_geometry.width_micro_pt,
                group_geometry.ascent_micro_pt,
                group_geometry.descent_micro_pt,
            ),
            (
                inner_geometry.x_micro_pt,
                inner_geometry.y_micro_pt,
                inner_geometry.width_micro_pt,
                inner_geometry.ascent_micro_pt,
                inner_geometry.descent_micro_pt,
            )
        );
        assert!(
            direct_snapshot
                .nodes
                .iter()
                .any(|geometry| geometry.node_id == symbol_id)
        );
    }
}

#[test]
fn persisted_group_inside_text_keeps_text_shaping_context() {
    struct TextShaper;
    impl MathTextShaper for TextShaper {
        fn shape_text(
            &self,
            text: &str,
            _style: MathTextStyle,
            font_size_micro_pt: i64,
        ) -> Result<MathTextMetrics, String> {
            Ok(MathTextMetrics {
                handle: u64::try_from(text.len()).map_err(|error| error.to_string())?,
                width_micro_pt: font_size_micro_pt,
                ascent_micro_pt: font_size_micro_pt / 2,
                descent_micro_pt: 0,
            })
        }
    }
    let text = MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Text,
        text: None,
        attributes: BTreeMap::from([("style".into(), "text".into())]),
        source_range: None,
        children: vec![
            MathNode {
                id: NodeId::generate(),
                kind: MathNodeKind::TextContent,
                text: Some("前".into()),
                attributes: BTreeMap::new(),
                source_range: None,
                children: Vec::new(),
                extra: BTreeMap::new(),
            },
            MathNode {
                id: NodeId::generate(),
                kind: MathNodeKind::Group,
                text: None,
                attributes: BTreeMap::new(),
                source_range: None,
                children: vec!["中", "文"]
                    .into_iter()
                    .map(|value| MathNode {
                        id: NodeId::generate(),
                        kind: MathNodeKind::TextContent,
                        text: Some(value.into()),
                        attributes: BTreeMap::new(),
                        source_range: None,
                        children: Vec::new(),
                        extra: BTreeMap::new(),
                    })
                    .collect(),
                extra: BTreeMap::new(),
            },
            MathNode {
                id: NodeId::generate(),
                kind: MathNodeKind::TextContent,
                text: Some("后".into()),
                attributes: BTreeMap::new(),
                source_range: None,
                children: Vec::new(),
                extra: BTreeMap::new(),
            },
        ],
        extra: BTreeMap::new(),
    };
    let text_id = text.id.to_string();
    let group_id = text.children[1].id.to_string();
    let tree = MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: r"\text{前{中文}后}".into(),
        root: text,
        extra: BTreeMap::new(),
    };
    let backend = MathLayoutBackend::new()
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));
    let snapshot = layout_math_tree(&backend, &tree, MathInputKind::Inline, 1200, &TextShaper)
        .unwrap_or_else(|error| panic!("text group should preserve text shaping: {error}"));
    assert!(
        snapshot
            .items
            .iter()
            .any(|item| matches!(item, MathLayoutItem::ExternalText { .. }))
    );
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|geometry| geometry.node_id == text_id)
    );
    assert!(
        !snapshot
            .nodes
            .iter()
            .any(|geometry| geometry.node_id == group_id)
    );
}

#[test]
fn invalid_or_over_budget_model_trees_fail_before_opening_text_shaper_transaction() {
    struct CountingShaper(std::cell::Cell<usize>);
    impl MathTextShaper for CountingShaper {
        fn begin_layout(&self) -> Result<(), String> {
            self.0.set(self.0.get() + 1);
            Ok(())
        }

        fn shape_text(
            &self,
            _text: &str,
            _style: MathTextStyle,
            _font_size_micro_pt: i64,
        ) -> Result<MathTextMetrics, String> {
            Err("no text is expected".into())
        }
    }
    fn empty_node(kind: MathNodeKind, children: Vec<MathNode>) -> MathNode {
        MathNode {
            id: NodeId::generate(),
            kind,
            text: None,
            attributes: BTreeMap::new(),
            source_range: None,
            children,
            extra: BTreeMap::new(),
        }
    }
    let backend = MathLayoutBackend::new()
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));
    let shaper = CountingShaper(std::cell::Cell::new(0));

    let symbol = MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Symbol,
        text: Some("x".into()),
        attributes: BTreeMap::from([("atomClass".into(), "ord".into())]),
        source_range: None,
        children: Vec::new(),
        extra: BTreeMap::new(),
    };
    let deep_root = (0..=caret_doc::MAX_MATH_TREE_DEPTH).fold(symbol, |child, _| {
        empty_node(MathNodeKind::Group, vec![child])
    });
    let deep_tree = MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: "x".into(),
        root: deep_root,
        extra: BTreeMap::new(),
    };
    assert!(layout_math_tree(&backend, &deep_tree, MathInputKind::Inline, 1200, &shaper).is_err());

    let mut invalid_group = empty_node(
        MathNodeKind::Group,
        vec![MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Symbol,
            text: Some("x".into()),
            attributes: BTreeMap::from([("atomClass".into(), "ord".into())]),
            source_range: None,
            children: Vec::new(),
            extra: BTreeMap::new(),
        }],
    );
    invalid_group.source_range = Some(caret_doc::MathSourceRange {
        start: 0,
        end: 4,
        extra: BTreeMap::new(),
    });
    let invalid_range_tree = MathTree {
        schema_version: MATH_TREE_SCHEMA_VERSION,
        source: "x".into(),
        root: invalid_group,
        extra: BTreeMap::new(),
    };
    assert!(
        layout_math_tree(
            &backend,
            &invalid_range_tree,
            MathInputKind::Inline,
            1200,
            &shaper,
        )
        .is_err()
    );
    assert_eq!(shaper.0.get(), 0);
}

#[test]
fn persistent_math_ids_and_root_source_range_reach_layout_syntax() {
    let source = r"\frac{α}{b}";
    let parsed = parse(source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("formula should parse: {error}"));
    let ImportedMath::Inline(inline) = import_parsed_math(&parsed)
        .unwrap_or_else(|error| panic!("formula should import: {error}"))
    else {
        panic!("inline input should import as inline math");
    };
    let tree = syntax_tree_from_model(&inline.math_tree, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("model should map to layout syntax: {error}"));

    assert_eq!(
        tree.root.provenance.as_deref(),
        Some(inline.math_tree.root.id.as_str())
    );
    assert_eq!(tree.root.source_range, Some(0..source.len()));
    assert_eq!(tree.root.children.len(), 2);
    assert!(
        tree.root
            .children
            .iter()
            .all(|node| node.provenance.is_some())
    );

    struct TextShaper;
    impl MathTextShaper for TextShaper {
        fn shape_text(
            &self,
            text: &str,
            _style: MathTextStyle,
            font_size_micro_pt: i64,
        ) -> Result<MathTextMetrics, String> {
            Ok(MathTextMetrics {
                handle: u64::try_from(text.len())
                    .unwrap_or_else(|error| panic!("test handle should fit u64: {error}")),
                width_micro_pt: font_size_micro_pt,
                ascent_micro_pt: font_size_micro_pt / 2,
                descent_micro_pt: 0,
            })
        }
    }

    let backend = MathLayoutBackend::new()
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));
    let snapshot = layout_math_tree(
        &backend,
        &inline.math_tree,
        MathInputKind::Inline,
        1200,
        &TextShaper,
    )
    .unwrap_or_else(|error| panic!("persistent formula should lay out: {error}"));
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.node_id == inline.math_tree.root.id.as_str())
    );
    assert!(
        snapshot
            .items
            .iter()
            .any(|item| matches!(item, MathLayoutItem::Rule { .. }))
    );
}

#[test]
fn math_alphabet_commands_keep_stix_glyphs_with_the_real_body_text_provider() {
    let source = r"\mathbb{R}+\mathcal{A}+\mathfrak{g}+\mathscr{F}";
    let parsed = parse(source, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("math alphabet formula should parse: {error}"));
    let ImportedMath::Inline(inline) = import_parsed_math(&parsed)
        .unwrap_or_else(|error| panic!("math alphabet formula should import: {error}"))
    else {
        panic!("inline input should import as inline math");
    };
    let backend = MathLayoutBackend::new()
        .unwrap_or_else(|error| panic!("pinned math font should load: {error}"));
    let mut catalog = FontCatalog::system()
        .unwrap_or_else(|error| panic!("system font catalog should load: {error}"));
    let mut layouter = ParagraphLayouter::new();
    let settings =
        ResolvedDocumentTextSettings::from_document_settings(&DocumentSettings::default())
            .unwrap_or_else(|error| panic!("default text settings should resolve: {error}"));
    let base_style = ShapingStyle::new(1200);
    let provider = ParleyMathTextShaper::new(&mut catalog, &mut layouter, &settings, &base_style);
    struct RejectBodyText;
    impl MathTextShaper for RejectBodyText {
        fn shape_text(
            &self,
            _text: &str,
            _style: MathTextStyle,
            _font_size_micro_pt: i64,
        ) -> Result<MathTextMetrics, String> {
            Err("math alphabet unexpectedly requested body text shaping".into())
        }
    }
    let syntax = syntax_tree_from_model(&inline.math_tree, MathInputKind::Inline)
        .unwrap_or_else(|error| panic!("math alphabet tree should convert: {error}"));
    let expected = backend
        .layout(&syntax, 1200, &RejectBodyText)
        .unwrap_or_else(|error| panic!("math alphabet should not need body shaping: {error}"));
    let snapshot = layout_math_tree(
        &backend,
        &inline.math_tree,
        MathInputKind::Inline,
        1200,
        &provider,
    )
    .unwrap_or_else(|error| panic!("math alphabet formula should lay out: {error}"));

    assert!(
        snapshot
            .items
            .iter()
            .any(|item| matches!(item, MathLayoutItem::Glyph { .. }))
    );
    assert_eq!(snapshot.items, expected.items);
    assert!(
        snapshot
            .items
            .iter()
            .all(|item| !matches!(item, MathLayoutItem::ExternalText { .. }))
    );
    assert!(provider.take_resources().is_empty());
}
