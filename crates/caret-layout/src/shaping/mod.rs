//! 使用 Parley Engine 塑形，并保留 Unicode 字节、字符、cluster 与 glyph 的对应关系。

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

use caret_doc::{InlineEnvironmentKind, InlineNode, Paragraph};
use parley_engine::itemize::Item;
use parley_engine::{
    Analysis, AnalysisOptions, Analyzer, BaseDirection, ShapeOptions, ShapedText, Shaper,
};

use crate::FontCatalog;
use crate::font_selection::{FontPreferenceRules, ResolvedDocumentTextSettings};

mod font_selector;
use font_selector::CatalogSelector;
mod unicode;
pub use unicode::UnicodeBoundaries;
use unicode::{is_chinese, uses_dictionary};

const CENTIPOINTS_TO_POINTS: f32 = 0.01;

/// 一个段落塑形请求的字体与语言设置。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapingStyle {
    /// 文档保存的请求字体族；None 表示由应用的系统默认文本字体提供。
    pub requested_family: Option<String>,
    /// 字号，单位为百分之一 pt；50 表示 0.5 pt。界面仍以 pt 输入和显示。
    pub font_size_centipoints: u16,
    /// 是否请求粗体字形。
    pub bold: bool,
    /// 是否请求斜体字形。
    pub italic: bool,
    /// BCP 47 语言标签。
    pub language: Option<String>,
}

impl Default for ShapingStyle {
    fn default() -> Self {
        Self {
            requested_family: None,
            font_size_centipoints: 1200,
            bold: false,
            italic: false,
            language: None,
        }
    }
}

/// 段落内容超出塑形引擎可表达的范围时返回错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapingError {
    /// Parley 的字符样式索引最多可表示 65,536 种样式。
    TooManyStyleRuns,
    /// Parley 使用 32 位字符位置，超过范围的文本不能塑形。
    TextTooLong,
    /// 字号的百分之一 pt 值必须大于零。
    InvalidFontSize,
    /// 重塑范围超出逻辑文本或落在 UTF-8 字符内部。
    InvalidTextRange,
    /// 字体后端未能塑形段落中的全部字符。
    IncompleteShaping,
}

impl std::fmt::Display for ShapingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyStyleRuns => formatter.write_str("layout.too_many_style_runs"),
            Self::TextTooLong => formatter.write_str("layout.text_too_long"),
            Self::InvalidFontSize => formatter.write_str("layout.invalid_font_size"),
            Self::InvalidTextRange => formatter.write_str("layout.invalid_text_range"),
            Self::IncompleteShaping => formatter.write_str("layout.incomplete_shaping"),
        }
    }
}

impl std::error::Error for ShapingError {}

#[derive(Debug)]
struct StyledSpan<'a> {
    text: &'a str,
    style: ShapingStyle,
}

/// 可复用的段落塑形结果。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShapedParagraph {
    /// 引擎生成的 runs、glyphs、clusters 与字符映射。
    pub text: ShapedText,
    /// Caret 使用的独立 Unicode 17 行断机会，字节偏移对应原文。
    pub line_breaks: Vec<usize>,
    /// 扩展字素簇边界，UTF-8 字节偏移。
    pub grapheme_boundaries: Vec<usize>,
    /// 各 shaped cluster 对应的 UTF-8 字节范围。
    pub cluster_bytes: Vec<Range<usize>>,
    /// 显式或文档字体偏好中至少一个族名未安装；缺字仍由 Fontique 系统回退处理。
    pub requested_family_missing: bool,
    /// 本段落实际使用的字体族，按首次使用顺序去重。
    pub actual_families: Vec<String>,
    /// 目录代际，供布局缓存键使用。
    pub font_generation: u64,
    /// 完整段落解析出的基方向。它不包含完整 UBA 双向状态，不能单独用于软换行后的行重塑。
    pub base_direction: BaseDirection,
}

/// 复用段落分析和 HarfRust shaper 缓存的轻量上下文。
#[derive(Debug, Default)]
pub struct TextShaper {
    analyzer: Analyzer,
    analysis: Analysis,
    shaper: Shaper,
    result: ShapedParagraph,
    style_indices: Vec<u16>,
    char_bytes: Vec<usize>,
}

impl TextShaper {
    /// 创建可复用塑形上下文。
    pub fn new() -> Self {
        Self::default()
    }

    /// 分析并塑形整个段落；传入目录在本调用中只进行字体查询，不重新扫描系统目录。
    pub fn shape(
        &mut self,
        catalog: &mut FontCatalog,
        text: &str,
        style: &ShapingStyle,
    ) -> Result<&ShapedParagraph, ShapingError> {
        self.shape_spans(
            catalog,
            &[StyledSpan {
                text,
                style: style.clone(),
            }],
        )
    }

    /// 使用文档级语言与字体偏好塑形独立文本，例如空段落标记的度量探针。
    pub fn shape_with_resolved_settings(
        &mut self,
        catalog: &mut FontCatalog,
        text: &str,
        settings: &ResolvedDocumentTextSettings,
        language_override: Option<&str>,
        base_style: &ShapingStyle,
    ) -> Result<&ShapedParagraph, ShapingError> {
        let mut style = base_style.clone();
        style.language = language_override
            .map(str::to_owned)
            .or_else(|| settings.default_language().map(str::to_owned))
            .or_else(|| base_style.language.clone());
        self.shape_spans_with_preferences(
            catalog,
            &[StyledSpan { text, style }],
            None,
            Some(settings.font_preferences()),
        )
    }

    /// 使用段落基方向塑形片段；基方向不等于完整 UBA 状态，因此不能用于软换行后的行重塑。
    pub fn shape_with_direction(
        &mut self,
        catalog: &mut FontCatalog,
        text: &str,
        style: &ShapingStyle,
        paragraph_direction: BaseDirection,
    ) -> Result<&ShapedParagraph, ShapingError> {
        self.shape_spans_with_direction(
            catalog,
            &[StyledSpan {
                text,
                style: style.clone(),
            }],
            Some(paragraph_direction),
        )
    }

    /// 展平 Paragraph 的 Run/行内环境生成派生样式片段，不修改文档节点树。
    pub fn shape_paragraph(
        &mut self,
        catalog: &mut FontCatalog,
        paragraph: &Paragraph,
        base_style: &ShapingStyle,
    ) -> Result<&ShapedParagraph, ShapingError> {
        let mut base = base_style.clone();
        base.language = paragraph
            .language
            .clone()
            .or_else(|| base_style.language.clone());
        let mut spans = Vec::new();
        append_inlines(&paragraph.inlines, &base, &mut spans);
        self.shape_spans(catalog, &spans)
    }

    /// 使用已解析文档设置塑形段落；Paragraph/Run 的完整 BCP 47 标签仍可覆盖默认值。
    pub fn shape_paragraph_with_resolved_settings(
        &mut self,
        catalog: &mut FontCatalog,
        paragraph: &Paragraph,
        settings: &ResolvedDocumentTextSettings,
        base_style: &ShapingStyle,
    ) -> Result<&ShapedParagraph, ShapingError> {
        let mut base = base_style.clone();
        base.language = paragraph.language.clone().or_else(|| {
            settings
                .default_language()
                .map(str::to_owned)
                .or_else(|| base_style.language.clone())
        });
        let mut spans = Vec::new();
        append_inlines(&paragraph.inlines, &base, &mut spans);
        self.shape_spans_with_preferences(catalog, &spans, None, Some(settings.font_preferences()))
    }

    /// 重塑一段 UTF-8 范围并指定段落基方向；不会保留原段落完整的 UBA 双向状态，
    /// 不能用作完整段落排版后的逐行重塑。
    pub fn shape_paragraph_range(
        &mut self,
        catalog: &mut FontCatalog,
        paragraph: &Paragraph,
        base_style: &ShapingStyle,
        byte_range: Range<usize>,
        paragraph_direction: BaseDirection,
    ) -> Result<&ShapedParagraph, ShapingError> {
        let mut base = base_style.clone();
        base.language = paragraph
            .language
            .clone()
            .or_else(|| base_style.language.clone());
        let mut full_spans = Vec::new();
        append_inlines(&paragraph.inlines, &base, &mut full_spans);
        let full_length = full_spans.iter().map(|span| span.text.len()).sum::<usize>();
        if byte_range.start > byte_range.end || byte_range.end > full_length {
            return Err(ShapingError::InvalidTextRange);
        }
        if !spans_char_boundary(&full_spans, byte_range.start)
            || !spans_char_boundary(&full_spans, byte_range.end)
        {
            return Err(ShapingError::InvalidTextRange);
        }
        let mut spans = Vec::new();
        let mut span_start = 0usize;
        for span in full_spans {
            let span_end = span_start + span.text.len();
            let start = byte_range.start.max(span_start);
            let end = byte_range.end.min(span_end);
            if start < end {
                let local_start = start - span_start;
                let local_end = end - span_start;
                if !span.text.is_char_boundary(local_start)
                    || !span.text.is_char_boundary(local_end)
                {
                    return Err(ShapingError::InvalidTextRange);
                }
                spans.push(StyledSpan {
                    text: &span.text[local_start..local_end],
                    style: span.style,
                });
            }
            span_start = span_end;
        }
        self.shape_spans_with_direction(catalog, &spans, Some(paragraph_direction))
    }

    /// 塑形段落范围并沿用已解析的文档语言和双向基方向；不会保留完整 UBA 双向状态，
    /// 不能用作完整段落排版后的逐行重塑。
    pub fn shape_paragraph_range_with_resolved_settings(
        &mut self,
        catalog: &mut FontCatalog,
        paragraph: &Paragraph,
        settings: &ResolvedDocumentTextSettings,
        base_style: &ShapingStyle,
        byte_range: Range<usize>,
        paragraph_direction: BaseDirection,
    ) -> Result<&ShapedParagraph, ShapingError> {
        let mut base = base_style.clone();
        base.language = paragraph.language.clone().or_else(|| {
            settings
                .default_language()
                .map(str::to_owned)
                .or_else(|| base_style.language.clone())
        });
        let mut full_spans = Vec::new();
        append_inlines(&paragraph.inlines, &base, &mut full_spans);
        let full_length = full_spans.iter().map(|span| span.text.len()).sum::<usize>();
        if byte_range.start > byte_range.end || byte_range.end > full_length {
            return Err(ShapingError::InvalidTextRange);
        }
        if !spans_char_boundary(&full_spans, byte_range.start)
            || !spans_char_boundary(&full_spans, byte_range.end)
        {
            return Err(ShapingError::InvalidTextRange);
        }
        let mut spans = Vec::new();
        let mut span_start = 0usize;
        for span in full_spans {
            let span_end = span_start + span.text.len();
            let start = byte_range.start.max(span_start);
            let end = byte_range.end.min(span_end);
            if start < end {
                let local_start = start - span_start;
                let local_end = end - span_start;
                spans.push(StyledSpan {
                    text: &span.text[local_start..local_end],
                    style: span.style,
                });
            }
            span_start = span_end;
        }
        self.shape_spans_with_preferences(
            catalog,
            &spans,
            Some(paragraph_direction),
            Some(settings.font_preferences()),
        )
    }

    fn shape_spans(
        &mut self,
        catalog: &mut FontCatalog,
        spans: &[StyledSpan<'_>],
    ) -> Result<&ShapedParagraph, ShapingError> {
        self.shape_spans_with_direction(catalog, spans, None)
    }

    fn shape_spans_with_direction(
        &mut self,
        catalog: &mut FontCatalog,
        spans: &[StyledSpan<'_>],
        base_direction: Option<BaseDirection>,
    ) -> Result<&ShapedParagraph, ShapingError> {
        self.shape_spans_with_preferences(catalog, spans, base_direction, None)
    }

    fn shape_spans_with_preferences(
        &mut self,
        catalog: &mut FontCatalog,
        spans: &[StyledSpan<'_>],
        base_direction: Option<BaseDirection>,
        preferences: Option<&FontPreferenceRules>,
    ) -> Result<&ShapedParagraph, ShapingError> {
        let mut styles = Vec::<ShapingStyle>::new();
        let mut style_indices_by_value = HashMap::<ShapingStyle, usize>::new();
        let mut item_ends = Vec::<(u32, usize)>::new();
        let mut dictionary_ranges = Vec::<Range<usize>>::new();
        let mut chinese_ranges = Vec::<Range<usize>>::new();
        self.style_indices.clear();
        let mut char_count = 0usize;
        let mut byte_count = 0usize;
        for span in spans {
            if span.style.font_size_centipoints == 0 {
                return Err(ShapingError::InvalidFontSize);
            }
            let byte_end = byte_count
                .checked_add(span.text.len())
                .ok_or(ShapingError::TextTooLong)?;
            if !span.text.is_empty() && span.style.language.as_deref().is_some_and(uses_dictionary)
            {
                if let Some(previous) = dictionary_ranges.last_mut()
                    && previous.end == byte_count
                {
                    previous.end = byte_end;
                } else {
                    dictionary_ranges.push(byte_count..byte_end);
                }
            }
            if !span.text.is_empty() && span.style.language.as_deref().is_some_and(is_chinese) {
                if let Some(previous) = chinese_ranges.last_mut()
                    && previous.end == byte_count
                {
                    previous.end = byte_end;
                } else {
                    chinese_ranges.push(byte_count..byte_end);
                }
            }
            byte_count = byte_end;
            let style_index = match style_indices_by_value.get(&span.style) {
                Some(index) => *index,
                None => {
                    let index = styles.len();
                    if index > u16::MAX as usize {
                        return Err(ShapingError::TooManyStyleRuns);
                    }
                    styles.push(span.style.clone());
                    style_indices_by_value.insert(span.style.clone(), index);
                    index
                }
            };
            let span_char_count = span.text.chars().count();
            if span_char_count == 0 {
                continue;
            }
            char_count = char_count
                .checked_add(span_char_count)
                .ok_or(ShapingError::TextTooLong)?;
            let char_end = u32::try_from(char_count).map_err(|_| ShapingError::TextTooLong)?;
            let style_index =
                u16::try_from(style_index).map_err(|_| ShapingError::TooManyStyleRuns)?;
            self.style_indices
                .extend(std::iter::repeat_n(style_index, span_char_count));
            if let Some((previous_end, previous_style)) = item_ends.last_mut()
                && *previous_style == style_index as usize
            {
                // Run 边界不应中断相同有效样式的连字、字形连接或字素塑形。
                *previous_end = char_end;
            } else {
                item_ends.push((char_end, style_index as usize));
            }
        }
        let text = if let [span] = spans {
            Cow::Borrowed(span.text)
        } else {
            Cow::Owned(spans.iter().map(|span| span.text).collect())
        };
        self.analyzer.analyze(
            &text,
            &AnalysisOptions {
                base_direction: base_direction.unwrap_or(BaseDirection::Auto),
                ..AnalysisOptions::default()
            },
            &mut self.analysis,
        );
        self.result.base_direction = if self.analysis.paragraph_level().is_rtl() {
            BaseDirection::Rtl
        } else {
            BaseDirection::Ltr
        };
        let items: Vec<_> = item_ends
            .iter()
            .map(|(char_end, style_index)| {
                let style = &styles[*style_index];
                Item {
                    char_end: *char_end,
                    options: ShapeOptions {
                        font_size: f32::from(style.font_size_centipoints) * CENTIPOINTS_TO_POINTS,
                        language: style
                            .language
                            .as_deref()
                            .and_then(|tag| fontique::Language::parse(tag).ok()),
                        features: &[],
                        variations: &[],
                    },
                }
            })
            .collect();
        let mut selector = CatalogSelector::new(catalog, &styles, preferences);

        if char_count > 0 {
            self.shaper.shape_text(
                &text,
                &self.analysis,
                &self.style_indices,
                items,
                &mut selector,
                &mut self.result.text,
            );
        } else {
            self.result.text.clear();
        }
        if self.result.text.characters().len() != char_count {
            self.result.text.clear();
            return Err(ShapingError::IncompleteShaping);
        }

        self.result.cluster_bytes.clear();
        self.char_bytes.clear();
        self.char_bytes
            .extend(text.char_indices().map(|(offset, _)| offset));
        self.char_bytes.push(text.len());
        for cluster in self.result.text.shaped_clusters() {
            let start = cluster.chars_range().start as usize;
            let end = cluster.chars_range().end as usize;
            self.result
                .cluster_bytes
                .push(self.char_bytes[start]..self.char_bytes[end]);
        }
        UnicodeBoundaries::fill(
            &text,
            &mut self.result.grapheme_boundaries,
            &mut self.result.line_breaks,
            &dictionary_ranges,
            &chinese_ranges,
        );
        self.result.requested_family_missing = selector.requested_family_missing;
        self.result
            .actual_families
            .clone_from(&selector.actual_families);
        self.result.font_generation = selector.font_generation;
        Ok(&self.result)
    }
}

fn spans_char_boundary(spans: &[StyledSpan<'_>], offset: usize) -> bool {
    if offset == 0 {
        return true;
    }
    let mut span_start = 0usize;
    for span in spans {
        let span_end = span_start + span.text.len();
        if (span_start..=span_end).contains(&offset) {
            return span.text.is_char_boundary(offset - span_start);
        }
        span_start = span_end;
    }
    false
}

/// 返回段落的逻辑 UTF-8 文本；Run 和环境边界不插入字符，软换行与不透明节点各映射为一个占位码点。
pub fn paragraph_text(paragraph: &Paragraph) -> String {
    fn append(nodes: &[InlineNode], text: &mut String) {
        for node in nodes {
            match node {
                InlineNode::Run(run) => text.push_str(&run.text),
                InlineNode::Environment(environment) => append(&environment.children, text),
                InlineNode::SoftBreak(_) => text.push('\n'),
                InlineNode::Opaque(_) => text.push('\u{fffc}'),
            }
        }
    }
    let mut text = String::new();
    append(&paragraph.inlines, &mut text);
    text
}

fn append_inlines<'a>(
    nodes: &'a [InlineNode],
    style: &ShapingStyle,
    output: &mut Vec<StyledSpan<'a>>,
) {
    for node in nodes {
        match node {
            InlineNode::Run(run) => {
                let mut run_style = style.clone();
                if let Some(language) = &run.language {
                    run_style.language = Some(language.clone());
                }
                if !run.text.is_empty() {
                    output.push(StyledSpan {
                        text: &run.text,
                        style: run_style,
                    });
                }
            }
            InlineNode::Environment(environment) => {
                let mut nested = style.clone();
                match environment.kind {
                    InlineEnvironmentKind::Bold => {
                        nested.bold = environment
                            .props
                            .get("enabled")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(true);
                    }
                    InlineEnvironmentKind::Italic => {
                        nested.italic = environment
                            .props
                            .get("enabled")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(true);
                    }
                    InlineEnvironmentKind::Font => {
                        if let Some(family) = environment
                            .props
                            .get("fontFamily")
                            .and_then(serde_json::Value::as_str)
                        {
                            nested.requested_family = Some(family.to_owned());
                        }
                    }
                }
                append_inlines(&environment.children, &nested, output);
            }
            InlineNode::SoftBreak(_) => output.push(StyledSpan {
                text: "\n",
                style: style.clone(),
            }),
            InlineNode::Opaque(_) => output.push(StyledSpan {
                text: "\u{fffc}",
                style: style.clone(),
            }),
        }
    }
}
