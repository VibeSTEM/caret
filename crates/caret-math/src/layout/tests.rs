//! 公式布局的行为测试：几何关系、覆盖语料、缓存。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::sync::Arc;

use caret_doc::MathNode;

use super::{MathItem, MathLayout, MathLayoutEngine, MathStyle, layout_math};
use crate::layout::MathFont;
use crate::{MathInputKind, ParsedMath, parse};

const EPSILON: f64 = 1e-6;
const SIZE: f64 = 10.0;

fn font() -> &'static MathFont {
    MathFont::builtin().unwrap_or_else(|error| panic!("内置字体加载失败：{error}"))
}

fn root(source: &str) -> MathNode {
    match parse(source, MathInputKind::Equation) {
        Ok(ParsedMath::Tree(tree)) => tree.root,
        other => panic!("{source}: {other:?}"),
    }
}

fn lay(source: &str, style: MathStyle) -> MathLayout {
    layout_math(font(), &root(source), SIZE, style)
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
fn node_boxes_cover_children() {
    let tree = root(r"\frac{a}{b}");
    let layout = layout_math(font(), &tree, SIZE, MathStyle::Display);
    let first = layout
        .nodes
        .first()
        .unwrap_or_else(|| panic!("应记录根节点"));
    assert_eq!(first.id, tree.id);
    assert!((first.width - layout.width).abs() < EPSILON);
    for node in &layout.nodes {
        assert!(node.x >= -EPSILON && node.x + node.width <= layout.width + EPSILON);
    }
    assert!(layout.nodes.len() >= 3);
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
    let small = layout_math(font(), &tree, 10.0, MathStyle::Display);
    let large = layout_math(font(), &tree, 20.0, MathStyle::Display);
    assert!((large.width - 2.0 * small.width).abs() < 1e-3);
    assert!((large.ascent - 2.0 * small.ascent).abs() < 1e-3);
}

#[test]
fn engine_caches_per_key() {
    let engine = MathLayoutEngine::new().unwrap_or_else(|error| panic!("{error}"));
    let tree = root("x+1");
    let first = engine.layout(&tree, SIZE, MathStyle::Inline);
    let second = engine.layout(&tree, SIZE, MathStyle::Inline);
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(engine.cached_len(), 1);
    let _display = engine.layout(&tree, SIZE, MathStyle::Display);
    let _larger = engine.layout(&tree, SIZE * 2.0, MathStyle::Inline);
    assert_eq!(engine.cached_len(), 3);
    let other = root("x+2");
    let _other = engine.layout(&other, SIZE, MathStyle::Inline);
    assert_eq!(engine.cached_len(), 4);
    engine.clear_cache();
    assert_eq!(engine.cached_len(), 0);
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
