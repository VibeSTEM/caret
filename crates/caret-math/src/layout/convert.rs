//! 文档数学树到排版片段的转换：样式继承、原子类别、各节点类别的排版调度。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use read_fonts::tables::math::MathConstant as C;

use super::accent::{CancelKind, layout_accent, layout_cancel, layout_cancel_to, layout_line};
use super::alphabet::styled_char;
use super::context::{Context, MathSize, Style, Variant};
use super::fenced::{balanced_height, delimiter};
use super::fraction::layout_fraction;
use super::frame::{Fragment, Frame, Item, MathColor, Point};
use super::glyph::{center_on_axis, glyph_fragment, stretch};
use super::radical::layout_radical;
use super::run::layout_row;
use super::scripts::{Attachments, layout_attachments};
use super::table::{Align, TableRow, TableSpec, layout_table};
use crate::backend::BackendNodeKind as K;
use crate::backend::Node as MathNode;
use crate::fonts::Axis;
use crate::symbols::{AtomClass, accent_char, delimiter_char, literal_display_char, named_symbol};

/// 占位框颜色。
const PLACEHOLDER_COLOR: MathColor = MathColor {
    red: 0x9e,
    green: 0x9e,
    blue: 0x9e,
    alpha: 0xff,
};
/// `\boxed`、`\colorbox` 的内边距（pt）。
const BOX_PADDING: f64 = 3.0;
/// `\boxed`、`\fcolorbox` 的边框粗细（pt）。
const BOX_STROKE: f64 = 0.4;

pub(crate) struct Converter<'a> {
    pub ctx: Context<'a>,
}

impl Converter<'_> {
    /// 排版一个节点，并记录其范围供命中测试。
    pub(crate) fn node(&mut self, node: &MathNode, style: Style) -> Fragment {
        let mut fragment = self.layout(node, style);
        let frame = &fragment.frame;
        let record = Item::Node {
            id: node.id.clone(),
            width: frame.width,
            height: frame.height,
            baseline: frame.baseline,
        };
        fragment
            .frame
            .items
            .insert(0, (Point::new(0.0, 0.0), record));
        fragment
    }

    /// 把子节点序列排成一行；嵌套 Row 与 `\color` 展开到同一行参与间距计算。
    pub(crate) fn row(&mut self, children: &[MathNode], style: Style) -> Fragment {
        let mut fragments = Vec::with_capacity(children.len());
        self.collect(children, style, &mut fragments);
        if fragments.is_empty() {
            return Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord);
        }
        layout_row(&self.ctx, style, fragments)
    }

    fn collect(&mut self, children: &[MathNode], style: Style, out: &mut Vec<Fragment>) {
        for child in children {
            match child.kind {
                K::Row => self.collect(&child.children, style, out),
                K::Hline | K::Intertext => {}
                _ => out.push(self.node(child, style)),
            }
        }
    }

    fn child(&mut self, node: &MathNode, index: usize, style: Style) -> Fragment {
        match node.children.get(index) {
            Some(child) => self.node(child, style),
            None => self.placeholder(style),
        }
    }

    /// 可缺省的子项：不存在或为占位节点时返回 `None`。
    fn optional(&mut self, node: &MathNode, index: usize, style: Style) -> Option<Fragment> {
        node.children
            .get(index)
            .filter(|child| child.kind != K::Placeholder)
            .map(|child| self.node(child, style))
    }

    fn attribute<'n>(node: &'n MathNode, key: &str) -> &'n str {
        node.attributes.get(key).map_or("", String::as_str)
    }

    fn layout(&mut self, node: &MathNode, style: Style) -> Fragment {
        match node.kind {
            K::Row => self.row(&node.children, style),
            K::Group => {
                let mut group = self.row(&node.children, style);
                group.class = AtomClass::Ord;
                group
            }
            K::Style => {
                let size = match Self::attribute(node, "style") {
                    "display" => Some(MathSize::Display),
                    "text" => Some(MathSize::Text),
                    "script" => Some(MathSize::Script),
                    "scriptscript" => Some(MathSize::ScriptScript),
                    _ => None,
                };
                let style = size.map_or(style, |size| style.with_size(size));
                self.row(&node.children, style)
            }
            K::Placeholder => self.placeholder(style),
            K::Symbol => self.symbol(node, style),
            K::Operator => self.operator(node, style),
            K::Fraction => self.fraction(node, style),
            K::Radical => {
                let index = self.optional(node, 0, style.with_size(MathSize::ScriptScript));
                let body = self.child(node, 1, style.cramped());
                layout_radical(&self.ctx, style, body, index)
            }
            K::Superscript | K::Subscript | K::SubSup => self.scripts(node, style),
            K::Sum | K::Product | K::Integral | K::Limit => self.big_operator(node, style),
            K::Delimited => self.delimited(node, style),
            K::SizedDelimiter => self.sized_delimiter(node, style),
            K::Matrix => self.matrix(node, style),
            K::Substack => {
                let rows = node
                    .children
                    .iter()
                    .map(|line| TableRow::Cells(vec![self.node(line, style)]))
                    .collect();
                let spec = TableSpec {
                    aligns: vec![Align::Center],
                    vlines: Vec::new(),
                    hlines: Vec::new(),
                    column_gap: 0.0,
                    row_gap: 0.0,
                };
                layout_table(&self.ctx, style, rows, &spec)
            }
            K::OverUnder => {
                let mut base = self.child(node, 0, style);
                let top = self.optional(node, 1, style.superscript());
                let bottom = self.optional(node, 2, style.subscript());
                let class = base.class;
                base.limits = true;
                let mut fragment = layout_attachments(
                    &self.ctx,
                    style,
                    base,
                    false,
                    Attachments {
                        top,
                        bottom,
                        ..Attachments::default()
                    },
                );
                fragment.class = class;
                fragment
            }
            K::CancelTo => {
                let target = self.child(node, 0, style.superscript());
                let body = self.child(node, 1, style);
                layout_cancel_to(style, body, target)
            }
            K::Accent => self.accent(node, style),
            K::Space => self.space(node, style),
            K::Text => self.text(node, style),
            K::TextContent | K::TextSpace | K::TextSymbol | K::TextStyle => {
                let text_style = Style {
                    text_mode: true,
                    variant: Variant::Roman,
                    ..style
                };
                self.text_item(node, text_style)
            }
            K::Color | K::TextColor => {
                let color = MathColor::parse_hex(Self::attribute(node, "color"));
                let style = Style {
                    color: color.or(style.color),
                    ..style
                };
                self.row(&node.children, style)
            }
            K::ColorBox | K::FColorBox => self.color_box(node, style),
            K::Strut => {
                let em = self.ctx.font_size(style);
                let value = |prefix: &str| {
                    let read = |suffix: &str, default: f64| {
                        node.attributes
                            .get(&format!("{prefix}{suffix}"))
                            .and_then(|value| value.parse::<f64>().ok())
                            .unwrap_or(default)
                    };
                    let denominator = read("Denominator", 1.0);
                    if super::is_zero(denominator) {
                        0.0
                    } else {
                        read("Numerator", 0.0) / denominator * em
                    }
                };
                let (height, depth) = (value("height"), value("depth"));
                Fragment::new(Frame::new(0.0, height + depth, height), AtomClass::Ord)
            }
            K::Phantom => {
                let mut body = self.child(node, 0, style);
                body.frame.hide();
                match Self::attribute(node, "phantomKind") {
                    "hphantom" => {
                        body.frame.height = 0.0;
                        body.frame.baseline = 0.0;
                    }
                    "vphantom" => body.frame.width = 0.0,
                    _ => {}
                }
                body.glyph = None;
                body.class = AtomClass::Ord;
                body
            }
            K::Hline | K::Intertext => Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord),
        }
    }

    // ---- 基本字形 ----

    /// 字符片段；缺字时记录诊断并画缺字框。
    fn character(&mut self, character: char, class: AtomClass, style: Style) -> Fragment {
        let shown = if style.text_mode && style.variant == Variant::Roman {
            character
        } else {
            styled_char(character, style.variant)
        };
        let found = glyph_fragment(&self.ctx, style, shown, class)
            .or_else(|| glyph_fragment(&self.ctx, style, character, class));
        match found {
            Some(fragment) => fragment,
            None => {
                self.ctx
                    .diagnostic("math.glyph.missing", format!("U+{:04X}", character as u32));
                self.tofu(class, style)
            }
        }
    }

    fn tofu(&self, class: AtomClass, style: Style) -> Fragment {
        let em = self.ctx.font_size(style);
        let (width, height) = (0.5 * em, 0.7 * em);
        let mut frame = Frame::new(width, height, height);
        frame.push(
            Point::new(0.0, 0.0),
            Item::Rect {
                width,
                height,
                fill: None,
                stroke: Some((style.color, 0.05 * em)),
            },
        );
        Fragment::new(frame, class)
    }

    fn placeholder(&self, style: Style) -> Fragment {
        let em = self.ctx.font_size(style);
        let (width, height) = (0.6 * em, 0.7 * em);
        let mut frame = Frame::new(width, height, height);
        frame.push(
            Point::new(0.0, 0.0),
            Item::Rect {
                width,
                height,
                fill: None,
                stroke: Some((Some(PLACEHOLDER_COLOR), 0.05 * em)),
            },
        );
        Fragment::new(frame, AtomClass::Ord)
    }

    fn symbol(&mut self, node: &MathNode, style: Style) -> Fragment {
        let text = node.text.as_deref().unwrap_or_default();
        if let Some(class) = node.attributes.get("atomClass") {
            let class = AtomClass::parse(class).unwrap_or(AtomClass::Ord);
            let fragments: Vec<Fragment> = text
                .chars()
                .map(|character| self.character(literal_display_char(character), class, style))
                .collect();
            return self.joined(fragments, class, style);
        }
        match named_symbol(text) {
            Some(symbol) => self.character(symbol.character, symbol.class, style),
            None => {
                self.ctx.diagnostic("math.symbol.unknown", text);
                self.tofu(AtomClass::Ord, style)
            }
        }
    }

    /// 多个字符合成一个原子（无原子间距）。
    fn joined(&self, mut fragments: Vec<Fragment>, class: AtomClass, style: Style) -> Fragment {
        if fragments.len() == 1
            && let Some(mut single) = fragments.pop()
        {
            single.class = class;
            return single;
        }
        for fragment in &mut fragments {
            fragment.class = AtomClass::Ord;
        }
        let mut joined = layout_row(&self.ctx, style, fragments);
        joined.class = class;
        joined
    }

    fn upright_word(&mut self, text: &str, class: AtomClass, style: Style) -> Fragment {
        let upright = Style {
            variant: Variant::Roman,
            ..style
        };
        let fragments = text
            .chars()
            .map(|character| self.character(character, AtomClass::Ord, upright))
            .collect();
        self.joined(fragments, class, style)
    }

    fn operator(&mut self, node: &MathNode, style: Style) -> Fragment {
        let text = node.text.as_deref().unwrap_or_default();
        let mut fragment = self.upright_word(text, AtomClass::Op, style);
        fragment.limits = Self::attribute(node, "limits") == "true";
        fragment.large = true;
        fragment.text_like = true;
        fragment
    }

    // ---- 结构 ----

    fn fraction(&mut self, node: &MathNode, style: Style) -> Fragment {
        let kind = Self::attribute(node, "style");
        let inner = match kind {
            "dfrac" | "cfrac" => style.with_size(MathSize::Display),
            "tfrac" => style.with_size(MathSize::Text),
            _ => style,
        };
        let num = self.child(node, 0, inner.numerator());
        let denom = self.child(node, 1, inner.denominator());
        let padding = 0.1 * self.ctx.font_size(inner);
        let mut fragment = layout_fraction(&self.ctx, inner, num, denom, true, padding);
        fragment.class = AtomClass::Inner;
        fragment
    }

    fn scripts(&mut self, node: &MathNode, style: Style) -> Fragment {
        let base = self.child(node, 0, style);
        let (sub, sup) = match node.kind {
            K::Superscript => (None, node.children.get(1)),
            K::Subscript => (node.children.get(1), None),
            _ => (node.children.get(1), node.children.get(2)),
        };
        let sup = sup.map(|child| self.node(child, style.superscript()));
        let sub = sub.map(|child| self.node(child, style.subscript()));
        self.attach(style, base, sup, sub)
    }

    /// 按基底决定放成上下限还是右上右下脚标。
    fn attach(
        &mut self,
        style: Style,
        base: Fragment,
        sup: Option<Fragment>,
        sub: Option<Fragment>,
    ) -> Fragment {
        let display = style.size == MathSize::Display;
        let limits = if base.large {
            base.limits && display
        } else {
            base.limits
        };
        let stretchy = base.stretchy_x;
        let class = base.class;
        let mut fragment = if limits {
            layout_attachments(
                &self.ctx,
                style,
                base,
                stretchy,
                Attachments {
                    top: sup,
                    bottom: sub,
                    ..Attachments::default()
                },
            )
        } else {
            layout_attachments(
                &self.ctx,
                style,
                base,
                false,
                Attachments {
                    sup,
                    sub,
                    ..Attachments::default()
                },
            )
        };
        fragment.class = class;
        fragment
    }

    fn big_operator(&mut self, node: &MathNode, style: Style) -> Fragment {
        let display = style.size == MathSize::Display;
        let (base, limits) = match node.kind {
            K::Limit => {
                let mut base = self.upright_word("lim", AtomClass::Op, style);
                base.large = true;
                (base, true)
            }
            _ => {
                let character = match node.kind {
                    K::Sum => '∑',
                    K::Product => '∏',
                    _ => match Self::attribute(node, "integralKind") {
                        "iint" => '∬',
                        "iiint" => '∭',
                        "oint" => '∮',
                        "oiint" => '∯',
                        _ => '∫',
                    },
                };
                let mut glyph = self.character(character, AtomClass::Op, style);
                if display {
                    let min_height = self.ctx.styled(C::DisplayOperatorMinHeight, style);
                    glyph = stretch(&self.ctx, glyph, Axis::Y, min_height, 0.0);
                }
                center_on_axis(&self.ctx, &mut glyph, style);
                glyph.text_like = false;
                glyph.large = true;
                (glyph, node.kind != K::Integral)
            }
        };
        let mut base = base;
        base.limits = limits;
        let sub = self.optional(node, 0, style.subscript());
        let sup = if node.kind == K::Limit {
            None
        } else {
            self.optional(node, 1, style.superscript())
        };
        if sub.is_none() && sup.is_none() {
            return base;
        }
        self.attach(style, base, sup, sub)
    }

    fn delimited(&mut self, node: &MathNode, style: Style) -> Fragment {
        let body = self.child(node, 0, style);
        let target = balanced_height(&self.ctx, style, std::slice::from_ref(&body));
        let mut parts = Vec::with_capacity(3);
        let left = delimiter_char(Self::attribute(node, "left"));
        let right = delimiter_char(Self::attribute(node, "right"));
        if let Some(open) =
            left.and_then(|c| delimiter(&self.ctx, style, c, AtomClass::Open, target))
        {
            parts.push(open);
        }
        parts.push(body);
        if let Some(close) =
            right.and_then(|c| delimiter(&self.ctx, style, c, AtomClass::Close, target))
        {
            parts.push(close);
        }
        let mut fragment = layout_row(&self.ctx, style, parts);
        fragment.class = AtomClass::Inner;
        fragment.glyph = None;
        fragment
    }

    fn sized_delimiter(&mut self, node: &MathNode, style: Style) -> Fragment {
        let class = match Self::attribute(node, "atomClass") {
            "open" => AtomClass::Open,
            "close" => AtomClass::Close,
            "rel" => AtomClass::Rel,
            _ => AtomClass::Ord,
        };
        let scale = match Self::attribute(node, "size") {
            "Big" => 1.8,
            "bigg" => 2.4,
            "Bigg" => 3.0,
            _ => 1.2,
        };
        let target = scale * self.ctx.font_size(style);
        let Some(character) = delimiter_char(node.text.as_deref().unwrap_or("empty")) else {
            return Fragment::new(Frame::new(0.0, 0.0, 0.0), class);
        };
        delimiter(&self.ctx, style, character, class, target)
            .unwrap_or_else(|| self.tofu(class, style))
    }

    fn matrix(&mut self, node: &MathNode, style: Style) -> Fragment {
        let kind = match Self::attribute(node, "style") {
            "" => "matrix",
            other => other,
        };
        let em = self.ctx.font_size(style);
        let (left, right) = match kind {
            "pmatrix" => (Some('('), Some(')')),
            "bmatrix" => (Some('['), Some(']')),
            "Bmatrix" => (Some('{'), Some('}')),
            "vmatrix" => (Some('|'), Some('|')),
            "Vmatrix" => (Some('‖'), Some('‖')),
            "cases" => (Some('{'), None),
            _ => (None, None),
        };
        let columns = node
            .children
            .iter()
            .filter(|child| child.kind == K::Row)
            .map(|row| row.children.len())
            .max()
            .unwrap_or(0);
        let mut spec = TableSpec {
            aligns: vec![Align::Center; columns],
            vlines: Vec::new(),
            hlines: Vec::new(),
            column_gap: 0.5 * em,
            row_gap: 0.2 * em,
        };
        let cell_style = match kind {
            "aligned" | "gathered" | "split" => style,
            _ => style.with_size(style.size.min(MathSize::Text)),
        };
        match kind {
            "cases" => {
                spec.aligns = vec![Align::Left; columns];
                spec.column_gap = em;
            }
            "aligned" | "split" => {
                spec.aligns = (0..columns)
                    .map(|index| {
                        if index % 2 == 0 {
                            Align::Right
                        } else {
                            Align::Left
                        }
                    })
                    .collect();
                spec.column_gap = 0.0;
            }
            "array" => {
                let (aligns, vlines) = parse_columns(Self::attribute(node, "columns"));
                spec.aligns = aligns;
                spec.aligns
                    .resize(columns.max(spec.aligns.len()), Align::Center);
                spec.vlines = vlines;
            }
            _ => {}
        }
        let mut rows = Vec::new();
        for child in &node.children {
            match child.kind {
                K::Row => {
                    let cells = child
                        .children
                        .iter()
                        .map(|cell| self.node(cell, cell_style))
                        .collect();
                    rows.push(TableRow::Cells(cells));
                }
                K::Hline => spec.hlines.push(rows.len()),
                K::Intertext => rows.push(TableRow::Full(self.node(child, cell_style))),
                _ => rows.push(TableRow::Cells(vec![self.node(child, cell_style)])),
            }
        }
        let table = layout_table(&self.ctx, cell_style, rows, &spec);
        if left.is_none() && right.is_none() {
            return table;
        }
        let target = table.height();
        let mut parts = Vec::with_capacity(3);
        if let Some(open) =
            left.and_then(|c| delimiter(&self.ctx, style, c, AtomClass::Open, target))
        {
            parts.push(open);
        }
        parts.push(table);
        if let Some(close) =
            right.and_then(|c| delimiter(&self.ctx, style, c, AtomClass::Close, target))
        {
            parts.push(close);
        }
        let mut fragment = layout_row(&self.ctx, style, parts);
        fragment.class = AtomClass::Inner;
        fragment
    }

    fn accent(&mut self, node: &MathNode, style: Style) -> Fragment {
        let accent = Self::attribute(node, "accent");
        match accent {
            "overline" => {
                let body = self.child(node, 0, style.cramped());
                return layout_line(&self.ctx, style, body, true);
            }
            "underline" => {
                let body = self.child(node, 0, style);
                return layout_line(&self.ctx, style, body, false);
            }
            "cancel" | "bcancel" | "xcancel" => {
                let body = self.child(node, 0, style);
                let kind = match accent {
                    "bcancel" => CancelKind::Back,
                    "xcancel" => CancelKind::Cross,
                    _ => CancelKind::Forward,
                };
                return layout_cancel(style, body, kind);
            }
            "boxed" => {
                let body = self.child(node, 0, style);
                return boxed(body, None, Some(style.color), style);
            }
            "not" => {
                let body = self.child(node, 0, style);
                return self.negate(body, style);
            }
            _ => {}
        }
        let (character, top) = match accent {
            "overleftarrow" => ('\u{20D6}', true),
            "overrightarrow" => ('\u{20D7}', true),
            "overleftrightarrow" => ('\u{20E1}', true),
            "underleftarrow" => ('\u{20EE}', false),
            "underrightarrow" => ('\u{20EF}', false),
            "underleftrightarrow" => ('\u{034D}', false),
            "overbrace" => ('⏞', true),
            "underbrace" => ('⏟', false),
            other => match accent_char(other) {
                Some(character) => (character, true),
                None => {
                    self.ctx.diagnostic("math.accent.unknown", other);
                    return self.child(node, 0, style);
                }
            },
        };
        let brace = matches!(accent, "overbrace" | "underbrace");
        let body_style = if top { style.cramped() } else { style };
        let body = self.child(node, 0, body_style);
        let Some(mark) = glyph_fragment(&self.ctx, style, character, AtomClass::Ord) else {
            self.ctx
                .diagnostic("math.glyph.missing", format!("U+{:04X}", character as u32));
            return body;
        };
        if brace {
            return self.brace(body, mark, top, style);
        }
        layout_accent(&self.ctx, style, body, mark, top)
    }

    /// 横向花括号：伸展到内容宽度，与内容隔开 0.25em；结果之后的脚标按上下限放置。
    fn brace(&mut self, body: Fragment, mark: Fragment, top: bool, style: Style) -> Fragment {
        let gap = 0.25 * self.ctx.font_size(style);
        let mark = stretch(&self.ctx, mark, Axis::X, body.width(), 0.0);
        let width = body.width().max(mark.width());
        let class = body.class;
        let (body_x, mark_x) = ((width - body.width()) / 2.0, (width - mark.width()) / 2.0);
        let (body_height, mark_height) = (body.height(), mark.height());
        let height = body_height + gap + mark_height;
        let mut frame;
        if top {
            frame = Frame::new(width, height, mark.height() + gap + body.ascent());
            frame.push_frame(Point::new(mark_x, 0.0), mark.frame);
            frame.push_frame(Point::new(body_x, mark_height + gap), body.frame);
        } else {
            frame = Frame::new(width, height, body.ascent());
            frame.push_frame(Point::new(body_x, 0.0), body.frame);
            frame.push_frame(Point::new(mark_x, body_height + gap), mark.frame);
        }
        let mut fragment = Fragment::new(frame, class);
        fragment.limits = true;
        fragment.stretchy_x = true;
        fragment
    }

    /// `\not`：在内容上居中叠加长斜线。
    fn negate(&mut self, body: Fragment, style: Style) -> Fragment {
        let Some(slash) = glyph_fragment(&self.ctx, style, '⧸', AtomClass::Ord)
            .or_else(|| glyph_fragment(&self.ctx, style, '/', AtomClass::Ord))
        else {
            return body;
        };
        let class = body.class;
        let width = body.width().max(slash.width());
        let ascent = body.ascent().max(slash.ascent());
        let descent = body.descent().max(slash.descent());
        let mut frame = Frame::new(width, ascent + descent, ascent);
        let body_pos = Point::new((width - body.width()) / 2.0, ascent - body.ascent());
        let slash_pos = Point::new((width - slash.width()) / 2.0, ascent - slash.ascent());
        frame.push_frame(body_pos, body.frame);
        frame.push_frame(slash_pos, slash.frame);
        Fragment::new(frame, class)
    }

    fn space(&mut self, node: &MathNode, style: Style) -> Fragment {
        let em = self.ctx.font_size(style);
        let width = match Self::attribute(node, "spaceKind") {
            "thin" => 3.0 / 18.0,
            "medium" => 4.0 / 18.0,
            "thick" => 5.0 / 18.0,
            "negative_thin" => -3.0 / 18.0,
            "quad" => 1.0,
            "qquad" => 2.0,
            "hspace" => {
                let read = |key: &str, default: f64| {
                    node.attributes
                        .get(key)
                        .and_then(|value| value.parse::<f64>().ok())
                        .unwrap_or(default)
                };
                let denominator = read("valueDenominator", 1.0);
                if super::is_zero(denominator) {
                    0.0
                } else {
                    read("valueNumerator", 0.0) / denominator
                }
            }
            _ => 0.25,
        };
        Fragment::spacing(width * em)
    }

    // ---- 文本与字母表 ----

    fn text(&mut self, node: &MathNode, style: Style) -> Fragment {
        let kind = Self::attribute(node, "style");
        let has_text_items = node.children.iter().any(|child| {
            matches!(
                child.kind,
                K::TextContent | K::TextSpace | K::TextSymbol | K::TextStyle
            )
        });
        let text_mode = kind == "text" || has_text_items;
        let variant = match kind {
            "rm" | "text" => Variant::Roman,
            "bf" => Variant::Bold,
            "it" => Variant::Italic,
            "sf" => Variant::SansSerif,
            "tt" => Variant::Monospace,
            "bb" => Variant::DoubleStruck,
            "cal" | "scr" => Variant::Script,
            "frak" => Variant::Fraktur,
            "boldsymbol" | "pmb" => Variant::BoldItalic,
            _ => style.variant,
        };
        let inner = Style {
            variant,
            text_mode,
            ..style
        };
        if !text_mode {
            let mut fragment = self.row(&node.children, inner);
            if node.children.len() > 1 {
                fragment.class = AtomClass::Ord;
            }
            return fragment;
        }
        let fragments: Vec<Fragment> = node
            .children
            .iter()
            .map(|child| self.text_item(child, inner))
            .collect();
        let mut fragment = self.joined(fragments, AtomClass::Ord, inner);
        fragment.text_like = true;
        fragment
    }

    /// 文本模式中的一项；数学子项恢复数学模式。
    fn text_item(&mut self, node: &MathNode, style: Style) -> Fragment {
        let em = self.ctx.font_size(style);
        let mut fragment = match node.kind {
            K::TextContent | K::TextSymbol => {
                let text = node.text.as_deref().unwrap_or_default();
                let fragments = text
                    .chars()
                    .map(|character| {
                        if character == ' ' {
                            Fragment::new(Frame::new(0.25 * em, 0.0, 0.0), AtomClass::Ord)
                        } else {
                            self.character(character, AtomClass::Ord, style)
                        }
                    })
                    .collect();
                self.joined(fragments, AtomClass::Ord, style)
            }
            K::TextSpace => Fragment::new(Frame::new(0.25 * em, 0.0, 0.0), AtomClass::Ord),
            K::TextStyle => {
                let variant = match Self::attribute(node, "style") {
                    "bf" => Variant::Bold,
                    "it" => Variant::Italic,
                    "sf" => Variant::SansSerif,
                    "tt" => Variant::Monospace,
                    _ => Variant::Roman,
                };
                let inner = Style { variant, ..style };
                let fragments = node
                    .children
                    .iter()
                    .map(|child| self.text_item(child, inner))
                    .collect();
                self.joined(fragments, AtomClass::Ord, inner)
            }
            _ => {
                let math = Style {
                    text_mode: false,
                    variant: Variant::Normal,
                    ..style
                };
                return self.node(node, math);
            }
        };
        let record = Item::Node {
            id: node.id.clone(),
            width: fragment.frame.width,
            height: fragment.frame.height,
            baseline: fragment.frame.baseline,
        };
        fragment
            .frame
            .items
            .insert(0, (Point::new(0.0, 0.0), record));
        fragment
    }

    fn color_box(&mut self, node: &MathNode, style: Style) -> Fragment {
        let (fill, border) = if node.kind == K::ColorBox {
            (MathColor::parse_hex(Self::attribute(node, "color")), None)
        } else {
            (
                MathColor::parse_hex(Self::attribute(node, "fillColor")),
                Some(MathColor::parse_hex(Self::attribute(node, "borderColor"))),
            )
        };
        let body = match node.children.first() {
            Some(child) if child.kind == K::Text => {
                let text_style = Style {
                    text_mode: true,
                    variant: Variant::Roman,
                    ..style
                };
                let fragments = child
                    .children
                    .iter()
                    .map(|item| self.text_item(item, text_style))
                    .collect();
                self.joined(fragments, AtomClass::Ord, text_style)
            }
            Some(child) => self.node(child, style),
            None => Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord),
        };
        boxed(body, fill, border, style)
    }
}

/// 加内边距并画背景/边框。`border` 为 `Some` 时描边，其内层为描边颜色。
fn boxed(
    body: Fragment,
    fill: Option<MathColor>,
    border: Option<Option<MathColor>>,
    _style: Style,
) -> Fragment {
    let pad = BOX_PADDING;
    let width = body.width() + 2.0 * pad;
    let height = body.height() + 2.0 * pad;
    let mut frame = Frame::new(width, height, body.ascent() + pad);
    frame.push(
        Point::new(0.0, 0.0),
        Item::Rect {
            width,
            height,
            fill,
            stroke: border.map(|color| (color, BOX_STROKE)),
        },
    );
    frame.push_frame(Point::new(pad, pad), body.frame);
    Fragment::new(frame, AtomClass::Ord)
}

/// 解析 `array` 列格式：`l`/`c`/`r` 为列，`|` 为竖线。
fn parse_columns(spec: &str) -> (Vec<Align>, Vec<usize>) {
    let mut aligns = Vec::new();
    let mut vlines = Vec::new();
    for character in spec.chars() {
        match character {
            'l' => aligns.push(Align::Left),
            'c' => aligns.push(Align::Center),
            'r' => aligns.push(Align::Right),
            '|' => vlines.push(aligns.len()),
            _ => {}
        }
    }
    (aligns, vlines)
}
