//! 公式布局的行为测试：几何关系、覆盖语料、缓存。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{collections::BTreeMap, sync::Arc};

use caret_doc::{
    FlowContent, InlineEnvironment, InlineEnvironmentKind, MathContent, MathFlow, MathNode,
    MathNodeKind, MathPathStep, NodeId, Run, TextFlow,
};

use super::{
    MathItem, MathLayout, MathLayoutEngine, MathLayoutError, MathNodeBox, MathStyle,
    MathTextRequest, MathTextRunBox, MathTextShape, MathTextShapeError, MathTextShaper,
    layout_align, layout_math, layout_math_with_text_shaper,
};
use crate::layout::MathFont;
use crate::{MathInputKind, ParsedMath, parse};

const EPSILON: f64 = 1e-6;
const SIZE: f64 = 10.0;

fn font() -> &'static MathFont {
    MathFont::builtin().unwrap_or_else(|error| panic!("内置字体加载失败：{error}"))
}

fn root(source: &str) -> MathFlow {
    match parse(source, MathInputKind::Equation) {
        Ok(ParsedMath::Tree(tree)) => tree.root,
        other => panic!("{source}: {other:?}"),
    }
}

fn lay(source: &str, style: MathStyle) -> MathLayout {
    layout_math(font(), &root(source), SIZE, style)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"))
}

fn display(source: &str) -> MathLayout {
    lay(source, MathStyle::Display)
}

fn inline(source: &str) -> MathLayout {
    lay(source, MathStyle::Inline)
}

fn height(layout: &MathLayout) -> f64 {
    layout.ascent + layout.descent
}

fn assert_rendered_slot(layout: &MathLayout, slot: &MathNodeBox) {
    assert!(slot.width.is_finite() && slot.width > 0.0);
    assert!(slot.height.is_finite() && slot.height > 0.0);
    assert!(layout.items.iter().any(|item| matches!(item,
        MathItem::Rectangle { x, y, width, height, stroke: Some(_), stroke_width, .. }
        if (x - slot.x).abs() < EPSILON && (y - slot.y).abs() < EPSILON
            && (width - slot.width).abs() < EPSILON && (height - slot.height).abs() < EPSILON
            && *stroke_width > 0.0
    )));
}

fn glyphs(layout: &MathLayout) -> Vec<(f64, f64, f64)> {
    layout
        .items
        .iter()
        .filter_map(|item| match item {
            MathItem::Glyph { x, y, size, .. } => Some((*x, *y, *size)),
            _ => None,
        })
        .collect()
}

fn glyph_ids(layout: &MathLayout) -> Vec<u16> {
    layout
        .items
        .iter()
        .filter_map(|item| match item {
            MathItem::Glyph { glyph, .. } => Some(*glyph),
            _ => None,
        })
        .collect()
}

fn finite(layout: &MathLayout) -> bool {
    let mut values = vec![layout.width, layout.ascent, layout.descent];
    for item in &layout.items {
        match item {
            MathItem::Glyph { x, y, size, .. } => values.extend([*x, *y, *size]),
            MathItem::Rule {
                x,
                y,
                width,
                height,
                ..
            } => values.extend([*x, *y, *width, *height]),
            MathItem::Line {
                x,
                y,
                dx,
                dy,
                thickness,
                ..
            } => values.extend([*x, *y, *dx, *dy, *thickness]),
            MathItem::Rectangle {
                x,
                y,
                width,
                height,
                stroke_width,
                ..
            } => values.extend([*x, *y, *width, *height, *stroke_width]),
            MathItem::TextIsland {
                x,
                baseline_y,
                width,
                ..
            } => values.extend([*x, *baseline_y, *width]),
        }
    }
    values.iter().all(|value| value.is_finite())
}

#[test]
fn font_has_math_table() {
    let font = font();
    assert!(font.units_per_em() > 0.0);
    assert!(font.glyph('∑').is_some());
}

#[test]
fn corpus_lays_out_without_diagnostics() {
    let corpus = [
        r"x^2 + y^2 = z^2",
        r"\frac{a}{b} + \dfrac{1}{2} - \tfrac{3}{4}",
        r"\sqrt{x} + \sqrt[3]{y^2}",
        r"\sum_{i=1}^{n} i = \frac{n(n+1)}{2}",
        r"\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt{\pi}}{2}",
        r"\prod_{k} a_k \iint f \oint g",
        r"\lim_{x \to 0} \frac{\sin x}{x} = 1",
        r"\left( \frac{a}{b} \right) \left[ x \right] \left\{ y \right\}",
        r"\bigl( \Bigr) \biggl[ \Biggr]",
        r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
        r"\begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix} \begin{vmatrix} a \end{vmatrix}",
        r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & \text{otherwise} \end{cases}",
        r"\begin{array}{c|l} a & b \\ \hline c & d \end{array}",
        r"\hat{x} \bar{y} \vec{v} \dot{a} \ddot{b} \tilde{n} \widehat{xyz}",
        r"\overline{AB} \underline{cd} \overbrace{a+b}^{n} \underbrace{c+d}_{m}",
        r"\overrightarrow{AB} \overleftarrow{CD} \underrightarrow{x}",
        r"\cancel{x} \bcancel{y} \xcancel{z} \cancelto{0}{t}",
        r"\boxed{E = mc^2} \not= \mathbb{R} \mathcal{L} \mathfrak{g} \mathbf{v}",
        r"\text{if } x \in A \quad \qquad \, \: \; \! \hspace{1em}",
        r"\color{red} x \textcolor{blue}{y} \colorbox{yellow}{z}",
        r"a \phantom{x} \hphantom{y} \vphantom{z} \rule{0em}{1em}",
        r"\overset{def}{=} \underset{n}{\max} \substack{a \\ b}",
        r"\alpha \beta \Gamma \Delta \infty \partial \nabla \pm \times \cdot",
        r"x' y'' f'(x)",
        r"\sin x + \cos y + \log z + \operatorname{tr} A",
        r"a_{i,j}^{k} {}_n x",
    ];
    for source in corpus {
        for style in [MathStyle::Display, MathStyle::Inline] {
            let layout = lay(source, style);
            assert!(
                layout.diagnostics.is_empty(),
                "{source}: {:?}",
                layout.diagnostics
            );
            assert!(finite(&layout), "{source}");
            assert!(layout.width > 0.0, "{source}");
            assert!(!layout.items.is_empty(), "{source}");
        }
    }
}

#[test]
fn text_style_resets_preserve_other_emphasis() {
    let cases = [
        (r"\text{\textbf{\textup{x}}}", r"\text{\textbf{x}}"),
        (r"\text{\textit{\textmd{x}}}", r"\text{\textit{x}}"),
    ];
    for (source, expected) in cases {
        assert_eq!(glyph_ids(&inline(source)), glyph_ids(&inline(expected)));
    }
}

#[test]
fn superscript_is_raised_and_smaller() {
    let layout = inline("x^2");
    let glyphs = glyphs(&layout);
    assert_eq!(glyphs.len(), 2);
    let (base, script) = (glyphs[0], glyphs[1]);
    assert!(script.1 < base.1 - EPSILON, "上标应高于基线");
    assert!(script.2 < base.2 - EPSILON, "上标应缩小");
    assert!(script.0 > base.0 + EPSILON);
}

#[test]
fn subscript_is_lowered() {
    let layout = inline("x_2");
    let glyphs = glyphs(&layout);
    assert!(glyphs[1].1 > glyphs[0].1 + EPSILON);
}

#[test]
fn fraction_stacks_around_axis() {
    let layout = display(r"\frac{a}{b}");
    let glyphs = glyphs(&layout);
    assert_eq!(glyphs.len(), 2);
    assert!(glyphs[0].1 < 0.0 && glyphs[1].1 > 0.0, "{glyphs:?}");
    let rule = layout.items.iter().find_map(|item| match item {
        MathItem::Rule { y, height, .. } => Some((*y, *height)),
        _ => None,
    });
    let Some((rule_y, rule_height)) = rule else {
        panic!("分数应有横线");
    };
    assert!(rule_height > 0.0);
    // 分数线中心位于数学轴（基线之上）。
    assert!(rule_y + rule_height / 2.0 < 0.0);
}

#[test]
fn display_fraction_is_taller_than_inline() {
    assert!(height(&display(r"\frac{a}{b}")) > height(&inline(r"\frac{a}{b}")) + EPSILON);
}

#[test]
fn display_sum_is_larger_and_takes_limits() {
    let big = display(r"\sum_{i}^{n}");
    let small = inline(r"\sum_{i}^{n}");
    assert!(height(&big) > height(&small) + EPSILON);
    // 显示样式上下限居中于运算符上下，宽度不超过运算符太多。
    assert!(big.width < small.width + EPSILON);
}

#[test]
fn radical_grows_with_content() {
    let short = display(r"\sqrt{x}");
    let tall = display(r"\sqrt{\frac{a}{b}}");
    assert!(height(&tall) > height(&short) + EPSILON);
}

#[test]
fn delimiters_grow_with_content() {
    let plain = display(r"\left( x \right)");
    let tall = display(r"\left( \frac{\frac{a}{b}}{c} \right)");
    assert!(height(&tall) > height(&plain) + EPSILON);
    let sized = [r"\big(", r"\Big(", r"\bigg(", r"\Bigg("].map(|s| height(&display(s)));
    for pair in sized.windows(2) {
        assert!(pair[1] > pair[0] + EPSILON, "{sized:?}");
    }
}

#[test]
fn relations_get_thick_space() {
    let tight = display("ab");
    let spaced = display("a=b");
    let equals = display("=");
    assert!(spaced.width > tight.width + equals.width + EPSILON);
}

#[test]
fn leading_minus_is_ordinary() {
    let unary = display("-a");
    let minus = display("-");
    let a = display("a");
    assert!((unary.width - minus.width - a.width).abs() < EPSILON);
}

#[test]
fn script_spacing_is_suppressed() {
    let script = inline("x^{a=b}");
    let plain = inline("x^{ab}");
    // 上标内关系符不加粗空白：宽度差约为一个缩小（约 0.7 倍）的等号。
    let equals = inline("=").width;
    let delta = script.width - plain.width;
    assert!(delta < equals * 0.8, "{delta} {equals}");
}

#[test]
fn matrix_rows_stack() {
    let one = display(r"\begin{matrix} a \end{matrix}");
    let two = display(r"\begin{matrix} a \\ b \end{matrix}");
    assert!(height(&two) > height(&one) + EPSILON);
}

#[test]
fn negative_space_does_not_erase_matrix_contents() {
    let layout = display(r"\begin{matrix}\! & x\end{matrix}");
    assert!(!glyph_ids(&layout).is_empty());
}

#[test]
fn signed_glyph_ink_metrics_do_not_erase_matrix_contents() {
    let layout = display(r"\begin{matrix}= & x\end{matrix}");
    assert!(glyph_ids(&layout).len() >= 2);
}

#[test]
fn substack_rows_do_not_duplicate_math_node_ids() {
    let layout = display(r"\substack{a\\long}");
    let ids: Vec<_> = layout
        .nodes
        .iter()
        .filter_map(|node| node.id.as_ref())
        .collect();
    assert!(
        ids.iter()
            .enumerate()
            .all(|(index, id)| { ids[index + 1..].iter().all(|later| later != id) })
    );
}

#[test]
fn node_boxes_cover_children() {
    let tree = root(r"\frac{a}{b}");
    let layout = layout_math(font(), &tree, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let first = layout
        .nodes
        .first()
        .unwrap_or_else(|| panic!("应记录根节点"));
    let Some(MathContent::Math(root_node)) = tree.content.first() else {
        panic!("formula should start with a math node");
    };
    assert_eq!(first.id.as_ref(), Some(&tree.id));
    assert!(
        layout
            .nodes
            .iter()
            .any(|node| node.id.as_ref() == Some(&root_node.id))
    );
    assert!((first.width - layout.width).abs() < EPSILON);
    for node in &layout.nodes {
        assert!(node.x >= -EPSILON && node.x + node.width <= layout.width + EPSILON);
    }
    assert!(layout.nodes.len() >= 3);
}

#[test]
fn text_run_boxes_use_flow_path_and_run_index_instead_of_generated_ids() {
    let root_id = NodeId::generate();
    let runs = vec![
        Run {
            text: "alpha".into(),
            language: Some("en".into()),
            props: std::collections::BTreeMap::from([(
                "underline".into(),
                serde_json::Value::Bool(true),
            )]),
            extra: std::collections::BTreeMap::from([(
                "vendor-run-field".into(),
                serde_json::Value::String("preserve".into()),
            )]),
        },
        Run {
            text: "βeta".into(),
            language: Some("el".into()),
            props: Default::default(),
            extra: Default::default(),
        },
    ];
    let root = MathFlow {
        id: root_id.clone(),
        content: vec![FlowContent::Text(TextFlow {
            runs: runs.clone(),
            extra: Default::default(),
        })],
        extra: Default::default(),
    };
    let backend_nodes = crate::backend::flow_nodes_for_layout(&root);
    let layout_runs: Vec<_> = backend_nodes[0]
        .children
        .iter()
        .map(|node| node.run.clone())
        .collect();
    assert_eq!(layout_runs, runs.into_iter().map(Some).collect::<Vec<_>>());

    let layout = layout_math(font(), &root, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let run_boxes: Vec<_> = layout
        .nodes
        .iter()
        .filter(|node| node.text_run_index.is_some())
        .collect();

    assert_eq!(run_boxes.len(), 2);
    for (run_index, node) in run_boxes.iter().enumerate() {
        assert_eq!(node.id, None, "TextFlow runs have no persistent NodeId");
        assert_eq!(node.scope_id, root_id);
        assert_eq!(node.path, [MathPathStep::Content(0)]);
        assert_eq!(node.text_run_index, Some(run_index));
    }
}

#[test]
fn text_run_paths_include_math_operands() {
    let root = root(r"\frac{\text{numerator}}{\text{denominator}}");
    let layout = layout_math(font(), &root, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let paths: Vec<_> = layout
        .nodes
        .iter()
        .filter(|node| node.text_run_index == Some(0))
        .map(|node| node.path.clone())
        .collect();

    assert_eq!(paths.len(), 2);
    assert_eq!(
        paths[0],
        [
            MathPathStep::Content(0),
            MathPathStep::Operand(0),
            MathPathStep::Content(0),
        ]
    );
    assert_eq!(
        paths[1],
        [
            MathPathStep::Content(0),
            MathPathStep::Operand(1),
            MathPathStep::Content(0),
        ]
    );
}

#[test]
fn matrix_row_boxes_keep_the_root_scope_and_operand_path() {
    let root = root(r"\begin{matrix} a & b \\ c & d \end{matrix}");
    let Some(MathContent::Math(matrix)) = root.content.first() else {
        panic!("matrix should be a root math node");
    };
    let layout = layout_math(font(), &root, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));

    for (row_index, row) in matrix.children.iter().enumerate() {
        let row_box = layout
            .nodes
            .iter()
            .find(|node| node.id.as_ref() == Some(&row.id))
            .unwrap_or_else(|| panic!("missing matrix row box for {}", row.id));
        assert_eq!(row_box.scope_id, root.id);
        assert_eq!(
            row_box.path,
            [MathPathStep::Content(0), MathPathStep::Operand(row_index)]
        );
    }
}

#[test]
fn text_run_paths_include_nested_environment_children() {
    let root = MathFlow {
        id: NodeId::generate(),
        content: vec![FlowContent::Environment(InlineEnvironment {
            id: NodeId::generate(),
            kind: InlineEnvironmentKind::Bold,
            definition_id: None,
            props: Default::default(),
            children: vec![FlowContent::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Italic,
                definition_id: None,
                props: Default::default(),
                children: vec![FlowContent::Text(TextFlow {
                    runs: vec![Run {
                        text: "nested".into(),
                        language: Some("en".into()),
                        props: Default::default(),
                        extra: Default::default(),
                    }],
                    extra: Default::default(),
                })],
                extra: Default::default(),
            })],
            extra: Default::default(),
        })],
        extra: Default::default(),
    };
    let layout = layout_math(font(), &root, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let run_box = layout
        .nodes
        .iter()
        .find(|node| node.text_run_index == Some(0))
        .unwrap_or_else(|| panic!("nested text run box should be emitted"));

    assert_eq!(run_box.scope_id, root.id);
    assert_eq!(
        run_box.path,
        [
            MathPathStep::Content(0),
            MathPathStep::EnvironmentChild(0),
            MathPathStep::EnvironmentChild(0),
        ]
    );
}

#[test]
fn align_text_run_scopes_distinguish_repeated_cell_paths() {
    let align = match parse(
        r"\begin{aligned} \text{a} & \text{b} \\ \text{c} & \text{d} \end{aligned}",
        MathInputKind::Align,
    ) {
        Ok(ParsedMath::Align(align)) => align,
        other => panic!("valid aligned input should parse: {other:?}"),
    };
    let layout = layout_align(font(), &align, SIZE)
        .unwrap_or_else(|error| panic!("valid Align should lay out: {error}"));

    for row in &align.rows {
        for cell in &row.cells {
            let run_box = layout
                .nodes
                .iter()
                .find(|node| node.scope_id == cell.root.id && node.text_run_index == Some(0))
                .unwrap_or_else(|| panic!("missing text run for Align cell {}", cell.root.id));
            assert_eq!(run_box.path, [MathPathStep::Content(0)]);
        }
    }
}

#[test]
fn empty_math_and_text_flows_emit_stable_position_boxes() {
    let empty_math_flow = MathFlow {
        id: NodeId::generate(),
        content: Vec::new(),
        extra: Default::default(),
    };
    let empty_math_layout = layout_math(font(), &empty_math_flow, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let math_slot = empty_math_layout
        .nodes
        .first()
        .unwrap_or_else(|| panic!("empty MathFlow should retain a position box"));
    assert_eq!(math_slot.id.as_ref(), Some(&empty_math_flow.id));
    assert_eq!(math_slot.scope_id, empty_math_flow.id);
    assert!(!math_slot.is_text_flow);
    assert!(math_slot.is_flow);
    assert!(math_slot.path.is_empty());
    assert_rendered_slot(&empty_math_layout, math_slot);
    let larger = layout_math(font(), &empty_math_flow, SIZE * 2.0, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let larger_slot = &larger.nodes[0];
    assert!((larger_slot.width - math_slot.width * 2.0).abs() < EPSILON);
    assert!((larger_slot.height - math_slot.height * 2.0).abs() < EPSILON);
    assert!(empty_math_flow.content.is_empty());

    let empty_text_flow = MathFlow {
        id: NodeId::generate(),
        content: vec![FlowContent::Text(TextFlow::default())],
        extra: Default::default(),
    };
    let empty_text_layout = layout_math(font(), &empty_text_flow, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let text_slot = empty_text_layout
        .nodes
        .iter()
        .find(|node| node.is_text_flow)
        .unwrap_or_else(|| panic!("empty TextFlow should retain a position box"));
    assert_eq!(text_slot.id, None);
    assert_eq!(text_slot.scope_id, empty_text_flow.id);
    assert_eq!(text_slot.path, [MathPathStep::Content(0)]);
    assert!(text_slot.is_text_flow);
    assert_eq!(text_slot.text_run_index, None);
}

#[test]
fn empty_math_flow_operand_keeps_its_parent_path_box() {
    let mut root = root(r"\frac{x}{y}");
    let Some(MathContent::Math(fraction)) = root.content.first_mut() else {
        panic!("fraction should be a root math node");
    };
    let Some(empty_operand) = fraction.children.first_mut() else {
        panic!("fraction should have a numerator flow");
    };
    empty_operand.content.clear();
    let empty_flow_id = empty_operand.id.clone();
    let layout = layout_math(font(), &root, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let empty_box = layout
        .nodes
        .iter()
        .find(|node| node.id.as_ref() == Some(&empty_flow_id))
        .unwrap_or_else(|| panic!("empty MathFlow operand should retain a position box"));

    assert!(empty_box.is_flow);
    assert_eq!(empty_box.scope_id, root.id);
    assert_eq!(
        empty_box.path,
        [MathPathStep::Content(0), MathPathStep::Operand(0)]
    );
    assert_rendered_slot(&layout, empty_box);
    let MathContent::Math(fraction) = &root.content[0] else {
        panic!("expected fraction")
    };
    assert!(fraction.children[0].content.is_empty());
}

#[test]
fn math_flow_boxes_retain_the_effective_script_font_size() {
    let root = root(r"x^{\frac{}{}}");
    let layout = layout_math(font(), &root, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("script fraction should layout: {error}"));
    let script_slot = layout
        .nodes
        .iter()
        .find(|node| {
            node.is_flow
                && !node.is_text_flow
                && node.font_size_pt.is_some_and(|size| size < SIZE)
                && node.width > 0.0
                && node.height > 0.0
        })
        .unwrap_or_else(|| panic!("script flow should retain its effective font size"));

    assert!(script_slot.font_size_pt.is_some_and(|size| size < SIZE));
}

#[test]
fn placeholder_operand_retains_flow_identity_and_single_box() {
    let root = root(r"\frac{}{x}");
    let MathContent::Math(fraction) = &root.content[0] else {
        panic!("expected fraction");
    };
    let flow = &fraction.children[0];
    assert!(
        matches!(flow.content.as_slice(), [MathContent::Math(node)] if node.kind == MathNodeKind::Placeholder)
    );
    let layout = layout_math(font(), &root, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let slot = layout
        .nodes
        .iter()
        .find(|node| node.id.as_ref() == Some(&flow.id))
        .unwrap_or_else(|| panic!("missing placeholder flow"));
    assert!(slot.is_flow);
    assert_eq!(slot.scope_id, root.id);
    assert_eq!(
        slot.path,
        [MathPathStep::Content(0), MathPathStep::Operand(0)]
    );
    assert_rendered_slot(&layout, slot);
    assert_eq!(
        layout
            .items
            .iter()
            .filter(|item| matches!(item, MathItem::Rectangle { .. }))
            .count(),
        1
    );
}

#[test]
fn optional_placeholder_flows_keep_identity_without_changing_rendering() {
    for (source, indices) in [(r"\sqrt{x}", &[0_usize][..]), (r"\sum", &[0, 1][..])] {
        let root = root(source);
        let MathContent::Math(node) = &root.content[0] else {
            panic!("expected a math node: {source}");
        };
        let layout = layout_math(font(), &root, SIZE, MathStyle::Display)
            .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
        let without_positions = super::layout_nodes(
            font(),
            crate::backend::flow_nodes(&root),
            SIZE,
            MathStyle::Display,
            None,
        )
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
        assert_eq!(layout.items, without_positions.items, "{source}");
        assert_eq!(layout.width, without_positions.width, "{source}");
        assert_eq!(layout.ascent, without_positions.ascent, "{source}");
        assert_eq!(layout.descent, without_positions.descent, "{source}");
        for &index in indices {
            let flow = &node.children[index];
            assert!(matches!(
                flow.content.as_slice(),
                [MathContent::Math(placeholder)] if placeholder.kind == MathNodeKind::Placeholder
            ));
            let marker = layout
                .nodes
                .iter()
                .find(|candidate| candidate.id.as_ref() == Some(&flow.id))
                .unwrap_or_else(|| panic!("missing optional flow marker: {source}, {index}"));
            assert!(marker.is_flow);
            assert_eq!(marker.scope_id, root.id);
            assert_eq!(
                marker.path,
                [MathPathStep::Content(0), MathPathStep::Operand(index)]
            );
            assert_eq!(marker.width, 0.0);
            assert_eq!(marker.height, 0.0);
        }
    }
}

#[test]
fn empty_align_cells_have_visible_scoped_slots() {
    let mut align = match parse(
        r"\begin{aligned}x&y\\z&w\end{aligned}",
        MathInputKind::Align,
    ) {
        Ok(ParsedMath::Align(align)) => align,
        other => panic!("valid aligned input should parse: {other:?}"),
    };
    for row in &mut align.rows {
        for cell in &mut row.cells {
            cell.root.content.clear();
        }
    }
    let layout = layout_align(font(), &align, SIZE)
        .unwrap_or_else(|error| panic!("valid Align should lay out: {error}"));
    let mut positions = Vec::new();
    for row in &align.rows {
        for cell in &row.cells {
            let slot = layout
                .nodes
                .iter()
                .find(|node| node.id.as_ref() == Some(&cell.root.id))
                .unwrap_or_else(|| panic!("missing empty cell slot"));
            assert_eq!(slot.scope_id, cell.root.id);
            assert!(slot.path.is_empty() && slot.is_flow);
            assert_rendered_slot(&layout, slot);
            assert!(cell.root.content.is_empty());
            positions.push((slot.x, slot.y));
        }
    }
    assert_eq!(positions.len(), 4);
    assert_ne!(positions[0].0, positions[1].0);
    assert_ne!(positions[0].1, positions[2].1);
}

#[test]
fn empty_flow_inside_row_keeps_slot_with_or_without_text_shaper() {
    struct UnexpectedShaper;
    impl MathTextShaper for UnexpectedShaper {
        fn shape(&mut self, _: MathTextRequest) -> Result<MathTextShape, MathTextShapeError> {
            panic!("empty math flow has no text to shape")
        }
    }
    let flow = MathFlow {
        id: NodeId::generate(),
        content: Vec::new(),
        extra: Default::default(),
    };
    let flow_id = flow.id.clone();
    let row = MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Row,
        text: None,
        attributes: Default::default(),
        children: vec![flow],
        extra: Default::default(),
    };
    let root = MathFlow {
        id: NodeId::generate(),
        content: vec![MathContent::Math(row)],
        extra: Default::default(),
    };
    let plain = layout_math(font(), &root, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let shaped = layout_math_with_text_shaper(
        font(),
        &root,
        SIZE,
        MathStyle::Display,
        &mut UnexpectedShaper,
    )
    .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    for layout in [&plain, &shaped] {
        let slot = layout
            .nodes
            .iter()
            .find(|node| node.id.as_ref() == Some(&flow_id))
            .unwrap_or_else(|| panic!("missing empty flow slot"));
        assert_eq!(slot.scope_id, root.id);
        assert_eq!(
            slot.path,
            [MathPathStep::Content(0), MathPathStep::Operand(0)]
        );
        assert!(slot.is_flow);
        assert_rendered_slot(layout, slot);
    }
}

#[test]
fn transparent_single_glyph_row_preserves_script_and_accent_geometry() {
    for source in ["x^2", r"\hat{x}"] {
        let plain = root(source);
        let mut wrapped = plain.clone();
        let Some(MathContent::Math(parent)) = wrapped.content.first_mut() else {
            panic!("formula should begin with a math node");
        };
        let Some(base_flow) = parent.children.first_mut() else {
            panic!("parent node should have a base flow");
        };
        let Some(MathContent::Math(base)) = base_flow.content.first().cloned() else {
            panic!("base should be a math node");
        };
        let inner_row = MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Row,
            text: None,
            attributes: Default::default(),
            children: vec![MathFlow {
                id: NodeId::generate(),
                content: vec![MathContent::Math(base)],
                extra: Default::default(),
            }],
            extra: Default::default(),
        };
        let inner_row_id = inner_row.id.clone();
        let outer_row = MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Row,
            text: None,
            attributes: Default::default(),
            children: vec![MathFlow {
                id: NodeId::generate(),
                content: vec![MathContent::Math(inner_row)],
                extra: Default::default(),
            }],
            extra: Default::default(),
        };
        let outer_row_id = outer_row.id.clone();
        base_flow.content = vec![MathContent::Math(outer_row)];

        let plain_layout = layout_math(font(), &plain, SIZE, MathStyle::Display)
            .unwrap_or_else(|error| panic!("{source}: {error}"));
        let wrapped_layout = layout_math(font(), &wrapped, SIZE, MathStyle::Display)
            .unwrap_or_else(|error| panic!("{source}: {error}"));

        assert_eq!(wrapped_layout.items, plain_layout.items, "{source}");
        assert_eq!(wrapped_layout.width, plain_layout.width, "{source}");
        assert_eq!(wrapped_layout.ascent, plain_layout.ascent, "{source}");
        assert_eq!(wrapped_layout.descent, plain_layout.descent, "{source}");

        let row_box = |id: &NodeId| {
            wrapped_layout
                .nodes
                .iter()
                .find(|node| node.id.as_ref() == Some(id))
                .unwrap_or_else(|| panic!("missing transparent Row node box: {id}"))
        };
        let outer_box = row_box(&outer_row_id);
        let inner_box = row_box(&inner_row_id);
        assert!(
            outer_box.width > 0.0,
            "{source}: outer Row should cover content, got {outer_box:?}"
        );
        assert!(
            inner_box.width > 0.0,
            "{source}: inner Row should cover content"
        );
        assert!(outer_box.x <= inner_box.x + EPSILON, "{source}");
        assert!(outer_box.y <= inner_box.y + EPSILON, "{source}");
        assert!(
            outer_box.x + outer_box.width + EPSILON >= inner_box.x + inner_box.width,
            "{source}"
        );
        assert!(
            outer_box.y + outer_box.height + EPSILON >= inner_box.y + inner_box.height,
            "{source}"
        );
    }
}

#[test]
fn text_island_shaper_replaces_math_glyphs_and_keeps_run_positions() {
    struct MockTextShaper {
        next_token: u64,
    }

    impl MathTextShaper for MockTextShaper {
        fn shape(&mut self, request: MathTextRequest) -> Result<MathTextShape, MathTextShapeError> {
            let mut x = 0.0;
            let mut runs = Vec::with_capacity(request.runs.len());
            for run in &request.runs {
                let width = run.text.chars().count() as f64 * 4.0;
                runs.push(MathTextRunBox {
                    id: None,
                    scope_id: run.scope_id.clone(),
                    path: run.path.clone(),
                    run_index: run.run_index,
                    is_text_flow: run.is_text_flow,
                    is_flow: false,
                    is_container: false,
                    x,
                    y: 0.0,
                    width,
                    height: 10.0,
                    baseline: 8.0,
                });
                x += width;
            }
            let token = self.next_token;
            self.next_token += 1;
            Ok(MathTextShape {
                token,
                width: x,
                ascent: 8.0,
                descent: 2.0,
                requested_family_missing: false,
                runs,
                containers: request
                    .containers
                    .iter()
                    .map(|container| MathTextRunBox {
                        id: container.id.clone(),
                        scope_id: container.scope_id.clone(),
                        path: container.path.clone(),
                        run_index: None,
                        is_text_flow: false,
                        is_flow: container.is_flow,
                        is_container: container.is_container,
                        x: 0.0,
                        y: 0.0,
                        width: x,
                        height: 10.0,
                        baseline: 8.0,
                    })
                    .collect(),
            })
        }
    }

    let root = root(r"\text{alpha}");
    let mut shaper = MockTextShaper { next_token: 1 };
    let layout = layout_math_with_text_shaper(font(), &root, SIZE, MathStyle::Display, &mut shaper)
        .unwrap_or_else(|error| panic!("math text shaping should succeed: {error}"));

    let text_item = layout
        .items
        .iter()
        .find_map(|item| match item {
            MathItem::TextIsland {
                token,
                width,
                baseline_y,
                ..
            } => Some((*token, *width, *baseline_y)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("text island item should be published"));
    assert_eq!(text_item.0, 1);
    assert_eq!(text_item.1, 20.0);
    assert!(text_item.2.is_finite());
    assert!(
        !layout
            .items
            .iter()
            .any(|item| matches!(item, MathItem::Glyph { .. }))
    );
    let text_run = layout
        .nodes
        .iter()
        .find(|node| node.text_run_index == Some(0))
        .unwrap_or_else(|| panic!("TextFlow Run range should use shaped geometry"));
    assert_eq!(text_run.scope_id, root.id);
    assert!(text_run.width > 0.0);
}

#[test]
fn soft_break_without_text_shaper_is_rejected() {
    let root = MathFlow {
        id: NodeId::generate(),
        content: vec![MathContent::SoftBreak(caret_doc::SoftBreak::default())],
        extra: Default::default(),
    };
    let error = layout_math(font(), &root, SIZE, MathStyle::Inline)
        .err()
        .unwrap_or_else(|| panic!("SoftBreak without a text shaper must be rejected"));
    assert_eq!(error, MathLayoutError::TextShapingFailed);
}

#[test]
fn text_islands_follow_editable_flow_boundaries() {
    struct RecordingTextShaper {
        requests: Vec<MathTextRequest>,
        next_token: u64,
    }

    impl MathTextShaper for RecordingTextShaper {
        fn shape(&mut self, request: MathTextRequest) -> Result<MathTextShape, MathTextShapeError> {
            self.requests.push(request.clone());
            let mut x = 0.0;
            let runs = request
                .runs
                .iter()
                .map(|run| {
                    let width = run.text.chars().count() as f64 * 4.0;
                    let bounds = MathTextRunBox {
                        id: None,
                        scope_id: run.scope_id.clone(),
                        path: run.path.clone(),
                        run_index: run.run_index,
                        is_text_flow: run.is_text_flow,
                        is_flow: false,
                        is_container: false,
                        x,
                        y: 0.0,
                        width,
                        height: 10.0,
                        baseline: 8.0,
                    };
                    x += width;
                    bounds
                })
                .collect();
            let token = self.next_token;
            self.next_token += 1;
            Ok(MathTextShape {
                token,
                width: x,
                ascent: 8.0,
                descent: 2.0,
                requested_family_missing: false,
                runs,
                containers: request
                    .containers
                    .iter()
                    .map(|container| MathTextRunBox {
                        id: container.id.clone(),
                        scope_id: container.scope_id.clone(),
                        path: container.path.clone(),
                        run_index: None,
                        is_text_flow: false,
                        is_flow: container.is_flow,
                        is_container: container.is_container,
                        x: 0.0,
                        y: 0.0,
                        width: x,
                        height: 10.0,
                        baseline: 8.0,
                    })
                    .collect(),
            })
        }
    }

    let text = |value: &str| {
        MathContent::Text(TextFlow {
            runs: vec![Run {
                text: value.to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }],
            extra: Default::default(),
        })
    };
    let bold_id = NodeId::generate();
    let nested_flow = MathFlow {
        id: NodeId::generate(),
        content: vec![
            text("a"),
            MathContent::Environment(InlineEnvironment {
                id: bold_id.clone(),
                kind: InlineEnvironmentKind::Bold,
                definition_id: None,
                props: Default::default(),
                children: vec![
                    text("\u{301}"),
                    MathContent::Math(MathNode {
                        id: NodeId::generate(),
                        kind: MathNodeKind::Symbol,
                        text: Some("x".to_owned()),
                        attributes: Default::default(),
                        children: Vec::new(),
                        extra: Default::default(),
                    }),
                    text("z"),
                ],
                extra: Default::default(),
            }),
            text("c"),
            MathContent::SoftBreak(caret_doc::SoftBreak::default()),
            text("d"),
            MathContent::Opaque(caret_doc::OpaqueNode {
                raw: serde_json::json!({"type": "future", "payload": 1}),
            }),
            text("e"),
            MathContent::Math(MathNode {
                id: NodeId::generate(),
                kind: MathNodeKind::Fraction,
                text: None,
                attributes: Default::default(),
                children: vec![
                    MathFlow {
                        id: NodeId::generate(),
                        content: vec![text("u")],
                        extra: Default::default(),
                    },
                    MathFlow {
                        id: NodeId::generate(),
                        content: vec![text("v")],
                        extra: Default::default(),
                    },
                ],
                extra: Default::default(),
            }),
            MathContent::Text(TextFlow {
                runs: Vec::new(),
                extra: Default::default(),
            }),
        ],
        extra: Default::default(),
    };
    let root = MathFlow {
        id: NodeId::generate(),
        content: vec![MathContent::Math(MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Row,
            text: None,
            attributes: Default::default(),
            children: vec![nested_flow],
            extra: Default::default(),
        })],
        extra: Default::default(),
    };
    let mut shaper = RecordingTextShaper {
        requests: Vec::new(),
        next_token: 0,
    };

    layout_math_with_text_shaper(font(), &root, SIZE, MathStyle::Inline, &mut shaper)
        .unwrap_or_else(|error| panic!("valid text islands should shape: {error}"));
    let request_texts: Vec<String> = shaper
        .requests
        .iter()
        .map(|request| request.runs.iter().map(|run| run.text.as_str()).collect())
        .collect();
    assert_eq!(request_texts, ["a\u{301}", "zc\nd", "�", "e", "u", "v", ""]);
    assert_eq!(shaper.requests[0].runs.len(), 2);
    assert_eq!(shaper.requests[0].runs[0].style.bold, None);
    assert_eq!(shaper.requests[0].runs[1].style.bold, Some(true));
    assert!(
        shaper.requests[0]
            .containers
            .iter()
            .any(|container| container.id.as_ref() == Some(&bold_id))
    );
    let soft_break = &shaper.requests[1].runs[2];
    assert_eq!(soft_break.text, "\n");
    assert_eq!(
        soft_break.path,
        vec![
            MathPathStep::Content(0),
            MathPathStep::Operand(0),
            MathPathStep::Content(3)
        ]
    );
    assert!(!soft_break.is_text_flow);
    assert!(shaper.requests[6].runs[0].text.is_empty());
    assert!(shaper.requests[6].runs[0].is_text_flow);
    assert_eq!(shaper.requests[6].runs[0].run_index, None);

    let space_root = MathFlow {
        id: NodeId::generate(),
        content: vec![text("m"), text("n")],
        extra: Default::default(),
    };
    let mut backend_nodes = crate::backend::flow_nodes_for_layout(&space_root);
    backend_nodes[0].children.insert(
        1,
        crate::backend::Node {
            id: None,
            position: Some(crate::backend::NodePosition {
                scope_id: space_root.id.clone(),
                path: vec![MathPathStep::Content(1)],
                text_run_index: None,
                is_text_flow: true,
                is_flow: false,
                is_container: false,
                font_size_pt: None,
            }),
            kind: crate::backend::BackendNodeKind::TextSpace,
            text: None,
            run: None,
            attributes: Default::default(),
            children: Vec::new(),
        },
    );
    super::layout_nodes(
        font(),
        backend_nodes,
        SIZE,
        MathStyle::Inline,
        Some(&mut shaper),
    )
    .unwrap_or_else(|error| panic!("TextSpace should use body shaping: {error}"));
    let text_space_request = shaper
        .requests
        .last()
        .unwrap_or_else(|| panic!("TextSpace should produce a body shaping request"));
    let text_space: String = text_space_request
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect();
    assert_eq!(text_space, "m n");
    assert_eq!(text_space_request.runs.len(), 3);
}

#[test]
fn flow_and_text_style_boxes_are_unique_and_cover_math_islands() {
    #[derive(Default)]
    struct RecordingShaper {
        requests: Vec<MathTextRequest>,
        next_token: u64,
    }

    impl MathTextShaper for RecordingShaper {
        fn shape(&mut self, request: MathTextRequest) -> Result<MathTextShape, MathTextShapeError> {
            self.requests.push(request.clone());
            let mut x = 0.0;
            let runs = request
                .runs
                .iter()
                .map(|run| {
                    let width = run.text.chars().count() as f64 * 4.0;
                    let bounds = MathTextRunBox {
                        id: None,
                        scope_id: run.scope_id.clone(),
                        path: run.path.clone(),
                        run_index: run.run_index,
                        is_text_flow: run.is_text_flow,
                        is_flow: false,
                        is_container: false,
                        x,
                        y: 0.0,
                        width,
                        height: 10.0,
                        baseline: 8.0,
                    };
                    x += width;
                    bounds
                })
                .collect();
            let containers = request
                .containers
                .iter()
                .map(|container| MathTextRunBox {
                    id: container.id.clone(),
                    scope_id: container.scope_id.clone(),
                    path: container.path.clone(),
                    run_index: None,
                    is_text_flow: false,
                    is_flow: container.is_flow,
                    is_container: container.is_container,
                    x: 0.0,
                    y: 0.0,
                    width: x,
                    height: 10.0,
                    baseline: 8.0,
                })
                .collect();
            let token = self.next_token;
            self.next_token += 1;
            Ok(MathTextShape {
                token,
                width: x,
                ascent: 8.0,
                descent: 2.0,
                requested_family_missing: false,
                runs,
                containers,
            })
        }
    }

    let text = |value: &str| {
        MathContent::Text(TextFlow {
            runs: vec![Run {
                text: value.to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }],
            extra: Default::default(),
        })
    };
    let math_symbol = || {
        MathContent::Math(MathNode {
            id: NodeId::generate(),
            kind: MathNodeKind::Symbol,
            text: Some("x".to_owned()),
            attributes: Default::default(),
            children: Vec::new(),
            extra: Default::default(),
        })
    };
    let environment = |id, kind, props, children| {
        MathContent::Environment(InlineEnvironment {
            id,
            kind,
            definition_id: None,
            props,
            children,
            extra: Default::default(),
        })
    };
    let find_container = |layout: &MathLayout, id: &NodeId| -> MathNodeBox {
        let matches: Vec<_> = layout
            .nodes
            .iter()
            .filter(|node| node.id.as_ref() == Some(id) && node.is_container)
            .cloned()
            .collect();
        assert_eq!(matches.len(), 1, "container {id} must have one box");
        matches[0].clone()
    };
    let assert_covers_descendants = |layout: &MathLayout, container: &MathNodeBox| {
        let descendants: Vec<_> = layout
            .nodes
            .iter()
            .filter(|node| {
                node.scope_id == container.scope_id
                    && node.path.len() > container.path.len()
                    && node.path.starts_with(&container.path)
            })
            .collect();
        assert!(!descendants.is_empty(), "container has no descendant boxes");
        for child in descendants {
            assert!(child.x >= container.x - EPSILON);
            assert!(child.y >= container.y - EPSILON);
            assert!(child.x + child.width <= container.x + container.width + EPSILON);
            assert!(child.y + child.height <= container.y + container.height + EPSILON);
        }
    };

    let pure_math_environment_id = NodeId::generate();
    let pure_math = MathFlow {
        id: NodeId::generate(),
        content: vec![environment(
            pure_math_environment_id.clone(),
            InlineEnvironmentKind::Bold,
            Default::default(),
            vec![math_symbol()],
        )],
        extra: Default::default(),
    };
    let mut shaper = RecordingShaper::default();
    let pure_layout =
        layout_math_with_text_shaper(font(), &pure_math, SIZE, MathStyle::Inline, &mut shaper)
            .unwrap_or_else(|error| panic!("pure math environment should lay out: {error}"));
    assert_covers_descendants(
        &pure_layout,
        &find_container(&pure_layout, &pure_math_environment_id),
    );

    let font_id = NodeId::generate();
    let italic_id = NodeId::generate();
    let bold_id = NodeId::generate();
    let numerator_flow_id = NodeId::generate();
    let denominator_flow_id = NodeId::generate();
    let numerator = MathFlow {
        id: numerator_flow_id.clone(),
        content: vec![environment(
            NodeId::generate(),
            InlineEnvironmentKind::Italic,
            BTreeMap::from([("enabled".to_owned(), serde_json::json!(false))]),
            vec![text("x")],
        )],
        extra: Default::default(),
    };
    let denominator = MathFlow {
        id: denominator_flow_id.clone(),
        content: vec![environment(
            NodeId::generate(),
            InlineEnvironmentKind::Bold,
            BTreeMap::from([("enabled".to_owned(), serde_json::json!(false))]),
            vec![text("y")],
        )],
        extra: Default::default(),
    };
    let fraction = MathContent::Math(MathNode {
        id: NodeId::generate(),
        kind: MathNodeKind::Fraction,
        text: None,
        attributes: Default::default(),
        children: vec![numerator, denominator],
        extra: Default::default(),
    });
    let root = MathFlow {
        id: NodeId::generate(),
        content: vec![environment(
            font_id.clone(),
            InlineEnvironmentKind::Font,
            BTreeMap::from([("fontFamily".to_owned(), serde_json::json!("sans-serif"))]),
            vec![environment(
                italic_id.clone(),
                InlineEnvironmentKind::Italic,
                Default::default(),
                vec![environment(
                    bold_id.clone(),
                    InlineEnvironmentKind::Bold,
                    Default::default(),
                    vec![text("a"), fraction, text("b")],
                )],
            )],
        )],
        extra: Default::default(),
    };
    let first_request = shaper.requests.len();
    let layout = layout_math_with_text_shaper(font(), &root, SIZE, MathStyle::Inline, &mut shaper)
        .unwrap_or_else(|error| panic!("mixed math environments should lay out: {error}"));
    assert_eq!(
        shaper.requests[first_request..]
            .iter()
            .map(|request| request
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>())
            .collect::<Vec<_>>(),
        ["a", "x", "y", "b"]
    );
    let outer = &shaper.requests[first_request].runs[0].style;
    assert_eq!(outer.bold, Some(true));
    assert_eq!(outer.italic, Some(true));
    assert_eq!(outer.font_family.as_deref(), Some("sans-serif"));
    let numerator_style = &shaper.requests[first_request + 1].runs[0].style;
    assert_eq!(numerator_style.bold, Some(true));
    assert_eq!(numerator_style.italic, Some(false));
    assert_eq!(numerator_style.font_family.as_deref(), Some("sans-serif"));
    let denominator_style = &shaper.requests[first_request + 2].runs[0].style;
    assert_eq!(denominator_style.bold, Some(false));
    assert_eq!(denominator_style.italic, Some(true));
    assert_eq!(denominator_style.font_family.as_deref(), Some("sans-serif"));
    for id in [&font_id, &italic_id, &bold_id] {
        assert_covers_descendants(&layout, &find_container(&layout, id));
    }
    let numerator_flow = layout
        .nodes
        .iter()
        .filter(|node| node.id.as_ref() == Some(&numerator_flow_id) && node.is_flow)
        .count();
    let denominator_flow = layout
        .nodes
        .iter()
        .filter(|node| node.id.as_ref() == Some(&denominator_flow_id) && node.is_flow)
        .count();
    assert_eq!(numerator_flow, 1);
    assert_eq!(denominator_flow, 1);
}

#[test]
fn coalescing_container_boxes_preserves_distinct_paths_and_first_occurrence_order() {
    let id = NodeId::generate();
    let scope_id = NodeId::generate();
    let make_box = |path, x, width| MathNodeBox {
        id: Some(id.clone()),
        scope_id: scope_id.clone(),
        path,
        text_run_index: None,
        is_text_flow: false,
        is_flow: false,
        is_container: true,
        x,
        y: 0.0,
        width,
        height: 10.0,
        baseline: 8.0,
        font_size_pt: None,
    };
    let mut nodes = vec![
        make_box(vec![MathPathStep::Content(0)], 0.0, 1.0),
        make_box(vec![MathPathStep::Content(1)], 5.0, 1.0),
        make_box(vec![MathPathStep::Content(0)], 2.0, 1.0),
    ];
    super::coalesce_container_boxes(&mut nodes);
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].path, [MathPathStep::Content(0)]);
    assert_eq!(nodes[1].path, [MathPathStep::Content(1)]);
    assert_eq!(nodes[0].x, 0.0);
    assert_eq!(nodes[0].width, 3.0);
}

#[test]
fn math_alphabet_keeps_its_variant_when_text_islands_are_shaped() {
    struct EmptyTextShaper(u64);

    impl MathTextShaper for EmptyTextShaper {
        fn shape(&mut self, request: MathTextRequest) -> Result<MathTextShape, MathTextShapeError> {
            Ok(MathTextShape {
                token: self.0,
                width: 0.0,
                ascent: 8.0,
                descent: 2.0,
                requested_family_missing: false,
                runs: request
                    .runs
                    .iter()
                    .map(|run| MathTextRunBox {
                        id: None,
                        scope_id: run.scope_id.clone(),
                        path: run.path.clone(),
                        run_index: run.run_index,
                        is_text_flow: run.is_text_flow,
                        is_flow: false,
                        is_container: false,
                        x: 0.0,
                        y: 0.0,
                        width: 0.0,
                        height: 10.0,
                        baseline: 8.0,
                    })
                    .collect(),
                containers: request
                    .containers
                    .iter()
                    .map(|container| MathTextRunBox {
                        id: container.id.clone(),
                        scope_id: container.scope_id.clone(),
                        path: container.path.clone(),
                        run_index: None,
                        is_text_flow: false,
                        is_flow: container.is_flow,
                        is_container: container.is_container,
                        x: 0.0,
                        y: 0.0,
                        width: 0.0,
                        height: 10.0,
                        baseline: 8.0,
                    })
                    .collect(),
            })
            .inspect(|_shape| {
                self.0 += 1;
            })
        }
    }

    let source = r"\mathbb{R}+\text{x}";
    let without_text_shaper = lay(source, MathStyle::Inline);
    let with_text_island = root(source);
    let mut shaper = EmptyTextShaper(1);
    let shaped = layout_math_with_text_shaper(
        font(),
        &with_text_island,
        SIZE,
        MathStyle::Inline,
        &mut shaper,
    )
    .unwrap_or_else(|error| panic!("mixed math and text should lay out: {error}"));

    assert_eq!(
        glyph_ids(&shaped)[..2],
        glyph_ids(&without_text_shaper)[..2],
        "the MathAlphabet and following operator should keep their math variants"
    );
}

#[test]
fn text_shaper_cannot_return_flow_bounds_for_a_run() {
    struct InvalidFlowRunShaper;

    impl MathTextShaper for InvalidFlowRunShaper {
        fn shape(&mut self, request: MathTextRequest) -> Result<MathTextShape, MathTextShapeError> {
            let runs = request
                .runs
                .iter()
                .map(|run| MathTextRunBox {
                    id: None,
                    scope_id: run.scope_id.clone(),
                    path: run.path.clone(),
                    run_index: run.run_index,
                    is_text_flow: run.is_text_flow,
                    is_flow: true,
                    is_container: false,
                    x: 0.0,
                    y: 0.0,
                    width: 4.0,
                    height: 10.0,
                    baseline: 8.0,
                })
                .collect();
            Ok(MathTextShape {
                token: 1,
                width: 4.0,
                ascent: 8.0,
                descent: 2.0,
                requested_family_missing: false,
                runs,
                containers: Vec::new(),
            })
        }
    }

    let mut shaper = InvalidFlowRunShaper;
    assert_eq!(
        layout_math_with_text_shaper(
            font(),
            &root(r"\text{x}"),
            SIZE,
            MathStyle::Inline,
            &mut shaper,
        ),
        Err(MathLayoutError::TextShapingFailed)
    );
}

#[test]
fn text_island_shaping_failure_rejects_the_complete_math_layout() {
    struct FailedTextShaper;
    impl MathTextShaper for FailedTextShaper {
        fn shape(
            &mut self,
            _request: MathTextRequest,
        ) -> Result<MathTextShape, MathTextShapeError> {
            Err(MathTextShapeError::Failed)
        }
    }

    let root = root(r"\text{cannot fall back to math glyphs}");
    let mut shaper = FailedTextShaper;
    assert_eq!(
        layout_math_with_text_shaper(font(), &root, SIZE, MathStyle::Display, &mut shaper),
        Err(super::MathLayoutError::TextShapingFailed)
    );
}

#[test]
fn colors_propagate() {
    let layout = display(r"\color{red} x");
    assert!(layout.items.iter().any(|item| matches!(
        item,
        MathItem::Glyph {
            color: Some(color),
            ..
        } if color.red == 0xff && color.green == 0
    )));
}

#[test]
fn color_and_text_color_preserve_child_atom_spacing() {
    let ParsedMath::Tree(mut color_tree) = parse(r"\textcolor{red}{a+b}", MathInputKind::Equation)
        .unwrap_or_else(|error| panic!("colored expression should parse: {error}"))
    else {
        panic!("expected an equation tree")
    };
    let color = color_tree
        .root
        .content
        .first_mut()
        .and_then(|content| match content {
            MathContent::Math(node) => Some(node),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected a color node"));
    color.kind = caret_doc::MathNodeKind::Color;
    let text_color_layout = layout_math(
        font(),
        &root(r"\textcolor{red}{a+b}"),
        SIZE,
        MathStyle::Display,
    )
    .unwrap_or_else(|error| panic!("TextColor layout should succeed: {error}"));
    let color_layout = layout_math(font(), &color_tree.root, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("Color layout should succeed: {error}"));

    assert_eq!(color_layout.items, text_color_layout.items);
    assert_eq!(color_layout.width, text_color_layout.width);
}

#[test]
fn cancel_draws_lines() {
    let layout = display(r"\xcancel{x}");
    let lines = layout
        .items
        .iter()
        .filter(|item| matches!(item, MathItem::Line { .. }))
        .count();
    assert_eq!(lines, 2);
}

#[test]
fn layout_scales_with_size() {
    let tree = root(r"\frac{x^2}{y}");
    let small = layout_math(font(), &tree, 10.0, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let large = layout_math(font(), &tree, 20.0, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    assert!((large.width - 2.0 * small.width).abs() < 1e-3);
    assert!((large.ascent - 2.0 * small.ascent).abs() < 1e-3);
}

#[test]
fn engine_caches_per_key() {
    let engine = MathLayoutEngine::new().unwrap_or_else(|error| panic!("{error}"));
    let tree = root("x+1");
    let first = engine
        .layout(&tree, SIZE, MathStyle::Inline)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let second = engine
        .layout(&tree, SIZE, MathStyle::Inline)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(engine.cached_len(), 1);
    let _display = engine
        .layout(&tree, SIZE, MathStyle::Display)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    let _larger = engine
        .layout(&tree, SIZE * 2.0, MathStyle::Inline)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    assert_eq!(engine.cached_len(), 3);
    let other = root("x+2");
    let _other = engine
        .layout(&other, SIZE, MathStyle::Inline)
        .unwrap_or_else(|error| panic!("布局输入应合法：{error}"));
    assert_eq!(engine.cached_len(), 4);
    engine.clear_cache();
    assert_eq!(engine.cached_len(), 0);
}

#[test]
fn layout_rejects_invalid_sizes_without_caching_them() {
    let engine = MathLayoutEngine::new().unwrap_or_else(|error| panic!("{error}"));
    let tree = root("x");
    for size in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.0,
        -1.0,
        655.36,
    ] {
        assert_eq!(
            engine.layout(&tree, size, MathStyle::Inline),
            Err(super::MathLayoutError::InvalidSize)
        );
    }
    assert_eq!(engine.cached_len(), 0);
    assert!(engine.layout(&tree, 0.01, MathStyle::Inline).is_ok());
}

#[test]
fn placeholder_renders_box() {
    let layout = display(r"\frac{}{}");
    let boxes = layout
        .items
        .iter()
        .filter(|item| matches!(item, MathItem::Rectangle { .. }))
        .count();
    assert_eq!(boxes, 2);
}

#[test]
fn align_layout_uses_document_rows_and_cell_roots() {
    let align = match parse(
        r"\begin{aligned}x&=1\\y&=2\end{aligned}",
        MathInputKind::Align,
    ) {
        Ok(ParsedMath::Align(align)) => align,
        other => panic!("Align source should produce an Align document node: {other:?}"),
    };
    let expected_ids = std::iter::once(align.id.clone())
        .chain(align.rows.iter().flat_map(|row| {
            std::iter::once(row.id.clone()).chain(row.cells.iter().map(|cell| cell.root.id.clone()))
        }))
        .collect::<Vec<NodeId>>();
    let layout = layout_align(font(), &align, SIZE)
        .unwrap_or_else(|error| panic!("valid Align should lay out: {error}"));

    assert!(layout.width > 0.0);
    assert!(layout.ascent > 0.0);
    assert!(
        layout
            .items
            .iter()
            .any(|item| matches!(item, MathItem::Glyph { .. }))
    );
    for id in expected_ids {
        let count = layout
            .nodes
            .iter()
            .filter(|node| node.id.as_ref() == Some(&id))
            .count();
        assert_eq!(count, 1, "expected one node box for {id}");
    }
}
