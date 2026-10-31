//! 文档数学树到 LaTeX 的序列化。
//!
//! 输出是解析器可接受的规范形式：参数一律加花括号，控制词后按需补空格。
//! 对解析器产出的树，`parse(serialize(tree))` 与原树结构相同（节点 ID 与源范围除外）。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{Align, MathNode, MathNodeKind, MathTree};

use crate::symbols::{NAMED_COLORS, delimiter_source};

/// 序列化单棵数学树（不含外层 `$` 或 `\[`）。
pub fn serialize_tree(tree: &MathTree) -> String {
    serialize_node(&tree.root)
}

/// 序列化任意数学节点。
pub fn serialize_node(node: &MathNode) -> String {
    let mut writer = Writer::default();
    writer.node(node);
    writer.output
}

/// 序列化对齐公式为 `aligned` 环境。
pub fn serialize_align(align: &Align) -> String {
    let mut writer = Writer::default();
    writer.raw("\\begin{aligned}");
    for (index, row) in align.rows.iter().enumerate() {
        if index > 0 {
            writer.raw(" \\\\");
        }
        for (cell_index, cell) in row.cells.iter().enumerate() {
            writer.raw(if cell_index == 0 { " " } else { " & " });
            writer.node(&cell.root);
        }
    }
    writer.raw(" \\end{aligned}");
    writer.output
}

#[derive(Default)]
struct Writer {
    output: String,
    /// 上一段输出以控制词结尾，紧跟字母时需要空格分隔。
    after_control_word: bool,
}

impl Writer {
    fn raw(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.after_control_word && text.starts_with(|c: char| c.is_ascii_alphabetic()) {
            self.output.push(' ');
        }
        self.output.push_str(text);
        self.after_control_word = false;
    }

    /// 写出控制词，之后的字母会自动隔开。
    fn command(&mut self, name: &str) {
        self.raw("\\");
        self.output.push_str(name);
        self.after_control_word = name.bytes().all(|byte| byte.is_ascii_alphabetic());
    }

    /// 花括号参数。
    fn group(&mut self, node: &MathNode) {
        self.raw("{");
        self.contents(node);
        self.raw("}");
    }

    /// 写出节点内容；Row 展开为子项，避免多余的花括号层级。
    fn contents(&mut self, node: &MathNode) {
        match node.kind {
            MathNodeKind::Row => self.children(&node.children),
            MathNodeKind::Placeholder => {}
            _ => self.node(node),
        }
    }

    fn children(&mut self, children: &[MathNode]) {
        for (index, child) in children.iter().enumerate() {
            let is_last = index + 1 == children.len();
            if child.kind == MathNodeKind::Color && !is_last {
                // `\color` 作用到列表末尾，不在末尾时用分组限定范围。
                self.raw("{");
                self.node(child);
                self.raw("}");
            } else {
                self.node(child);
            }
        }
    }

    fn attribute<'a>(node: &'a MathNode, key: &str) -> &'a str {
        node.attributes.get(key).map_or("", String::as_str)
    }

    fn child(node: &MathNode, index: usize) -> Option<&MathNode> {
        node.children.get(index)
    }

    fn color(&mut self, value: &str) {
        match NAMED_COLORS.iter().find(|(_, hex)| *hex == value) {
            Some((name, _)) => {
                self.raw("{");
                self.raw(name);
                self.raw("}");
            }
            None => {
                self.raw("[HTML]{");
                self.raw(&value.trim_start_matches('#').to_ascii_uppercase());
                self.raw("}");
            }
        }
    }

    fn script(&mut self, marker: &str, node: Option<&MathNode>) {
        if let Some(node) = node.filter(|node| node.kind != MathNodeKind::Placeholder) {
            self.raw(marker);
            self.group(node);
        }
    }

    /// 上下标基底：会与前文合并或带内部结构的基底加花括号。
    fn base(&mut self, node: Option<&MathNode>) {
        let Some(node) = node else {
            return;
        };
        let needs_group = match node.kind {
            MathNodeKind::Placeholder => {
                self.raw("{}");
                return;
            }
            MathNodeKind::Row
            | MathNodeKind::Superscript
            | MathNodeKind::Subscript
            | MathNodeKind::SubSup
            | MathNodeKind::Sum
            | MathNodeKind::Product
            | MathNodeKind::Integral
            | MathNodeKind::Limit => true,
            MathNodeKind::Symbol => node.text.as_ref().is_some_and(|text| {
                node.attributes.contains_key("atomClass") && text.chars().count() > 1
            }),
            _ => false,
        };
        if needs_group {
            self.group(node);
        } else {
            self.node(node);
        }
    }

    fn node(&mut self, node: &MathNode) {
        use MathNodeKind as K;
        match node.kind {
            K::Row | K::Style => self.children(&node.children),
            K::Group => {
                self.raw("{");
                self.children(&node.children);
                self.raw("}");
            }
            K::Placeholder => self.raw("{}"),
            K::Symbol => {
                let text = node.text.as_deref().unwrap_or_default();
                if node.attributes.contains_key("atomClass") {
                    for character in text.chars() {
                        self.literal(character);
                    }
                } else {
                    self.command(text);
                }
            }
            K::Operator => {
                let text = node.text.as_deref().unwrap_or_default();
                let limits = Self::attribute(node, "limits") == "true";
                if crate::symbols::text_operator_limits(text) == Some(limits) {
                    self.command(text);
                } else {
                    self.command(if limits {
                        "operatorname*"
                    } else {
                        "operatorname"
                    });
                    self.raw("{");
                    self.raw(text);
                    self.raw("}");
                }
            }
            K::Fraction => {
                let style = Self::attribute(node, "style");
                self.command(if style.is_empty() { "frac" } else { style });
                for index in 0..2 {
                    if let Some(child) = Self::child(node, index) {
                        self.group(child);
                    }
                }
            }
            K::Radical => {
                self.command("sqrt");
                if let Some(index) =
                    Self::child(node, 0).filter(|index| index.kind != K::Placeholder)
                {
                    self.raw("[");
                    self.contents(index);
                    self.raw("]");
                }
                if let Some(body) = Self::child(node, 1) {
                    self.group(body);
                }
            }
            K::Superscript => {
                self.base(Self::child(node, 0));
                self.superscript(Self::child(node, 1));
            }
            K::Subscript => {
                self.base(Self::child(node, 0));
                self.script("_", Self::child(node, 1));
            }
            K::SubSup => {
                self.base(Self::child(node, 0));
                self.script("_", Self::child(node, 1));
                self.superscript(Self::child(node, 2));
            }
            K::Sum | K::Product | K::Integral => {
                let name = match node.kind {
                    K::Sum => "sum",
                    K::Product => "prod",
                    _ => match Self::attribute(node, "integralKind") {
                        "" => "int",
                        kind => kind,
                    },
                };
                self.command(name);
                self.script("_", Self::child(node, 0));
                self.script("^", Self::child(node, 1));
            }
            K::Limit => {
                self.command("lim");
                self.script("_", Self::child(node, 0));
            }
            K::Delimited => {
                self.command("left");
                self.delimiter(Self::attribute(node, "left"));
                if let Some(body) = Self::child(node, 0) {
                    self.contents(body);
                }
                self.command("right");
                self.delimiter(Self::attribute(node, "right"));
            }
            K::SizedDelimiter => {
                let suffix = match Self::attribute(node, "atomClass") {
                    "open" => "l",
                    "close" => "r",
                    "rel" => "m",
                    _ => "",
                };
                let size = match Self::attribute(node, "size") {
                    "" => "big",
                    size => size,
                };
                self.command(&format!("{size}{suffix}"));
                self.delimiter(node.text.as_deref().unwrap_or("empty"));
            }
            K::Matrix => self.matrix(node),
            K::Substack => {
                self.command("substack");
                self.raw("{");
                for (index, line) in node.children.iter().enumerate() {
                    if index > 0 {
                        self.raw(" \\\\ ");
                    }
                    self.contents(line);
                }
                self.raw("}");
            }
            K::Intertext => {
                self.command("intertext");
                self.raw("{");
                if let Some(text) = Self::child(node, 0) {
                    self.text_children(&text.children);
                }
                self.raw("}");
            }
            K::Hline => self.command("hline"),
            K::OverUnder => {
                let base = Self::child(node, 0);
                let over = Self::child(node, 1).filter(|child| child.kind != K::Placeholder);
                let under = Self::child(node, 2).filter(|child| child.kind != K::Placeholder);
                match (over, under) {
                    (Some(over), Some(under)) => {
                        self.command("overset");
                        self.group(over);
                        self.raw("{");
                        self.command("underset");
                        self.group(under);
                        self.optional_group(base);
                        self.raw("}");
                    }
                    (Some(over), None) => {
                        self.command("overset");
                        self.group(over);
                        self.optional_group(base);
                    }
                    (None, Some(under)) => {
                        self.command("underset");
                        self.group(under);
                        self.optional_group(base);
                    }
                    (None, None) => {
                        if let Some(base) = base {
                            self.node(base);
                        }
                    }
                }
            }
            K::CancelTo => {
                self.command("cancelto");
                self.optional_group(Self::child(node, 0));
                self.optional_group(Self::child(node, 1));
            }
            K::Accent => {
                let accent = Self::attribute(node, "accent");
                self.command(if accent == "ring" { "mathring" } else { accent });
                self.optional_group(Self::child(node, 0));
            }
            K::Space => self.space(node),
            K::Text => self.text(node),
            K::TextContent | K::TextSpace | K::TextSymbol | K::TextStyle => {
                // 文本项脱离 Text 出现时按 `\text` 包装。
                self.command("text");
                self.raw("{");
                self.text_children(std::slice::from_ref(node));
                self.raw("}");
            }
            K::Color => {
                self.command("color");
                self.color(Self::attribute(node, "color"));
                if let Some(body) = Self::child(node, 0) {
                    self.contents(body);
                }
            }
            K::TextColor => {
                self.command("textcolor");
                self.color(Self::attribute(node, "color"));
                self.optional_group(Self::child(node, 0));
            }
            K::ColorBox | K::FColorBox => {
                if node.kind == K::ColorBox {
                    self.command("colorbox");
                    self.color(Self::attribute(node, "color"));
                } else {
                    self.command("fcolorbox");
                    self.color(Self::attribute(node, "borderColor"));
                    self.color(Self::attribute(node, "fillColor"));
                }
                self.raw("{");
                match Self::child(node, 0) {
                    Some(text) if text.kind == K::Text => self.text_children(&text.children),
                    Some(other) => {
                        self.raw("$");
                        self.contents(other);
                        self.raw("$");
                    }
                    None => {}
                }
                self.raw("}");
            }
            K::Strut => {
                let read = |prefix: &str| -> (i128, i128) {
                    let value = |suffix: &str| {
                        node.attributes
                            .get(&format!("{prefix}{suffix}"))
                            .and_then(|value| value.parse::<i128>().ok())
                    };
                    (
                        value("Numerator").unwrap_or(0),
                        value("Denominator").unwrap_or(1).max(1),
                    )
                };
                let height = read("height");
                let depth = read("depth");
                // `\rule[-d]{0em}{h+d}` 产生高 h、深 d 的零宽支柱。
                let total = (height.0 * depth.1 + depth.0 * height.1, height.1 * depth.1);
                self.command("rule");
                if depth.0 != 0 {
                    self.raw(&format!("[{}]", dimension((-depth.0, depth.1))));
                }
                self.raw(&format!("{{0em}}{{{}}}", dimension(total)));
            }
            K::Phantom => {
                let kind = match Self::attribute(node, "phantomKind") {
                    "" => "phantom",
                    kind => kind,
                };
                self.command(kind);
                self.optional_group(Self::child(node, 0));
            }
        }
    }

    fn optional_group(&mut self, node: Option<&MathNode>) {
        match node {
            Some(node) => self.group(node),
            None => self.raw("{}"),
        }
    }

    /// 上标：开头的连续撇号写成 `'`，其余部分写成 `^{...}`。
    fn superscript(&mut self, node: Option<&MathNode>) {
        let Some(node) = node else {
            return;
        };
        let items: &[MathNode] = if node.kind == MathNodeKind::Row {
            &node.children
        } else {
            std::slice::from_ref(node)
        };
        let primes = items.iter().take_while(|item| is_prime(item)).count();
        for _ in 0..primes {
            self.raw("'");
        }
        let rest = &items[primes..];
        if rest.is_empty() {
            if primes == 0 {
                self.raw("^{}");
            }
            return;
        }
        self.raw("^{");
        self.children(rest);
        self.raw("}");
    }

    fn delimiter(&mut self, encoded: &str) {
        let source = delimiter_source(encoded);
        match source.strip_prefix('\\') {
            Some(name) if name.bytes().all(|byte| byte.is_ascii_alphabetic()) => self.command(name),
            _ => self.raw(&source),
        }
    }

    /// 字面字符，LaTeX 特殊字符加反斜杠转义。
    fn literal(&mut self, character: char) {
        match character {
            '%' | '&' | '#' | '_' | '$' => {
                self.raw("\\");
                self.output.push(character);
            }
            '{' => self.command("lbrace"),
            '}' => self.command("rbrace"),
            '\\' => self.command("backslash"),
            '~' | '^' => {
                self.raw("\\text{");
                self.raw(&character.to_string());
                self.raw("}");
            }
            other => self.raw(&other.to_string()),
        }
    }

    fn space(&mut self, node: &MathNode) {
        match Self::attribute(node, "spaceKind") {
            "thin" => self.raw("\\,"),
            "medium" => self.raw("\\:"),
            "thick" => self.raw("\\;"),
            "negative_thin" => self.raw("\\!"),
            "quad" => self.command("quad"),
            "qquad" => self.command("qquad"),
            "no_break_space" => self.raw("~"),
            "hspace" => {
                let value = |key: &str| {
                    node.attributes
                        .get(key)
                        .and_then(|value| value.parse::<i128>().ok())
                };
                let numerator = value("valueNumerator").unwrap_or(0);
                let denominator = value("valueDenominator").unwrap_or(1).max(1);
                self.command("hspace");
                self.raw(&format!("{{{}}}", dimension((numerator, denominator))));
            }
            _ => self.raw("\\ "),
        }
    }

    fn text(&mut self, node: &MathNode) {
        let style = Self::attribute(node, "style");
        let text_mode = style == "text"
            || node.children.is_empty() && matches!(style, "rm" | "bf" | "it" | "sf" | "tt")
            || node.children.iter().any(is_text_item);
        if text_mode {
            self.command(match style {
                "rm" => "textrm",
                "bf" => "textbf",
                "it" => "textit",
                "sf" => "textsf",
                "tt" => "texttt",
                _ => "text",
            });
            self.raw("{");
            self.text_children(&node.children);
            self.raw("}");
        } else {
            let command = match style {
                "boldsymbol" | "pmb" => style.to_owned(),
                other => format!("math{other}"),
            };
            self.command(&command);
            self.raw("{");
            self.children(&node.children);
            self.raw("}");
        }
    }

    fn text_children(&mut self, children: &[MathNode]) {
        for child in children {
            match child.kind {
                MathNodeKind::TextContent => {
                    for character in child.text.as_deref().unwrap_or_default().chars() {
                        match character {
                            '%' | '&' | '#' | '_' | '$' | '{' | '}' => {
                                self.raw("\\");
                                self.output.push(character);
                            }
                            '\\' | '~' | '^' => {
                                self.raw(&format!("\\symbol{{{}}}", character as u32))
                            }
                            other => self.raw(&other.to_string()),
                        }
                    }
                }
                MathNodeKind::TextSpace => match child.text.as_deref() {
                    Some("control_space") => self.raw("\\ "),
                    Some("no_break_space") => self.raw("~"),
                    _ => self.raw(" "),
                },
                MathNodeKind::TextSymbol => {
                    self.raw("\\");
                    self.output
                        .push_str(child.text.as_deref().unwrap_or_default());
                }
                MathNodeKind::TextStyle => {
                    let style = Self::attribute(child, "style");
                    self.command(&format!("text{style}"));
                    self.raw("{");
                    self.text_children(&child.children);
                    self.raw("}");
                }
                _ => {
                    self.raw("$");
                    self.contents(child);
                    self.raw("$");
                }
            }
        }
    }

    fn matrix(&mut self, node: &MathNode) {
        let style = match Self::attribute(node, "style") {
            "" => "matrix",
            style => style,
        };
        self.command("begin");
        self.raw(&format!("{{{style}}}"));
        if style == "array" {
            self.raw(&format!("{{{}}}", Self::attribute(node, "columns")));
        }
        let mut previous_was_row = false;
        for child in &node.children {
            match child.kind {
                MathNodeKind::Row => {
                    if previous_was_row {
                        self.raw(" \\\\");
                    }
                    for (index, cell) in child.children.iter().enumerate() {
                        self.raw(if index == 0 { " " } else { " & " });
                        self.contents(cell);
                    }
                    previous_was_row = true;
                }
                _ => {
                    if previous_was_row {
                        self.raw(" \\\\");
                    }
                    self.raw(" ");
                    self.node(child);
                    previous_was_row = false;
                }
            }
        }
        self.raw(" ");
        self.command("end");
        self.raw(&format!("{{{style}}}"));
    }
}

fn is_prime(node: &MathNode) -> bool {
    node.kind == MathNodeKind::Symbol
        && node.text.as_deref() == Some("prime")
        && node.attributes.is_empty()
}

fn is_text_item(node: &MathNode) -> bool {
    matches!(
        node.kind,
        MathNodeKind::TextContent
            | MathNodeKind::TextSpace
            | MathNodeKind::TextSymbol
            | MathNodeKind::TextStyle
    )
}

/// 有理数 em 长度写成解析器可读的形式：能精确写成小数 em 时用 em，
/// 否则在能精确写成小数 mu（1/18 em）时用 mu。
fn dimension((numerator, denominator): (i128, i128)) -> String {
    if is_finite_decimal(denominator) {
        return format!("{}em", decimal((numerator, denominator)));
    }
    let mu = (numerator * 18, denominator);
    if is_finite_decimal(mu.1 / gcd(mu.0, mu.1)) {
        return format!("{}mu", decimal(mu));
    }
    format!("{}em", decimal((numerator, denominator)))
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

fn is_finite_decimal(mut denominator: i128) -> bool {
    denominator = denominator.abs().max(1);
    for factor in [2, 5] {
        while denominator % factor == 0 {
            denominator /= factor;
        }
    }
    denominator == 1
}

/// 有理数写成有限小数；分母含 2 和 5 以外的因子时保留六位小数。
fn decimal((numerator, denominator): (i128, i128)) -> String {
    let negative = (numerator < 0) != (denominator < 0);
    let (numerator, denominator) = (numerator.unsigned_abs(), denominator.unsigned_abs().max(1));
    let whole = numerator / denominator;
    let mut remainder = numerator % denominator;
    let mut digits = String::new();
    let mut steps = 0;
    while remainder != 0 && steps < 18 {
        remainder *= 10;
        digits.push(char::from(b'0' + (remainder / denominator) as u8));
        remainder %= denominator;
        steps += 1;
    }
    let sign = if negative && (whole != 0 || !digits.is_empty()) {
        "-"
    } else {
        ""
    };
    if digits.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{digits}")
    }
}
