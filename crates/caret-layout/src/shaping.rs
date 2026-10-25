//! 使用 Parley Engine 塑形，并保留 Unicode 字节、字符、cluster 与 glyph 的对应关系。

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use caret_doc::{InlineEnvironmentKind, InlineNode, Paragraph};
use fontique::{
    Attributes, FallbackKey, FontStyle, FontWeight, FontWidth, GenericFamily, QueryFont,
    QueryStatus,
};
use icu_segmenter::{LineSegmenter, LineSegmenterBorrowed};
use parley_engine::itemize::{Item, Segment};
use parley_engine::shape::CharCluster;
use parley_engine::{
    Analysis, AnalysisDataSources, AnalysisOptions, Analyzer, FontData, FontInstance,
    FontInstanceRef, FontSelector, ShapeOptions, ShapedText, Shaper,
};

use crate::FontCatalog;

const CENTIPOINTS_TO_POINTS: f32 = 0.01;

thread_local! {
    static LINE_SEGMENTER: RefCell<LineSegmenterBorrowed<'static>> = RefCell::new({
        LineSegmenter::new_17_for_non_complex_scripts(Default::default())
    });
    static DICTIONARY_LINE_SEGMENTER: RefCell<LineSegmenterBorrowed<'static>> = RefCell::new({
        let mut segmenter = LineSegmenter::new_17_for_non_complex_scripts(Default::default());
        segmenter.load_dictionary();
        segmenter
    });
}

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
    /// 字体后端未能塑形段落中的全部字符。
    IncompleteShaping,
}

impl std::fmt::Display for ShapingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyStyleRuns => formatter.write_str("layout.too_many_style_runs"),
            Self::TextTooLong => formatter.write_str("layout.text_too_long"),
            Self::InvalidFontSize => formatter.write_str("layout.invalid_font_size"),
            Self::IncompleteShaping => formatter.write_str("layout.incomplete_shaping"),
        }
    }
}

impl std::error::Error for ShapingError {}

#[derive(Debug)]
struct StyledSpan<'a> {
    text: Cow<'a, str>,
    style: ShapingStyle,
}

/// 由固定 Unicode 17 数据计算的段落字素与行断边界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnicodeBoundaries {
    /// 扩展字素簇起止位置，使用 UTF-8 字节偏移，含 0 与字符串长度。
    pub grapheme: Vec<usize>,
    /// UAX #14 行断机会，使用 UTF-8 字节偏移，含 0 与字符串长度。
    pub line_break: Vec<usize>,
}

impl UnicodeBoundaries {
    /// 对原文计算 Unicode 17 字素和行断边界，不规范化输入。
    pub fn new(text: &str) -> Self {
        Self::new_with_language(text, None)
    }

    /// 按文档语言计算边界；泰语、老挝语、高棉语和缅甸语使用 ICU4X 内置词典。
    pub fn new_with_language(text: &str, language: Option<&str>) -> Self {
        let mut grapheme = Vec::new();
        let mut line_break = Vec::new();
        let dictionary_ranges = if language.is_some_and(uses_dictionary) && !text.is_empty() {
            std::iter::once(0..text.len()).collect()
        } else {
            Vec::new()
        };
        Self::fill(text, &mut grapheme, &mut line_break, &dictionary_ranges);
        Self {
            grapheme,
            line_break,
        }
    }

    fn fill(
        text: &str,
        grapheme: &mut Vec<usize>,
        line_break: &mut Vec<usize>,
        dictionary_ranges: &[Range<usize>],
    ) {
        use unicode_segmentation::UnicodeSegmentation;

        grapheme.clear();
        grapheme.push(0);
        grapheme.extend(
            text.grapheme_indices(true)
                .skip(1)
                .map(|(offset, _)| offset),
        );
        if grapheme.last().copied() != Some(text.len()) {
            grapheme.push(text.len());
        }
        line_break.clear();
        let line_break_text = replace_complex_context_for_line_break(text);
        let mut generic_breaks = Vec::new();
        LINE_SEGMENTER.with(|segmenter| {
            generic_breaks.extend(segmenter.borrow().segment_str(&line_break_text));
        });
        if dictionary_ranges.is_empty() {
            line_break.extend(generic_breaks);
        } else {
            let mut dictionary_breaks = Vec::new();
            DICTIONARY_LINE_SEGMENTER.with(|segmenter| {
                dictionary_breaks.extend(segmenter.borrow().segment_str(text));
            });
            line_break.extend(
                generic_breaks
                    .into_iter()
                    .filter(|boundary| !is_inside_dictionary_range(*boundary, dictionary_ranges)),
            );
            line_break.extend(dictionary_breaks.into_iter().filter(|boundary| {
                is_inside_dictionary_range(*boundary, dictionary_ranges)
                    && grapheme.binary_search(boundary).is_ok()
            }));
            line_break.sort_unstable();
            line_break.dedup();
        }
        if line_break.first().copied() != Some(0) {
            line_break.insert(0, 0);
        }
        if line_break.last().copied() != Some(text.len()) {
            line_break.push(text.len());
        }
    }
}

/// 在有序、不重叠的词典语言范围中二分查询断点归属。
fn is_inside_dictionary_range(boundary: usize, ranges: &[Range<usize>]) -> bool {
    let index = ranges.partition_point(|range| range.end <= boundary);
    ranges
        .get(index)
        .is_some_and(|range| range.start < boundary && boundary < range.end)
}

/// 按 UAX #14 LB1 将 SA 映射为 CM 或 AL，且保持每个码点的 UTF-8 长度以复用偏移。
fn replace_complex_context_for_line_break(text: &str) -> Cow<'_, str> {
    use icu_properties::{
        CodePointMapData,
        props::{GeneralCategory, LineBreak},
    };

    let line_break = CodePointMapData::<LineBreak>::new();
    if !text
        .chars()
        .any(|character| line_break.get(character) == LineBreak::ComplexContext)
    {
        return Cow::Borrowed(text);
    }
    let general_category = CodePointMapData::<GeneralCategory>::new();
    Cow::Owned(
        text.chars()
            .map(|character| {
                if line_break.get(character) != LineBreak::ComplexContext {
                    return character;
                }
                let is_mark = matches!(
                    general_category.get(character),
                    GeneralCategory::NonspacingMark | GeneralCategory::SpacingMark
                );
                match (is_mark, character.len_utf8()) {
                    (true, 2) => '\u{0300}',
                    (true, 3) => '\u{081c}',
                    (true, 4) => '\u{101fd}',
                    (false, 2) => 'À',
                    (false, 3) => '\u{0800}',
                    (false, 4) => '\u{10000}',
                    _ => character,
                }
            })
            .collect(),
    )
}

fn uses_dictionary(language: &str) -> bool {
    matches!(
        language
            .split('-')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "th" | "lo" | "km" | "my"
    )
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
    /// 是否请求的族名未安装；为 true 时结果来自系统 fallback。
    pub requested_family_missing: bool,
    /// 本段落实际使用的字体族，按首次使用顺序去重。
    pub actual_families: Vec<String>,
    /// 目录代际，供布局缓存键使用。
    pub font_generation: u64,
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
                text: Cow::Borrowed(text),
                style: style.clone(),
            }],
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

    fn shape_spans(
        &mut self,
        catalog: &mut FontCatalog,
        spans: &[StyledSpan<'_>],
    ) -> Result<&ShapedParagraph, ShapingError> {
        let mut styles = Vec::<ShapingStyle>::new();
        let mut style_indices_by_value = HashMap::<ShapingStyle, usize>::new();
        let mut item_ends = Vec::<(u32, usize)>::new();
        let mut dictionary_ranges = Vec::<Range<usize>>::new();
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
            Cow::Borrowed(span.text.as_ref())
        } else {
            Cow::Owned(spans.iter().map(|span| span.text.as_ref()).collect())
        };
        self.analyzer
            .analyze(&text, &AnalysisOptions::default(), &mut self.analysis);
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
        let mut selector = CatalogSelector::new(catalog, &styles);

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
        );
        self.result.requested_family_missing = selector.requested_family_missing;
        self.result
            .actual_families
            .clone_from(&selector.actual_families);
        self.result.font_generation = selector.font_generation;
        Ok(&self.result)
    }
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
                        text: Cow::Borrowed(&run.text),
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
                text: Cow::Borrowed("\n"),
                style: style.clone(),
            }),
            InlineNode::Opaque(_) => output.push(StyledSpan {
                text: Cow::Borrowed("\u{fffc}"),
                style: style.clone(),
            }),
        }
    }
}

struct CatalogSelector<'a> {
    query: fontique::Query<'a>,
    styles: &'a [ShapingStyle],
    active_style_index: Option<u16>,
    active_emoji_mode: Option<bool>,
    selected: Option<FontInstance>,
    data_sources: AnalysisDataSources,
    requested_family_missing: bool,
    actual_families: Vec<String>,
    family_names: Arc<HashMap<fontique::FamilyId, String>>,
    available_family_names: Arc<HashSet<String>>,
    font_generation: u64,
}

impl<'a> CatalogSelector<'a> {
    fn new(catalog: &'a mut FontCatalog, styles: &'a [ShapingStyle]) -> Self {
        let font_generation = catalog.generation();
        let family_names = Arc::clone(&catalog.family_names_by_id);
        let available_family_names = Arc::clone(&catalog.family_names_lowercase);
        let query = catalog.collection.query(&mut catalog.source_cache);
        Self {
            query,
            styles,
            active_style_index: None,
            active_emoji_mode: None,
            selected: None,
            data_sources: AnalysisDataSources::new(),
            requested_family_missing: false,
            actual_families: Vec::new(),
            family_names,
            available_family_names,
            font_generation,
        }
    }

    fn query_font(&mut self, cluster: &mut CharCluster) -> Option<QueryFont> {
        let mut best: Option<(parley_engine::shape::Coverage, QueryFont)> = None;
        self.query.matches_with(|font| {
            let coverage = cluster.calculate_coverage(
                |character| {
                    font.charmap()
                        .is_some_and(|charmap| charmap.map(character).is_some())
                },
                &self.data_sources,
            );
            if best
                .as_ref()
                .is_none_or(|(best_coverage, _)| coverage > *best_coverage)
            {
                best = Some((coverage, font.clone()));
            }
            if coverage.is_complete() {
                QueryStatus::Stop
            } else {
                QueryStatus::Continue
            }
        });
        best.map(|(_, font)| font)
    }
}

impl FontSelector for CatalogSelector<'_> {
    fn select_font(
        &mut self,
        segment: &Segment,
        options: &ShapeOptions<'_>,
        cluster: &mut CharCluster,
    ) -> Option<FontInstanceRef<'_>> {
        let style_index = cluster.chars().first()?.style_index;
        let emoji_mode = cluster.is_emoji();
        if self.active_style_index != Some(style_index)
            || self.active_emoji_mode != Some(emoji_mode)
        {
            let style = self.styles.get(style_index as usize)?;
            let mut families = Vec::with_capacity(2);
            if emoji_mode {
                families.push(GenericFamily::Emoji.into());
            }
            if let Some(family) = style.requested_family.as_deref() {
                families.push(fontique::QueryFamily::from(family));
                self.requested_family_missing |=
                    !self.available_family_names.contains(&family.to_lowercase());
            }
            if style.requested_family.is_none() || emoji_mode {
                families.push(GenericFamily::SystemUi.into());
            }
            self.query.set_families(families);
            self.query.set_attributes(Attributes::new(
                FontWidth::default(),
                if style.italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                },
                if style.bold {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                },
            ));
            self.active_style_index = Some(style_index);
            self.active_emoji_mode = Some(emoji_mode);
        }
        self.query
            .set_fallbacks(FallbackKey::new(segment.script, options.language.as_ref()));
        let font = self.query_font(cluster)?;
        if let Some(name) = self.family_names.get(&font.family.0)
            && !self.actual_families.iter().any(|previous| previous == name)
        {
            self.actual_families.push(name.clone());
        }
        self.selected = Some(FontInstance {
            font: FontData {
                data: font.blob,
                index: font.index,
            },
            synthesis: font.synthesis,
        });
        self.selected.as_ref().map(FontInstance::as_ref)
    }
}

impl FontSelector for &mut CatalogSelector<'_> {
    fn begin_segment(&mut self, segment: &Segment, options: &ShapeOptions<'_>) {
        CatalogSelector::begin_segment(self, segment, options);
    }

    fn select_font(
        &mut self,
        segment: &Segment,
        options: &ShapeOptions<'_>,
        cluster: &mut CharCluster,
    ) -> Option<FontInstanceRef<'_>> {
        CatalogSelector::select_font(self, segment, options, cluster)
    }
}
