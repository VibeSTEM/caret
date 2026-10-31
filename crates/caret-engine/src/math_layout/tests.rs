//! 验证持久化数学节点映射到布局 provenance 与源码范围。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{layout_math_tree, syntax_tree_from_model};
use crate::{ImportedMath, import_parsed_math};
use caret_doc::DocumentSettings;
use caret_layout::{
    FontCatalog, ParagraphLayouter, ParleyMathTextShaper, ResolvedDocumentTextSettings,
    ShapingStyle,
};
use caret_math::{
    MathInputKind, MathLayoutBackend, MathLayoutItem, MathTextMetrics, MathTextShaper,
    MathTextStyle, parse,
};

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
