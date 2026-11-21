//! 剪贴板数学辅助：规范 TeX 的剪贴板定界符包装与外部纯文本数学识别。
//!
//! [`wrap_tex`] 把共享检查式序列化器生成的规范 TeX 正文包装为剪贴板架构规定的
//! `text/plain` 定界符；[`scan_plain_text_math`] 在外部纯文本粘贴时只识别显式、
//! 未转义且闭合的数学定界符，边界不确定时整段回退为普通文本，不部分插入公式。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::MATH_ALIGNED_MATRIX_STYLE;

use crate::{MathFragment, MathInputKind};

/// 把规范 TeX 正文包装为宿主种类对应的剪贴板定界符。
///
/// 行内公式使用 `\(...\)`；Equation 和 Align 使用 `\[...\]`。Align 的正文来自
/// [`MathFragment::to_tex`]，已经包含完整的 `\begin{aligned}...\end{aligned}`
/// 环境，因此这里只添加显示公式定界符，不重复包裹环境。
pub fn wrap_tex(tex: &str, kind: MathInputKind) -> String {
    let (opening, closing) = match kind {
        MathInputKind::Inline => (r"\(", r"\)"),
        MathInputKind::Equation | MathInputKind::Align => (r"\[", r"\]"),
    };
    let mut wrapped = String::with_capacity(opening.len() + tex.len() + closing.len());
    wrapped.push_str(opening);
    wrapped.push_str(tex);
    wrapped.push_str(closing);
    wrapped
}

/// 外部纯文本中识别出的一处数学片段。
///
/// `start`/`end` 是输入文本中的 UTF-8 字节范围，包含首尾定界符；`fragment` 已经
/// 按 `kind` 完成解析和宿主约束检查。
#[derive(Debug, Clone, PartialEq)]
pub struct PlainTextMathSpan {
    /// 片段对应的文档对象种类；混合文本中的片段恒为 [`MathInputKind::Inline`]。
    pub kind: MathInputKind,
    /// 解析得到的数学片段。
    pub fragment: MathFragment,
    /// 片段起始字节偏移（包含定界符）。
    pub start: usize,
    /// 片段结束字节偏移（不含，包含定界符）。
    pub end: usize,
}

/// 外部纯文本粘贴按数学片段切分后的一段内容。
#[derive(Debug, Clone, PartialEq)]
pub enum PlainTextMathSegment {
    /// 不属于任何数学片段的原文。
    Text(String),
    /// 一处已识别的数学片段。
    Math(PlainTextMathSpan),
}

/// 外部纯文本数学识别的结果。
#[derive(Debug, Clone, PartialEq)]
pub enum PlainTextMathScan {
    /// 没有可安全识别的数学片段，整段按普通文本处理。
    Plain,
    /// 去除首尾空白后的全部文本是一个显示公式（`\[...\]` 或 `$$...$$`）。
    ///
    /// 宿主种类只可能是 [`MathInputKind::Equation`] 或 [`MathInputKind::Align`]。
    WholeDisplay(MathFragment),
    /// 去除首尾空白后的全部文本是一个行内公式（`$...$` 或 `\(...\)`）。
    WholeInline(MathFragment),
    /// 混合文本中识别出的行内公式；段按原文顺序排列，至少有一处公式。
    ///
    /// 相邻公式之间不插入空 `Text` 段；文本段保留原文和精确内容。混合识别
    /// 只处理成对的 `\(...\)`，`$` 不参与。
    MixedInline(Vec<PlainTextMathSegment>),
}

/// 识别外部纯文本中的数学片段。
///
/// 识别顺序与剪贴板架构的约定一致：
///
/// 1. 整段（trim 后）是 `\[...\]` 或 `$$...$$`：内容完整包裹一个规范
///    `aligned` 环境时构造 [`MathInputKind::Align`]，否则构造
///    [`MathInputKind::Equation`]；
/// 2. 整段是单个 `$...$` 或 `\(...\)` 时构造 [`MathInputKind::Inline`]；
/// 3. 其余文本只识别成对的 `\(...\)` 行内片段，`$` 保持原文。
///
/// 未闭合定界符、解析失败、超限或无法确定完整边界时整段回退为
/// [`PlainTextMathScan::Plain`]，不会部分插入公式。解析复用受限数学语法，
/// 不执行任何 TeX 宏或命令。
pub fn scan_plain_text_math(text: &str) -> PlainTextMathScan {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return PlainTextMathScan::Plain;
    }
    if let Some(content) = strip_display_fence(trimmed) {
        let kind = if is_aligned_environment(content) {
            MathInputKind::Align
        } else {
            MathInputKind::Equation
        };
        return match MathFragment::parse(trimmed, kind) {
            Ok(fragment) => PlainTextMathScan::WholeDisplay(fragment),
            Err(_) => PlainTextMathScan::Plain,
        };
    }
    if is_whole_inline_fence(trimmed) {
        return match MathFragment::parse(trimmed, MathInputKind::Inline) {
            Ok(fragment) => PlainTextMathScan::WholeInline(fragment),
            Err(_) => PlainTextMathScan::Plain,
        };
    }
    match scan_inline_pairs(text) {
        Some(segments) => PlainTextMathScan::MixedInline(segments),
        None => PlainTextMathScan::Plain,
    }
}

/// 去掉 `\[...\]` 或 `$$...$$` 的显示定界符，返回其中的内容。
fn strip_display_fence(trimmed: &str) -> Option<&str> {
    for (opening, closing) in [("\\[", "\\]"), ("$$", "$$")] {
        if trimmed.len() >= opening.len() + closing.len()
            && trimmed.starts_with(opening)
            && trimmed.ends_with(closing)
        {
            return Some(&trimmed[opening.len()..trimmed.len() - closing.len()]);
        }
    }
    None
}

/// 内容（trim 后）是否恰好是一个规范 `aligned` 环境。
fn is_aligned_environment(content: &str) -> bool {
    let name = MATH_ALIGNED_MATRIX_STYLE.name;
    let opening = format!(r"\begin{{{name}}}");
    let closing = format!(r"\end{{{name}}}");
    let trimmed = content.trim();
    trimmed.starts_with(&opening) && trimmed.ends_with(&closing)
}

/// 整段文本是否是单个 `$...$` 或 `\(...\)` 行内公式。
fn is_whole_inline_fence(trimmed: &str) -> bool {
    if trimmed.len() >= 2 && trimmed.starts_with('$') && trimmed.ends_with('$') {
        return true;
    }
    if trimmed.len() < 4 || !trimmed.starts_with(r"\(") || !trimmed.ends_with(r"\)") {
        return false;
    }
    // `\(a\)\(b\)` 之类的多个片段不算“整段”，交给混合识别逐对处理。
    matches!(
        inline_delimiters(trimmed).as_slice(),
        [InlineDelimiter::Open(0), InlineDelimiter::Close(close)] if *close + 2 == trimmed.len()
    )
}

/// 已按转义规则筛选出的行内定界符位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineDelimiter {
    /// `\(` 的字节位置。
    Open(usize),
    /// `\)` 的字节位置。
    Close(usize),
}

/// 收集未被转义的行内定界符。
///
/// 连续反斜杠成对消费：`\\(` 是转义的反斜杠加普通括号，不构成定界符；`\\\(` 是
/// 转义反斜杠后接 `\(`，仍是定界符。
fn inline_delimiters(text: &str) -> Vec<InlineDelimiter> {
    let bytes = text.as_bytes();
    let mut delimiters = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] != b'\\' {
            index += 1;
            continue;
        }
        match bytes[index + 1] {
            b'\\' => index += 2,
            b'(' => {
                delimiters.push(InlineDelimiter::Open(index));
                index += 2;
            }
            b')' => {
                delimiters.push(InlineDelimiter::Close(index));
                index += 2;
            }
            _ => index += 1,
        }
    }
    delimiters
}

/// 把混合文本按成对 `\(...\)` 切段；存在未闭合或多余定界符时返回 `None`。
fn scan_inline_pairs(text: &str) -> Option<Vec<PlainTextMathSegment>> {
    let delimiters = inline_delimiters(text);
    let mut segments = Vec::new();
    let mut text_start = 0;
    let mut index = 0;
    while index < delimiters.len() {
        let InlineDelimiter::Open(start) = delimiters[index] else {
            return None;
        };
        let close_offset = delimiters[index + 1..]
            .iter()
            .position(|delimiter| matches!(delimiter, InlineDelimiter::Close(_)))?;
        let close_index = index + 1 + close_offset;
        let InlineDelimiter::Close(end) = delimiters[close_index] else {
            return None;
        };
        let fragment = MathFragment::parse(&text[start + 2..end], MathInputKind::Inline).ok()?;
        if start > text_start {
            segments.push(PlainTextMathSegment::Text(
                text[text_start..start].to_owned(),
            ));
        }
        segments.push(PlainTextMathSegment::Math(PlainTextMathSpan {
            kind: MathInputKind::Inline,
            fragment,
            start,
            end: end + 2,
        }));
        text_start = end + 2;
        index = close_index + 1;
    }
    if segments.is_empty() {
        return None;
    }
    if text_start < text.len() {
        segments.push(PlainTextMathSegment::Text(text[text_start..].to_owned()));
    }
    Some(segments)
}
