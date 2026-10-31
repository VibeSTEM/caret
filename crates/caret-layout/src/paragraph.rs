//! 将 Parley 行数据映射为 Caret 分页使用的 UTF-8 源范围与行框。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{InlineNode, Paragraph};
use std::ops::Range;

use crate::{LineMetrics, LineMetricsError};

mod parley;
pub use parley::{
    HitAffinity, LocalGlyph, ParagraphHit, ParagraphLayoutOptions, ParagraphLayouter,
    ParleyParagraphLayout, PositionedGlyphRun, PositionedParagraphDrawError,
    PositionedParagraphDrawItem, VisualAffinity, VisualCursor,
};

/// Caret 自有的段内对象放置请求；index 必须指向文本中的 U+FFFC 对象占位符。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InlineObjectPlacement {
    /// 当前段落内唯一的对象 ID。
    pub id: u64,
    /// 对象在逻辑文本中的 UTF-8 字节索引。
    pub index: usize,
    /// 对象占用的行内宽度，单位为 pt。
    pub width_pt: f32,
    /// 对象总高度，单位为 pt。
    pub height_pt: f32,
    /// 对象基线相对盒顶的距离；`None` 表示底边对齐正文基线。
    pub baseline_pt: Option<f32>,
}

/// Parley 已定位的行内对象几何；x 相对段落左边，y 相对对应的 Caret 行顶。
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedInlineObject {
    /// 与输入放置请求一致的对象 ID。
    pub id: u64,
    /// 对象对应的逻辑 UTF-8 字节范围。
    pub source_bytes: Range<usize>,
    /// 对象所在的视觉行索引。
    pub line_index: usize,
    /// 对象左边相对段落左边的 pt 坐标。
    pub x_pt: f32,
    /// 对象顶边相对对应 Caret 行顶的 pt 坐标。
    pub y_pt: f32,
    /// 对象占用的行内宽度，单位为 pt。
    pub width_pt: f32,
    /// 对象高度，单位为 pt。
    pub height_pt: f32,
    /// 对象基线相对对象顶边的距离。
    pub baseline_pt: f32,
}

/// 判断源文本是否会产生需要段落标记度量的空行，包含开头/连续/结尾 mandatory break。
pub fn has_empty_line(text: &str) -> bool {
    let mut state = EmptyLineState::default();
    for character in text.chars() {
        state.push(character);
    }
    state.has_empty_line()
}

/// 不创建拼接后的正文副本，检查段落是否需要段落标记行度量。
pub fn paragraph_has_empty_line(paragraph: &Paragraph) -> bool {
    fn visit(nodes: &[InlineNode], state: &mut EmptyLineState) {
        for node in nodes {
            match node {
                InlineNode::Run(run) => {
                    for character in run.text.chars() {
                        state.push(character);
                    }
                }
                InlineNode::Environment(environment) => visit(&environment.children, state),
                InlineNode::Math(_) => state.push('\u{fffc}'),
                InlineNode::SoftBreak(_) => state.push('\n'),
                InlineNode::Opaque(_) => state.push('\u{fffc}'),
            }
        }
    }
    let mut state = EmptyLineState::default();
    visit(&paragraph.inlines, &mut state);
    state.has_empty_line()
}

#[derive(Default)]
struct EmptyLineState {
    seen_any: bool,
    line_has_content: bool,
    saw_break: bool,
    empty_line: bool,
    previous_was_carriage_return: bool,
}

impl EmptyLineState {
    fn push(&mut self, character: char) {
        self.seen_any = true;
        if self.previous_was_carriage_return && character == '\n' {
            self.previous_was_carriage_return = false;
            return;
        }
        self.previous_was_carriage_return = false;
        if is_mandatory_break_character(character) {
            self.empty_line |= !self.line_has_content;
            self.saw_break = true;
            self.line_has_content = false;
            self.previous_was_carriage_return = character == '\r';
        } else {
            self.line_has_content = true;
        }
    }

    fn has_empty_line(&self) -> bool {
        !self.seen_any || self.empty_line || (self.saw_break && !self.line_has_content)
    }
}

/// 一条段落行对原文的引用和 Caret 行框数据。
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphLine {
    /// Parley visual line 引用的 UTF-8 源范围，不包含强制换行符。
    pub source_bytes: Range<usize>,
    /// 本行消费范围，包含末尾强制换行符。
    pub consumed_bytes: Range<usize>,
    /// Parley 计算的行内推进宽度，单位为 pt。
    pub advance_pt: f32,
    /// 推进宽度是否超过可用行宽。
    pub overflows_width: bool,
    pub metrics: LineMetrics,
}

/// 分页消费的段落行框。
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphLayout {
    pub lines: Vec<ParagraphLine>,
    pub total_height_pt: f32,
}

/// Parley 输出无法映射到合法 Caret UTF-8 行范围时拒绝布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphLayoutError {
    InvalidWidth,
    InvalidFontSize,
    InvalidInlineBox,
    InconsistentShaping,
    InvalidLineMetrics(LineMetricsError),
}

impl std::fmt::Display for ParagraphLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidWidth => formatter.write_str("layout.invalid_width"),
            Self::InvalidFontSize => formatter.write_str("layout.invalid_font_size"),
            Self::InvalidInlineBox => formatter.write_str("layout.invalid_inline_box"),
            Self::InconsistentShaping => formatter.write_str("layout.inconsistent_parley_output"),
            Self::InvalidLineMetrics(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ParagraphLayoutError {}

impl From<LineMetricsError> for ParagraphLayoutError {
    fn from(error: LineMetricsError) -> Self {
        Self::InvalidLineMetrics(error)
    }
}

fn is_mandatory_break_character(character: char) -> bool {
    matches!(
        character,
        '\n' | '\r' | '\u{000b}' | '\u{000c}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    )
}
