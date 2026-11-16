//! 用 Parley 高层 Layout 生成段落塑形、行断和视觉几何，并映射回 Caret 行框。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{
    borrow::Cow,
    collections::{HashSet, VecDeque},
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use caret_doc::{DocumentUnits, Paragraph};
use icu_locale_core::subtags::Script;
use icu_properties::{CodePointMapData, CodePointSetDataBorrowed, PropertyNamesShort, props};
use parley::{
    Alignment, AlignmentOptions, FontFamilyName, GenericFamily, IndentOptions, InlineBox,
    InlineBoxKind, Language, Layout, LayoutContext, StyleProperty, VerticalAlign,
    WhiteSpaceCollapse,
};

use crate::{
    FontCatalog, FontLineMetrics, InlineObjectLayout, LineSpacing, ParagraphLayout,
    ParagraphLayoutError, ResolvedDocumentTextSettings, ShapingStyle,
    shaping::{boundaries_for_spans, is_mandatory_line_break},
};

use crate::shaping::{StyledSpan, paragraph_style_spans};

mod interaction;
mod line_mapping;

use line_mapping::{ParagraphLineMapping, font_line_metrics, make_paragraph_lines};

/// 视觉光标停靠点的逻辑侧，与文档环境亲和性相互独立。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualAffinity {
    Upstream,
    Downstream,
}

/// 与具体布局无关的稳定视觉光标停靠点描述。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualCursor {
    byte_index: usize,
    affinity: VisualAffinity,
}

/// 文本命中落在逻辑字素簇前侧或后侧的语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitAffinity {
    Upstream,
    Downstream,
}

/// 同时保留视觉光标位置和文档输入继承侧的文本命中结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParagraphHit {
    pub cursor: VisualCursor,
    pub logical_affinity: HitAffinity,
}

impl VisualCursor {
    pub const fn byte_index(self) -> usize {
        self.byte_index
    }

    pub const fn affinity(self) -> VisualAffinity {
        self.affinity
    }
}

/// Parley 的完整段落布局及 Caret 行框/Unicode 映射。
#[derive(Debug, Clone)]
pub struct ParleyParagraphLayout {
    /// 绘制、命中和选择几何的唯一 shaping/layout 结果。
    pub layout: Layout<[u8; 4]>,
    /// Caret 分页消费的行范围和自定义行框。
    pub paragraph: ParagraphLayout,
    /// 逻辑 UTF-8 中的扩展字素簇边界。
    pub grapheme_boundaries: Vec<usize>,
    /// 每条行框顶端相对 Parley 自身行框顶端的垂直平移。
    pub line_top_offsets_pt: Vec<f32>,
    /// 每条 Caret 行框在段落中的顶端，用于二分查找命中行。
    pub line_box_tops_pt: Vec<f32>,
    /// Parley 原始行顶端，用于快速映射光标矩形。
    pub parley_line_tops_pt: Vec<f32>,
    /// Parley 原始行高；将点击采样限制在目标原始行内部。
    pub parley_line_heights_pt: Vec<f32>,
    /// 每条行框的文档级基线锚点相对 Parley baseline 的调整量。
    pub baseline_adjustments_pt: Vec<f32>,
    /// 空段落首行的缩进位置，用于放置没有字形锚点的插入点。
    pub empty_line_indent_pt: f32,
    /// 文档字体偏好中至少一个候选族未安装。
    pub requested_family_missing: bool,
    /// 首选候选族缺失时记录其族名，供公式布局诊断精确说明回退对象。
    pub missing_requested_families: Vec<String>,
    /// Parley 为至少一个字符发出 `.notdef` 字形；与数学文本岛共用检测逻辑。
    pub has_missing_glyph: bool,
    /// Parley inline-box ID 与逻辑 U+FFFC 源范围的映射，供命中和选区几何使用。
    inline_object_source_ranges: Vec<(u64, Range<usize>)>,
}

/// 段落布局的版心宽度、行距和共享行网格度量。
#[derive(Debug, Clone, Copy)]
pub struct ParagraphLayoutOptions {
    pub width_pt: f32,
    pub spacing: LineSpacing,
    /// 来自文档/段落基础样式字体的行距网格，不随正文语言或 Script 改变。
    pub line_grid_metrics: FontLineMetrics,
    /// 段落默认字号；同字号的脚本 fallback 不得扩大行距网格。
    pub line_grid_font_size_centipoints: u16,
    /// 首行缩进的字符数；字符宽度按段落基础字号的 em 计算。
    pub first_line_indent_chars: Option<u16>,
}

/// 可复用 Parley LayoutContext；字体目录和字体数据由 FontCatalog 共享。
#[derive(Default)]
pub struct ParagraphLayouter {
    context: LayoutContext<[u8; 4]>,
    shaping_cache: TextShapingCache,
}

/// 一个连续文本岛的样式范围；边界不会阻断组合字符或连字塑形。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextIslandSpan<'a> {
    pub text: &'a str,
    pub style: ShapingStyle,
    pub underline: bool,
}

/// 文本岛自然字体度量和最大水平推进，单位为 pt。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextIslandMetrics {
    pub advance_pt: f32,
    pub ascent_pt: f32,
    pub descent_pt: f32,
}

/// Parley 原始文本岛塑形结果；显式 mandatory break 可形成多行，不包含分页数据。
#[derive(Debug, Clone)]
pub struct TextIslandLayout {
    pub text: String,
    pub layout: Layout<[u8; 4]>,
    pub metrics: TextIslandMetrics,
    /// 输入文本的扩展字素簇边界，包含 0 和文本末尾。
    pub grapheme_boundaries: Vec<usize>,
    /// 字体偏好首选族未安装；generic family 不会触发此诊断。
    pub requested_family_missing: bool,
    /// 缺失的非 generic 首选族名。
    pub missing_requested_families: Vec<String>,
    /// Parley 为至少一个字符发出 `.notdef` 字形。
    pub has_missing_glyph: bool,
}

/// 文本岛塑形失败；字号非法或 Parley 输出不一致时返回错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextIslandError {
    InvalidFontSize,
    InconsistentShaping,
}

impl std::fmt::Display for TextIslandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidFontSize => "layout.invalid_font_size",
            Self::InconsistentShaping => "layout.inconsistent_parley_output",
        })
    }
}

impl std::error::Error for TextIslandError {}

/// 正文和文本岛共享空格校准缓存与 Parley 上下文。
#[derive(Default)]
struct TextShapingCache {
    font_generation: Option<u64>,
    space_adjustments_pt: std::collections::HashMap<
        SpaceStyleSignature,
        std::collections::HashMap<SpaceStyleKey, f32>,
    >,
    /// 保留正文与公式样式的近期缓存，避免长文档为每个签名无限留存校准数据。
    space_style_order: VecDeque<SpaceStyleSignature>,
    space_font_preferences: Option<crate::FontPreferenceRules>,
}

type SpaceStyleSignature = (Vec<ShapingStyle>, Option<u32>);
const SPACE_STYLE_CACHE_CAPACITY: usize = 8;

impl TextShapingCache {
    fn prepare(
        &mut self,
        catalog: &FontCatalog,
        settings: &ResolvedDocumentTextSettings,
        styles: Vec<ShapingStyle>,
        font_size_override_pt: Option<f32>,
        context: &mut LayoutContext<[u8; 4]>,
    ) -> SpaceStyleSignature {
        let generation = catalog.generation();
        if self.font_generation != Some(generation) {
            *context = LayoutContext::default();
            self.font_generation = Some(generation);
            self.space_adjustments_pt.clear();
            self.space_style_order.clear();
        }
        if self.space_font_preferences.as_ref() != Some(settings.font_preferences()) {
            self.space_font_preferences = Some(settings.font_preferences().clone());
            self.space_adjustments_pt.clear();
            self.space_style_order.clear();
        }
        (styles, font_size_override_pt.map(f32::to_bits))
    }

    fn space_adjustments_for(
        &mut self,
        signature: SpaceStyleSignature,
    ) -> &mut std::collections::HashMap<SpaceStyleKey, f32> {
        if let Some(index) = self
            .space_style_order
            .iter()
            .position(|cached| cached == &signature)
        {
            self.space_style_order.remove(index);
        } else if self.space_style_order.len() == SPACE_STYLE_CACHE_CAPACITY
            && let Some(oldest) = self.space_style_order.pop_front()
        {
            self.space_adjustments_pt.remove(&oldest);
        }
        self.space_style_order.push_back(signature.clone());
        self.space_adjustments_pt.entry(signature).or_default()
    }
}

impl ParagraphLayouter {
    /// 将连续 spans 拼成一个 UTF-8 文本后整体塑形，跨 span 的 grapheme 与连字保持完整。
    /// 空输入返回零度量的空布局；mandatory break 按正文 shaping 规则形成视觉行。
    pub fn shape_text_island(
        &mut self,
        catalog: &mut FontCatalog,
        settings: &ResolvedDocumentTextSettings,
        spans: &[TextIslandSpan<'_>],
    ) -> Result<TextIslandLayout, TextIslandError> {
        self.shape_text_island_with_font_size_pt(catalog, settings, spans, None)
    }

    /// 使用布局派生的 pt 字号塑形；与文档存储字号不同，此值保留脚本缩放的小数精度。
    pub fn shape_text_island_at_font_size_pt(
        &mut self,
        catalog: &mut FontCatalog,
        settings: &ResolvedDocumentTextSettings,
        spans: &[TextIslandSpan<'_>],
        font_size_pt: f32,
    ) -> Result<TextIslandLayout, TextIslandError> {
        self.shape_text_island_with_font_size_pt(catalog, settings, spans, Some(font_size_pt))
    }

    fn shape_text_island_with_font_size_pt(
        &mut self,
        catalog: &mut FontCatalog,
        settings: &ResolvedDocumentTextSettings,
        spans: &[TextIslandSpan<'_>],
        font_size_override_pt: Option<f32>,
    ) -> Result<TextIslandLayout, TextIslandError> {
        if font_size_override_pt.is_some_and(|size| !size.is_finite() || size <= 0.0) {
            return Err(TextIslandError::InvalidFontSize);
        }
        for span in spans {
            if font_size_override_pt.is_none()
                && !span.text.is_empty()
                && span.style.font_size_centipoints == 0
            {
                return Err(TextIslandError::InvalidFontSize);
            }
        }
        let styled_spans: Vec<_> = spans
            .iter()
            .filter(|span| !span.text.is_empty())
            .map(|span| StyledSpan {
                text: span.text,
                style: span.style.clone(),
                underline: span.underline,
            })
            .collect();
        let Some(base_style) = styled_spans.first().map(|span| &span.style) else {
            // 空文本岛不进入 Parley：返回零度量的默认布局，也不触碰共享塑形缓存。
            return Ok(TextIslandLayout {
                text: String::new(),
                layout: Layout::default(),
                metrics: TextIslandMetrics {
                    advance_pt: 0.0,
                    ascent_pt: 0.0,
                    descent_pt: 0.0,
                },
                grapheme_boundaries: vec![0],
                requested_family_missing: false,
                missing_requested_families: Vec::new(),
                has_missing_glyph: false,
            });
        };
        let PreparedShaping {
            text,
            layout,
            grapheme_boundaries,
            requested_family_missing,
            missing_requested_families,
            has_missing_glyph,
        } = self
            .prepare_shaping(
                catalog,
                settings,
                &styled_spans,
                base_style,
                font_size_override_pt,
                PreparedLayoutOptions {
                    width_pt: None,
                    first_line_indent_chars: None,
                    inline_objects: &[],
                },
            )
            .map_err(|_| TextIslandError::InconsistentShaping)?;
        let mut lines = layout.lines();
        let first_line = lines.next().ok_or(TextIslandError::InconsistentShaping)?;
        let first_baseline = first_line.metrics().baseline;
        let mut min_content_top = first_line.metrics().content_block_min_coord;
        let mut max_content_bottom = first_line.metrics().content_block_max_coord;
        let mut max_advance = first_line.metrics().advance;
        for line in lines {
            let line_metrics = line.metrics();
            min_content_top = min_content_top.min(line_metrics.content_block_min_coord);
            max_content_bottom = max_content_bottom.max(line_metrics.content_block_max_coord);
            max_advance = max_advance.max(line_metrics.advance);
        }
        let mut metrics = TextIslandMetrics {
            advance_pt: max_advance,
            ascent_pt: first_baseline - min_content_top,
            descent_pt: max_content_bottom - first_baseline,
        };
        if !metrics.advance_pt.is_finite()
            || metrics.advance_pt < 0.0
            || !metrics.ascent_pt.is_finite()
            || metrics.ascent_pt < 0.0
            || !metrics.descent_pt.is_finite()
            || metrics.descent_pt < 0.0
        {
            return Err(TextIslandError::InconsistentShaping);
        }
        // 归一化负零，同时保留自然字形度量。
        metrics.advance_pt = metrics.advance_pt.max(0.0);
        Ok(TextIslandLayout {
            text,
            layout,
            metrics,
            grapheme_boundaries,
            requested_family_missing,
            missing_requested_families,
            has_missing_glyph,
        })
    }
}

/// 正文段落和公式文本岛共用同一条 Parley `.notdef` 检查路径。
fn layout_has_missing_glyph(layout: &Layout<[u8; 4]>) -> bool {
    layout
        .lines()
        .flat_map(|line| line.items())
        .any(|item| match item {
            parley::PositionedLayoutItem::GlyphRun(run) => run.glyphs().any(|glyph| glyph.id == 0),
            parley::PositionedLayoutItem::InlineBox(_) => false,
        })
}

fn is_inline_object_boundary(ranges: &[Range<usize>], offset: usize) -> bool {
    ranges
        .binary_search_by_key(&offset, |range| range.start)
        .is_ok()
        || ranges
            .binary_search_by_key(&offset, |range| range.end)
            .is_ok()
}

fn is_inline_object_range(ranges: &[Range<usize>], candidate: &Range<usize>) -> bool {
    ranges
        .binary_search_by_key(&candidate.start, |range| range.start)
        .is_ok_and(|index| ranges[index] == *candidate)
}

/// 将 Parley 不识别的 mandatory break 映射为等字节长度的换行序列，保持所有源范围偏移。
fn normalize_mandatory_breaks(text: &str) -> Cow<'_, str> {
    if !text
        .chars()
        .any(|character| matches!(character, '\u{000b}' | '\u{000c}' | '\u{0085}'))
    {
        return Cow::Borrowed(text);
    }
    let mut normalized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            // 避免源文本中相邻但独立的 CR 和 VT/FF 被替换成单个 CRLF。
            '\r' if matches!(characters.peek(), Some('\u{000b}' | '\u{000c}')) => {
                normalized.push('\n');
            }
            '\u{000b}' | '\u{000c}' => normalized.push('\n'),
            '\u{0085}' => normalized.push_str("\r\n"),
            _ => normalized.push(character),
        }
    }
    Cow::Owned(normalized)
}

/// ASCII 与显式全字宽空格分别使用可观察的字符语义，但都按实际字体宽度校准。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SpaceKind {
    Ordinary,
    Em,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SpaceStyleKey {
    style_index: usize,
    script: Script,
    emoji: bool,
    kind: SpaceKind,
}

const ORDINARY_SPACE_EM_PERCENT: u16 = 50;
const EM_SPACE_EM_PERCENT: u16 = 100;

impl ParagraphLayouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 以文档/段落语言、行内环境、脚本字体偏好和 ICU 断点构建单一 Parley 布局。
    pub fn layout(
        &mut self,
        catalog: &mut FontCatalog,
        paragraph: &Paragraph,
        settings: &ResolvedDocumentTextSettings,
        base_style: &ShapingStyle,
        options: ParagraphLayoutOptions,
    ) -> Result<(String, ParleyParagraphLayout), ParagraphLayoutError> {
        self.layout_with_inline_objects(catalog, paragraph, settings, base_style, options, &[])
    }

    /// 使用同一 Parley 布局排正文和行内对象。
    ///
    /// `inline_objects` 的 ID 必须唯一，源范围不得重复或重叠且必须恰好覆盖逻辑文本中的一个 U+FFFC；盒推进、上升和下降度量必须有限且非负。塑形时以等 UTF-8 长度的零宽占位符代替 U+FFFC，所以 Parley 的字节索引仍直接对应返回的逻辑文本，只有 inline box 为对象提供推进。
    ///
    /// 成功时返回逻辑源文本及其 Parley 塑形布局。宽度、字号、对象范围或对象度量无效，以及塑形结果无法映射回源文本时返回 `ParagraphLayoutError`；目录和 Parley 上下文仍由 `self` 管理。
    pub fn layout_with_inline_objects(
        &mut self,
        catalog: &mut FontCatalog,
        paragraph: &Paragraph,
        settings: &ResolvedDocumentTextSettings,
        base_style: &ShapingStyle,
        options: ParagraphLayoutOptions,
        inline_objects: &[InlineObjectLayout],
    ) -> Result<(String, ParleyParagraphLayout), ParagraphLayoutError> {
        let ParagraphLayoutOptions {
            width_pt,
            spacing,
            line_grid_metrics,
            line_grid_font_size_centipoints,
            first_line_indent_chars,
        } = options;
        let empty_line_indent_pt = first_line_indent_chars
            .map(|chars| font_size_pt(base_style.font_size_centipoints) * f32::from(chars))
            .unwrap_or(0.0);
        if !width_pt.is_finite() || width_pt <= 0.0 {
            return Err(ParagraphLayoutError::InvalidWidth);
        }
        if base_style.font_size_centipoints == 0 {
            return Err(ParagraphLayoutError::InvalidFontSize);
        }
        let spans = paragraph_style_spans(paragraph, settings, base_style);
        let PreparedShaping {
            text,
            layout,
            grapheme_boundaries,
            requested_family_missing,
            missing_requested_families,
            has_missing_glyph,
        } = self.prepare_shaping(
            catalog,
            settings,
            &spans,
            base_style,
            None,
            PreparedLayoutOptions {
                width_pt: Some(width_pt),
                first_line_indent_chars,
                inline_objects,
            },
        )?;
        let ParagraphLineMapping {
            paragraph: paragraph_layout,
            line_top_offsets_pt,
            line_box_tops_pt,
            parley_line_tops_pt,
            parley_line_heights_pt,
            baseline_adjustments_pt,
        } = make_paragraph_lines(
            &text,
            &layout,
            spacing,
            line_grid_metrics,
            line_grid_font_size_centipoints,
            inline_objects,
        )?;
        Ok((
            text,
            ParleyParagraphLayout {
                layout,
                paragraph: paragraph_layout,
                grapheme_boundaries,
                line_top_offsets_pt,
                line_box_tops_pt,
                parley_line_tops_pt,
                parley_line_heights_pt,
                baseline_adjustments_pt,
                empty_line_indent_pt,
                requested_family_missing,
                missing_requested_families,
                has_missing_glyph,
                inline_object_source_ranges: inline_objects
                    .iter()
                    .map(|object| (object.id, object.source_bytes.clone()))
                    .collect(),
            },
        ))
    }

    /// 用 Parley 测量指定语言和字体样式的度量；测量空格不进入正文。
    pub fn measure_font_metrics(
        &mut self,
        catalog: &mut FontCatalog,
        settings: &ResolvedDocumentTextSettings,
        base_style: &ShapingStyle,
        language: Option<&str>,
        script_sample: Option<char>,
    ) -> Result<FontLineMetrics, ParagraphLayoutError> {
        let sample = script_sample.unwrap_or(' ');
        let paragraph = Paragraph {
            id: caret_doc::NodeId::generate(),
            language: language.map(str::to_owned),
            layout: None,
            inlines: vec![caret_doc::ParagraphContent::Text(caret_doc::TextFlow {
                runs: vec![caret_doc::Run {
                    text: sample.to_string(),
                    language: None,
                    props: Default::default(),
                    extra: Default::default(),
                }],
                extra: Default::default(),
            })],
            mark_style: Default::default(),
            props: Default::default(),
            extra: Default::default(),
        };
        let fallback = FontLineMetrics {
            ascent_pt: 9.6,
            descent_pt: 2.4,
            leading_pt: 0.0,
        };
        let (_, result) = self.layout(
            catalog,
            &paragraph,
            settings,
            base_style,
            ParagraphLayoutOptions {
                width_pt: 1000.0,
                spacing: LineSpacing::Natural,
                line_grid_metrics: fallback,
                line_grid_font_size_centipoints: base_style.font_size_centipoints,
                first_line_indent_chars: None,
            },
        )?;
        result
            .layout
            .lines()
            .next()
            .and_then(|line| line.runs().next())
            .map(|run| font_line_metrics(*run.font_metrics()))
            .ok_or(ParagraphLayoutError::InconsistentShaping)
    }
}

type PreparedStyleRange = (Range<usize>, ShapingStyle, Script, bool, bool, f32);

struct PreparedTextShaping {
    boundaries: crate::UnicodeBoundaries,
    decisions: Arc<PackedLineBreakDecisions>,
    style_ranges: Vec<PreparedStyleRange>,
    space_style_font_sizes_pt: Vec<f32>,
    space_samples: Vec<(Range<usize>, SpaceStyleKey)>,
    word_spacing_ranges: Vec<(Range<usize>, SpaceStyleKey)>,
    em_space_ranges: Vec<(Range<usize>, SpaceStyleKey)>,
    family_names_by_key: std::collections::HashMap<(ShapingStyle, Script, bool), Vec<String>>,
    requested_family_missing: bool,
    missing_requested_families: Vec<String>,
}

struct PreparedLayoutOptions<'a> {
    width_pt: Option<f32>,
    first_line_indent_chars: Option<u16>,
    inline_objects: &'a [InlineObjectLayout],
}

fn build_prepared_layout(
    context: &mut LayoutContext<[u8; 4]>,
    catalog: &mut FontCatalog,
    text: &str,
    base_style: &ShapingStyle,
    prepared: &PreparedTextShaping,
    space_adjustments_pt: &mut std::collections::HashMap<SpaceStyleKey, f32>,
    options: PreparedLayoutOptions<'_>,
) -> Result<Layout<[u8; 4]>, ParagraphLayoutError> {
    let PreparedLayoutOptions {
        width_pt,
        first_line_indent_chars,
        inline_objects,
    } = options;
    let mut build =
        |context: &mut LayoutContext<[u8; 4]>,
         adjustments: &std::collections::HashMap<SpaceStyleKey, f32>| {
            let input = ParleyLayoutInput {
                text,
                base_style,
                style_ranges: &prepared.style_ranges,
                family_names_by_key: &prepared.family_names_by_key,
                word_spacing_ranges: &prepared.word_spacing_ranges,
                em_space_ranges: &prepared.em_space_ranges,
                space_adjustments_pt: adjustments,
                decisions: &prepared.decisions,
                width_pt,
                first_line_indent_chars,
                inline_objects,
            };
            build_parley_layout(context, catalog, &input)
        };
    let (mut layout, mut callback_count) = build(context, space_adjustments_pt);
    if prepared
        .space_samples
        .iter()
        .any(|(_, key)| !space_adjustments_pt.contains_key(key))
    {
        cache_space_adjustments(
            &layout,
            &prepared.space_samples,
            &prepared.space_style_font_sizes_pt,
            space_adjustments_pt,
        );
        (layout, callback_count) = build(context, space_adjustments_pt);
    }
    if callback_count != prepared.decisions.len {
        return Err(ParagraphLayoutError::InconsistentShaping);
    }
    Ok(layout)
}

/// 正文段落与公式文本岛共用的塑形准备结果。
///
/// `layout` 是最终的 Parley 布局；`text` 和 `grapheme_boundaries` 始终描述逻辑源文本，
/// 而不是替换过 mandatory break 的布局文本。
struct PreparedShaping {
    text: String,
    layout: Layout<[u8; 4]>,
    grapheme_boundaries: Vec<usize>,
    requested_family_missing: bool,
    missing_requested_families: Vec<String>,
    has_missing_glyph: bool,
}

impl ParagraphLayouter {
    /// 正文段落与公式文本岛共享的塑形准备流水线。
    ///
    /// 两个入口的差异全部通过参数表达：`font_size_override_pt` 让公式文本岛按布局字号
    /// 覆盖 span 字号；`layout_options` 携带正文的宽度、首行缩进和行内对象，文本岛既
    /// 不设宽度和缩进，也没有行内对象。拼文本、mandatory break 归一化、塑形缓存键、
    /// 样式/断行准备、字素边界和 Parley 布局都只在这里实现一次。
    fn prepare_shaping(
        &mut self,
        catalog: &mut FontCatalog,
        settings: &ResolvedDocumentTextSettings,
        spans: &[StyledSpan<'_>],
        base_style: &ShapingStyle,
        font_size_override_pt: Option<f32>,
        layout_options: PreparedLayoutOptions<'_>,
    ) -> Result<PreparedShaping, ParagraphLayoutError> {
        let text: String = spans.iter().map(|span| span.text).collect();
        let layout_text = normalize_mandatory_breaks(&text);
        let space_style_signature = self.shaping_cache.prepare(
            catalog,
            settings,
            spans.iter().map(|span| span.style.clone()).collect(),
            font_size_override_pt,
            &mut self.context,
        );
        let inline_object_ranges =
            inline_object_source_ranges(&text, layout_options.inline_objects)?;
        let prepared = prepare_text_shaping_with_grapheme_fences(
            layout_text.as_ref(),
            spans,
            settings,
            catalog,
            font_size_override_pt,
            &inline_object_ranges,
        );
        // 光标边界始终取自源文本；等长的替换字符不一定有相同的 Unicode 边界。
        let grapheme_boundaries = match &layout_text {
            Cow::Borrowed(_) => prepared.boundaries.grapheme.clone(),
            Cow::Owned(_) => normalized_grapheme_boundaries(&text, spans, &inline_object_ranges),
        };
        let requested_family_missing = prepared.requested_family_missing;
        let missing_requested_families = prepared.missing_requested_families.clone();
        let space_adjustments = self
            .shaping_cache
            .space_adjustments_for(space_style_signature);
        let layout = build_prepared_layout(
            &mut self.context,
            catalog,
            layout_text.as_ref(),
            base_style,
            &prepared,
            space_adjustments,
            layout_options,
        )?;
        let has_missing_glyph = layout_has_missing_glyph(&layout);
        Ok(PreparedShaping {
            text,
            layout,
            grapheme_boundaries,
            requested_family_missing,
            missing_requested_families,
            has_missing_glyph,
        })
    }
}

/// 按源文本重新计算字素边界，并并入行内对象的起止 fence。
///
/// mandatory break 归一化只保持字节长度，替换字符的字素簇不一定与源文本一致，所以
/// 归一化后必须回到源文本计算边界。`UnicodeBoundaries` 的字素边界本身有序且去重，
/// 没有 fence 的文本岛再排序去重不会改变结果。
fn normalized_grapheme_boundaries(
    text: &str,
    spans: &[StyledSpan<'_>],
    inline_object_ranges: &[Range<usize>],
) -> Vec<usize> {
    let mut grapheme_boundaries = boundaries_for_spans(text, spans).grapheme;
    for range in inline_object_ranges {
        grapheme_boundaries.push(range.start);
        grapheme_boundaries.push(range.end);
    }
    grapheme_boundaries.sort_unstable();
    grapheme_boundaries.dedup();
    grapheme_boundaries
}

/// 校验行内对象的源范围、ID 和度量，返回按起点排序的源范围。
///
/// 每个对象必须恰好覆盖一个 U+FFFC 且互不重叠；塑形时用等长零宽占位符替换，
/// Parley 的字节索引因此仍能直接映射回逻辑文本。
fn inline_object_source_ranges(
    text: &str,
    inline_objects: &[InlineObjectLayout],
) -> Result<Vec<Range<usize>>, ParagraphLayoutError> {
    let mut inline_object_ids = HashSet::with_capacity(inline_objects.len());
    let mut inline_object_ranges = Vec::with_capacity(inline_objects.len());
    if inline_objects.iter().any(|object| {
        object.source_bytes.start >= object.source_bytes.end
            || object.source_bytes.end > text.len()
            || !text.is_char_boundary(object.source_bytes.start)
            || !text.is_char_boundary(object.source_bytes.end)
            || text.get(object.source_bytes.clone()) != Some("\u{fffc}")
            || !inline_object_ids.insert(object.id)
            || !object.advance_pt.is_finite()
            || !object.ascent_pt.is_finite()
            || !object.descent_pt.is_finite()
            || object.advance_pt < 0.0
            || object.ascent_pt < 0.0
            || object.descent_pt < 0.0
    }) {
        return Err(ParagraphLayoutError::InconsistentShaping);
    }
    inline_object_ranges.extend(
        inline_objects
            .iter()
            .map(|object| object.source_bytes.clone()),
    );
    inline_object_ranges.sort_unstable_by_key(|range| range.start);
    if inline_object_ranges
        .windows(2)
        .any(|ranges| ranges[0].end > ranges[1].start)
    {
        return Err(ParagraphLayoutError::InconsistentShaping);
    }
    Ok(inline_object_ranges)
}

/// 计算一次 Parley 塑形所需的样式区间、行断决策、空格校准点和字体族候选。
///
/// 输出由四个具名阶段按流水线顺序产生：边界与行断决策、span 索引、逐字素样式与
/// 空格校准、字体族解析；本函数只负责组装 `PreparedTextShaping`。
fn prepare_text_shaping_with_grapheme_fences(
    text: &str,
    spans: &[StyledSpan<'_>],
    settings: &ResolvedDocumentTextSettings,
    catalog: &FontCatalog,
    font_size_override_pt: Option<f32>,
    inline_object_ranges: &[Range<usize>],
) -> PreparedTextShaping {
    let (boundaries, decisions) =
        shaping_boundaries_and_decisions(text, spans, inline_object_ranges);
    let span_shaping = span_shaping_index(spans, font_size_override_pt);
    let grapheme_shaping = collect_grapheme_shaping(
        text,
        &boundaries.grapheme,
        &span_shaping,
        inline_object_ranges,
        font_size_override_pt,
    );
    let families = resolve_font_families(
        &grapheme_shaping.style_ranges,
        settings,
        catalog,
        inline_object_ranges,
    );
    PreparedTextShaping {
        boundaries,
        decisions,
        style_ranges: grapheme_shaping.style_ranges,
        space_style_font_sizes_pt: span_shaping.space_style_font_sizes_pt,
        space_samples: grapheme_shaping.space_samples,
        word_spacing_ranges: grapheme_shaping.word_spacing_ranges,
        em_space_ranges: grapheme_shaping.em_space_ranges,
        family_names_by_key: families.family_names_by_key,
        requested_family_missing: families.requested_family_missing,
        missing_requested_families: families.missing_requested_families,
    }
}

/// 计算字素与行断边界，把行内对象起止位置并入字素 fence，并生成行断覆盖表。
fn shaping_boundaries_and_decisions(
    text: &str,
    spans: &[StyledSpan<'_>],
    inline_object_ranges: &[Range<usize>],
) -> (crate::UnicodeBoundaries, Arc<PackedLineBreakDecisions>) {
    let mut boundaries = boundaries_for_spans(text, spans);
    for object_range in inline_object_ranges {
        boundaries.grapheme.push(object_range.start);
        boundaries.grapheme.push(object_range.end);
    }
    boundaries.grapheme.sort_unstable();
    boundaries.grapheme.dedup();
    let decisions = Arc::new(line_break_decisions(text, &boundaries));
    (boundaries, decisions)
}

/// 单次塑形准备的 span 索引：字节范围、去重样式索引和空格校准字号。
struct SpanShapingIndex<'a> {
    /// 每个 span 的源字节范围、样式和下划线。
    ranges: Vec<(Range<usize>, &'a ShapingStyle, bool)>,
    /// 每个 span 的去重样式索引，对应 `space_style_font_sizes_pt` 中的字号。
    space_style_indices: Vec<usize>,
    /// 每种去重样式用于空格校准的字号，按首次出现顺序排列。
    space_style_font_sizes_pt: Vec<f32>,
}

/// 记录每个 span 的源范围，并按首次出现顺序为样式分配空格校准字号。
fn span_shaping_index<'a>(
    spans: &'a [StyledSpan<'_>],
    font_size_override_pt: Option<f32>,
) -> SpanShapingIndex<'a> {
    let mut ranges = Vec::with_capacity(spans.len());
    let mut space_style_indices = Vec::with_capacity(spans.len());
    let mut space_style_font_sizes_pt = Vec::new();
    let mut style_indices = std::collections::HashMap::new();
    let mut offset = 0usize;
    for span in spans {
        let end = offset + span.text.len();
        ranges.push((offset..end, &span.style, span.underline));
        let style_index = *style_indices.entry(&span.style).or_insert_with(|| {
            let index = space_style_font_sizes_pt.len();
            space_style_font_sizes_pt.push(
                font_size_override_pt
                    .unwrap_or_else(|| font_size_pt(span.style.font_size_centipoints)),
            );
            index
        });
        space_style_indices.push(style_index);
        offset = end;
    }
    SpanShapingIndex {
        ranges,
        space_style_indices,
        space_style_font_sizes_pt,
    }
}

/// 一个字素簇的脚本与 emoji 呈现属性，决定字体族候选、空格校准键和区间合并。
#[derive(Clone, Copy)]
struct GraphemeAttributes {
    script: Script,
    emoji: bool,
}

/// 逐字素扫描得到的 Parley 样式区间与空格校准数据。
struct GraphemeShaping {
    style_ranges: Vec<PreparedStyleRange>,
    space_samples: Vec<(Range<usize>, SpaceStyleKey)>,
    word_spacing_ranges: Vec<(Range<usize>, SpaceStyleKey)>,
    em_space_ranges: Vec<(Range<usize>, SpaceStyleKey)>,
}

/// 按字素簇切分 span 样式，并收集空格校准样本与字距范围。
///
/// 样式只在字素簇边界上切换；普通空格每种（样式, 脚本, emoji）组合只保留首个样本，
/// 字距规则合并到连续区间，避免每个空格生成独立的 Parley 样式 run。
fn collect_grapheme_shaping(
    text: &str,
    grapheme_boundaries: &[usize],
    span_shaping: &SpanShapingIndex<'_>,
    inline_object_ranges: &[Range<usize>],
    font_size_override_pt: Option<f32>,
) -> GraphemeShaping {
    let emoji_presentation = CodePointSetDataBorrowed::new::<props::EmojiPresentation>();
    let mut style_ranges: Vec<PreparedStyleRange> = Vec::new();
    let mut space_samples: Vec<(Range<usize>, SpaceStyleKey)> = Vec::new();
    let mut word_spacing_ranges: Vec<(Range<usize>, SpaceStyleKey)> = Vec::new();
    let mut em_space_ranges: Vec<(Range<usize>, SpaceStyleKey)> = Vec::new();
    let mut sampled_space_keys = HashSet::new();
    let mut span_index = 0usize;
    let mut previous_script = grapheme_boundaries
        .windows(2)
        .find_map(|range| strong_script(&text[range[0]..range[1]]));
    for pair in grapheme_boundaries.windows(2) {
        let range = pair[0]..pair[1];
        if range.is_empty() {
            continue;
        }
        while span_shaping
            .ranges
            .get(span_index)
            .is_some_and(|(span_range, _, _)| span_range.end <= range.start)
        {
            span_index += 1;
        }
        let cluster_text = &text[range.clone()];
        let attributes = GraphemeAttributes {
            script: strong_script(cluster_text)
                .or(previous_script)
                .unwrap_or_else(default_script),
            emoji: is_emoji_grapheme(cluster_text, &emoji_presentation),
        };
        previous_script = Some(attributes.script);
        let space_kind = leading_space_kind(cluster_text);
        let code_role = span_shaping
            .ranges
            .get(span_index)
            .is_some_and(|(_, style, _)| style.font_role == crate::shaping::FontRole::Code);
        // Parley 的 WordSpacing 也会调整 NBSP；把它从连续样式区间拆出，
        // 否则同一字体先排过普通空格后，NBSP 宽度会依排版历史改变。
        if !code_role
            && span_shaping.ranges.get(span_index).is_some()
            && !cluster_text.contains('\u{00a0}')
        {
            let key = SpaceStyleKey {
                style_index: span_shaping.space_style_indices[span_index],
                script: attributes.script,
                emoji: attributes.emoji,
                kind: SpaceKind::Ordinary,
            };
            merge_word_spacing_range(&mut word_spacing_ranges, range.clone(), key);
        }
        if let Some(kind) = space_kind
            && !code_role
            && span_shaping.ranges.get(span_index).is_some()
        {
            let space_end = range.start
                + match kind {
                    SpaceKind::Ordinary => ' '.len_utf8(),
                    SpaceKind::Em => '\u{2003}'.len_utf8(),
                };
            let key = SpaceStyleKey {
                style_index: span_shaping.space_style_indices[span_index],
                script: attributes.script,
                emoji: attributes.emoji,
                kind,
            };
            push_space_sample(
                &mut space_samples,
                &mut em_space_ranges,
                &mut sampled_space_keys,
                range.start..space_end,
                key,
            );
        }
        span_index = append_grapheme_style_ranges(
            &mut style_ranges,
            &range,
            &span_shaping.ranges,
            span_index,
            attributes,
            font_size_override_pt,
            inline_object_ranges,
        );
    }
    GraphemeShaping {
        style_ranges,
        space_samples,
        word_spacing_ranges,
        em_space_ranges,
    }
}

/// 返回字素簇开头的空格类型：U+0020 为普通空格，U+2003 为全字宽空格。
fn leading_space_kind(cluster_text: &str) -> Option<SpaceKind> {
    if cluster_text.starts_with(' ') {
        Some(SpaceKind::Ordinary)
    } else if cluster_text.starts_with('\u{2003}') {
        Some(SpaceKind::Em)
    } else {
        None
    }
}

/// 合并相邻且校准键相同的字距范围，避免连续空格生成大量 Parley 样式 run。
fn merge_word_spacing_range(
    ranges: &mut Vec<(Range<usize>, SpaceStyleKey)>,
    range: Range<usize>,
    key: SpaceStyleKey,
) {
    if let Some((previous_range, previous_key)) = ranges.last_mut()
        && previous_range.end == range.start
        && *previous_key == key
    {
        previous_range.end = range.end;
    } else {
        ranges.push((range, key));
    }
}

/// 记录每种（样式, 脚本, emoji, 空格类型）组合的第一个空格测量样本。
///
/// 全字宽空格的 letter-spacing 修正需要覆盖每个实例，所以 `em_space_ranges` 不按样本键去重。
fn push_space_sample(
    space_samples: &mut Vec<(Range<usize>, SpaceStyleKey)>,
    em_space_ranges: &mut Vec<(Range<usize>, SpaceStyleKey)>,
    sampled_space_keys: &mut HashSet<SpaceStyleKey>,
    range: Range<usize>,
    key: SpaceStyleKey,
) {
    if sampled_space_keys.insert(key) {
        space_samples.push((range.clone(), key));
    }
    if key.kind == SpaceKind::Em {
        em_space_ranges.push((range, key));
    }
}

/// 把字素簇在 span 上的片段切成 Parley 样式区间，返回下一个 span 索引。
///
/// 片段只在 span 边界和字素簇边界上断开；行内对象起止位置必须成为独立 fence，
/// 否则 Parley 的 inline box 可能落在错误的样式区间一侧。
fn append_grapheme_style_ranges(
    style_ranges: &mut Vec<PreparedStyleRange>,
    range: &Range<usize>,
    span_ranges: &[(Range<usize>, &ShapingStyle, bool)],
    mut span_index: usize,
    attributes: GraphemeAttributes,
    font_size_override_pt: Option<f32>,
    inline_object_ranges: &[Range<usize>],
) -> usize {
    while let Some((span_range, style, underline)) = span_ranges.get(span_index) {
        if span_range.start >= range.end {
            break;
        }
        let fragment = span_range.start.max(range.start)..span_range.end.min(range.end);
        if fragment.start < fragment.end {
            let fragment_font_size_pt =
                font_size_override_pt.unwrap_or_else(|| font_size_pt(style.font_size_centipoints));
            push_style_range(
                style_ranges,
                (
                    fragment,
                    (*style).clone(),
                    attributes.script,
                    attributes.emoji,
                    *underline,
                    fragment_font_size_pt,
                ),
                inline_object_ranges,
            );
        }
        if span_range.end <= range.end {
            span_index += 1;
        } else {
            break;
        }
    }
    span_index
}

/// 追加一个样式片段；相邻且属性完全相同的片段合并成一个连续区间。
fn push_style_range(
    style_ranges: &mut Vec<PreparedStyleRange>,
    fragment: PreparedStyleRange,
    inline_object_ranges: &[Range<usize>],
) {
    if let Some(previous) = style_ranges.last_mut()
        && previous.0.end == fragment.0.start
        && !is_inline_object_boundary(inline_object_ranges, fragment.0.start)
        && previous.1 == fragment.1
        && previous.2 == fragment.2
        && previous.3 == fragment.3
        && previous.4 == fragment.4
        && previous.5 == fragment.5
    {
        previous.0.end = fragment.0.end;
    } else {
        style_ranges.push(fragment);
    }
}

/// 每个（样式, 脚本, emoji）键的字体族候选链和缺失首选族诊断。
struct ResolvedFontFamilies {
    family_names_by_key: std::collections::HashMap<(ShapingStyle, Script, bool), Vec<String>>,
    requested_family_missing: bool,
    missing_requested_families: Vec<String>,
}

/// 为样式区间中出现过的每个（样式, 脚本, emoji）键解析字体族候选链。
fn resolve_font_families(
    style_ranges: &[PreparedStyleRange],
    settings: &ResolvedDocumentTextSettings,
    catalog: &FontCatalog,
    inline_object_ranges: &[Range<usize>],
) -> ResolvedFontFamilies {
    let available_families = Arc::clone(&catalog.family_names_lowercase);
    let mut requested_family_missing = false;
    let mut missing_requested_families = std::collections::BTreeSet::new();
    let mut family_names_by_key =
        std::collections::HashMap::<(ShapingStyle, Script, bool), Vec<String>>::new();
    for (range, style, script, emoji, _, _) in style_ranges {
        if is_inline_object_range(inline_object_ranges, range) {
            continue;
        }
        let key = (style.clone(), *script, *emoji);
        family_names_by_key.entry(key).or_insert_with(|| {
            resolve_family_candidates(
                style,
                *script,
                settings,
                &available_families,
                &mut requested_family_missing,
                &mut missing_requested_families,
            )
        });
    }
    ResolvedFontFamilies {
        family_names_by_key,
        requested_family_missing,
        missing_requested_families: missing_requested_families.into_iter().collect(),
    }
}

/// 解析单个样式键的候选字体族；首选候选族未安装时记录缺失诊断。
fn resolve_family_candidates(
    style: &ShapingStyle,
    script: Script,
    settings: &ResolvedDocumentTextSettings,
    available_families: &HashSet<String>,
    requested_family_missing: &mut bool,
    missing_requested_families: &mut std::collections::BTreeSet<String>,
) -> Vec<String> {
    let candidates = settings.font_preferences().family_candidates(
        crate::font_selection::FontSelectionContext {
            script,
            language: style.language.as_deref(),
            role: style.font_role,
            explicit_family: style.requested_family.as_deref(),
        },
    );
    if let Some(candidate) = candidates.first()
        && !is_generic_family(candidate.family)
        && !available_families.contains(&candidate.family.to_lowercase())
    {
        *requested_family_missing = true;
        missing_requested_families.insert(candidate.family.to_owned());
    }
    let mut names = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        names.push(candidate.family.to_owned());
    }
    names
}

struct ParleyLayoutInput<'a> {
    text: &'a str,
    base_style: &'a ShapingStyle,
    style_ranges: &'a [PreparedStyleRange],
    family_names_by_key: &'a std::collections::HashMap<(ShapingStyle, Script, bool), Vec<String>>,
    word_spacing_ranges: &'a [(Range<usize>, SpaceStyleKey)],
    em_space_ranges: &'a [(Range<usize>, SpaceStyleKey)],
    space_adjustments_pt: &'a std::collections::HashMap<SpaceStyleKey, f32>,
    decisions: &'a Arc<PackedLineBreakDecisions>,
    width_pt: Option<f32>,
    first_line_indent_chars: Option<u16>,
    inline_objects: &'a [InlineObjectLayout],
}

/// 构建 Parley 布局，并在对应空白范围应用由实际字体度量得出的 advance 修正。
fn build_parley_layout(
    context: &mut LayoutContext<[u8; 4]>,
    catalog: &mut FontCatalog,
    input: &ParleyLayoutInput<'_>,
) -> (Layout<[u8; 4]>, usize) {
    let ParleyLayoutInput {
        text,
        base_style,
        style_ranges,
        family_names_by_key,
        word_spacing_ranges,
        em_space_ranges,
        space_adjustments_pt,
        decisions,
        width_pt,
        first_line_indent_chars,
        inline_objects,
    } = *input;
    // Parley 会把 inline box 插入指定索引，但不会替换该索引上的文本。保留三字节宽度，
    // 将逻辑 U+FFFC 换成等长、零推进的 U+200B；避免原替换字符贡献独立 advance，并保持源字节偏移不变。
    let mut shaping_text = text.to_owned();
    for object in inline_objects {
        shaping_text.replace_range(object.source_bytes.clone(), "\u{200b}");
    }
    let mut builder = context.ranged_builder(catalog.parley_context(), &shaping_text, 1.0, false);
    builder.push_default(StyleProperty::FontFamily(GenericFamily::SystemUi.into()));
    builder.push_default(StyleProperty::FontSize(font_size_pt(
        base_style.font_size_centipoints,
    )));
    builder.push_default(StyleProperty::WhiteSpaceCollapse(
        WhiteSpaceCollapse::BreakSpaces,
    ));

    for (range, style, script, emoji, underline, resolved_font_size_pt) in style_ranges {
        let key = (style.clone(), *script, *emoji);
        let family_names = family_names_by_key
            .get(&key)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let families = font_families(
            family_names,
            *emoji,
            key.0.font_role == crate::shaping::FontRole::Code,
        );
        builder.push(
            StyleProperty::FontFamily(families.as_slice().into()),
            range.clone(),
        );
        builder.push(
            StyleProperty::FontSize(*resolved_font_size_pt),
            range.clone(),
        );
        builder.push(
            StyleProperty::FontWeight(if style.bold {
                fontique::FontWeight::BOLD
            } else {
                fontique::FontWeight::NORMAL
            }),
            range.clone(),
        );
        builder.push(
            StyleProperty::FontStyle(if style.italic {
                fontique::FontStyle::Italic
            } else {
                fontique::FontStyle::Normal
            }),
            range.clone(),
        );
        builder.push(StyleProperty::Underline(*underline), range.clone());
        builder.push(
            StyleProperty::Locale(
                style
                    .language
                    .as_deref()
                    .and_then(|language| Language::parse(language).ok()),
            ),
            range.clone(),
        );
    }
    for (range, key) in word_spacing_ranges {
        let key = SpaceStyleKey {
            kind: SpaceKind::Ordinary,
            ..*key
        };
        let Some(adjustment_pt) = space_adjustments_pt.get(&key).copied() else {
            continue;
        };
        builder.push(StyleProperty::WordSpacing(adjustment_pt), range.clone());
    }
    for (range, key) in em_space_ranges {
        let Some(adjustment_pt) = space_adjustments_pt.get(key).copied() else {
            continue;
        };
        builder.push(StyleProperty::LetterSpacing(adjustment_pt), range.clone());
    }
    for object in inline_objects {
        builder.push_inline_box(InlineBox {
            id: object.id,
            kind: InlineBoxKind::InFlow,
            // 放在零宽占位符之后，避免 Parley 的超宽独占行将前后文本范围算到错误一侧。
            index: object.source_bytes.end,
            width: object.advance_pt,
            height: object.ascent_pt.max(0.0) + object.descent_pt.max(0.0),
            baseline: Some(object.ascent_pt.max(0.0)),
            vertical_align: VerticalAlign::BASELINE,
        });
    }

    let callback_index = Arc::new(AtomicUsize::new(0));
    let callback_cursor = Arc::clone(&callback_index);
    let callback_decisions = Arc::clone(decisions);
    let line_break_override =
        move |_| callback_decisions.get(callback_cursor.fetch_add(1, Ordering::Relaxed));
    builder.set_line_break_override(Some(&line_break_override));
    let mut layout = builder.build(&shaping_text);
    if let Some(chars) = first_line_indent_chars {
        let indent_pt = font_size_pt(base_style.font_size_centipoints) * f32::from(chars);
        layout.set_text_indent(indent_pt, IndentOptions::default());
    }
    layout.break_all_lines(width_pt);
    if first_line_indent_chars.is_some() {
        layout.align(Alignment::Start, AlignmentOptions::default());
    }
    (layout, callback_index.load(Ordering::Relaxed))
}

/// 从未校准布局的实际塑形 advance 缓存 U+0020 与 U+2003 的目标宽度修正。
fn cache_space_adjustments(
    layout: &Layout<[u8; 4]>,
    space_samples: &[(Range<usize>, SpaceStyleKey)],
    space_style_font_sizes_pt: &[f32],
    adjustments: &mut std::collections::HashMap<SpaceStyleKey, f32>,
) {
    let spaces_by_start: std::collections::HashMap<_, _> = space_samples
        .iter()
        .map(|(range, key)| (range.start, (range.end, key)))
        .collect();
    for line in layout.lines() {
        for run in line.runs() {
            for cluster in run.clusters() {
                let range = cluster.text_range();
                let Some((end, key)) = spaces_by_start.get(&range.start) else {
                    continue;
                };
                if range.end < *end || adjustments.contains_key(*key) {
                    continue;
                }
                let target_percent = match key.kind {
                    SpaceKind::Ordinary => ORDINARY_SPACE_EM_PERCENT,
                    SpaceKind::Em => EM_SPACE_EM_PERCENT,
                };
                let Some(font_size_pt) = space_style_font_sizes_pt.get(key.style_index) else {
                    continue;
                };
                let target_advance = *font_size_pt * f32::from(target_percent) * 0.01;
                let adjustment = target_advance - cluster.advance();
                if target_advance.is_finite() && adjustment.is_finite() {
                    adjustments.insert(**key, adjustment);
                }
            }
        }
    }
}

fn font_size_pt(font_size_centipoints: u16) -> f32 {
    DocumentUnits::centipoints_to_points(u32::from(font_size_centipoints)) as f32
}

fn is_generic_family(family: &str) -> bool {
    generic_family(family).is_some()
}

fn generic_family(family: &str) -> Option<GenericFamily> {
    match family.trim().to_ascii_lowercase().as_str() {
        "serif" => Some(GenericFamily::Serif),
        "sans-serif" => Some(GenericFamily::SansSerif),
        "monospace" => Some(GenericFamily::Monospace),
        _ => None,
    }
}

fn font_families(names: &[String], emoji: bool, code: bool) -> Vec<FontFamilyName<'_>> {
    let mut families = Vec::with_capacity(names.len() + 3);
    if emoji {
        families.push(FontFamilyName::Generic(GenericFamily::Emoji));
    }
    families.extend(names.iter().map(|family| match generic_family(family) {
        Some(generic) => FontFamilyName::Generic(generic),
        None => FontFamilyName::named(family),
    }));
    if code {
        families.push(FontFamilyName::Generic(GenericFamily::Monospace));
    }
    families.push(FontFamilyName::Generic(GenericFamily::SystemUi));
    families
}

#[derive(Clone)]
struct PackedLineBreakDecisions {
    words: Vec<u64>,
    len: usize,
}

impl PackedLineBreakDecisions {
    fn push(&mut self, allowed: bool) {
        if self.len.is_multiple_of(u64::BITS as usize) {
            self.words.push(0);
        }
        if allowed {
            let word = self.len / u64::BITS as usize;
            let bit = self.len % u64::BITS as usize;
            self.words[word] |= 1 << bit;
        }
        self.len += 1;
    }

    fn get(&self, index: usize) -> Option<bool> {
        (index < self.len).then(|| {
            let word = self.words[index / u64::BITS as usize];
            word & (1 << (index % u64::BITS as usize)) != 0
        })
    }
}

fn line_break_decisions(
    text: &str,
    boundaries: &crate::UnicodeBoundaries,
) -> PackedLineBreakDecisions {
    use icu_properties::props::LineBreak as LineBreakClass;

    let mandatory = CodePointMapData::<LineBreakClass>::new();
    let mut result = PackedLineBreakDecisions {
        words: Vec::with_capacity(text.len().div_ceil(u64::BITS as usize)),
        len: 0,
    };
    let mut byte_offset = 0usize;
    let mut line_break_index = 0usize;
    let mut grapheme_index = 0usize;
    let mut previous: Option<char> = None;
    for character in text.chars() {
        if let Some(before) = previous {
            byte_offset += before.len_utf8();
            while boundaries
                .line_break
                .get(line_break_index)
                .is_some_and(|point| *point < byte_offset)
            {
                line_break_index += 1;
            }
            while boundaries
                .grapheme
                .get(grapheme_index)
                .is_some_and(|point| *point < byte_offset)
            {
                grapheme_index += 1;
            }
            if !is_mandatory_line_break(mandatory.get(before))
                && !is_mandatory_line_break(mandatory.get(character))
            {
                result.push(
                    boundaries.line_break.get(line_break_index) == Some(&byte_offset)
                        && boundaries.grapheme.get(grapheme_index) == Some(&byte_offset),
                );
            }
        }
        previous = Some(character);
    }
    result
}

fn strong_script(text: &str) -> Option<Script> {
    let script_map = CodePointMapData::<props::Script>::new();
    let names = PropertyNamesShort::<props::Script>::new();
    text.chars().find_map(|character| {
        let script = script_map.get(character);
        if matches!(
            script,
            props::Script::Common | props::Script::Inherited | props::Script::Unknown
        ) {
            return None;
        }
        names.get_locale_script(script)
    })
}

fn default_script() -> Script {
    "Latn"
        .parse()
        .unwrap_or_else(|_| unreachable!("ICU accepts the stable Latn script tag"))
}

fn is_emoji_grapheme(text: &str, emoji_presentation: &CodePointSetDataBorrowed<'static>) -> bool {
    text.chars().any(|character| {
        emoji_presentation.contains(character) || matches!(character, '\u{200d}' | '\u{fe0f}')
    })
}

#[cfg(test)]
mod tests {
    use super::font_families;
    use parley::{FontFamilyName, GenericFamily};

    #[test]
    fn emoji_generic_family_precedes_named_preferences() {
        let names = vec!["Preferred Text Family".to_owned()];
        assert_eq!(
            font_families(&names, true, false),
            vec![
                FontFamilyName::Generic(GenericFamily::Emoji),
                FontFamilyName::named("Preferred Text Family"),
                FontFamilyName::Generic(GenericFamily::SystemUi),
            ]
        );
        assert_eq!(
            font_families(&names, false, false),
            vec![
                FontFamilyName::named("Preferred Text Family"),
                FontFamilyName::Generic(GenericFamily::SystemUi),
            ]
        );
    }

    #[test]
    fn code_font_places_monospace_after_named_preference() {
        let names = vec!["Preferred Code Family".to_owned()];
        assert_eq!(
            font_families(&names, false, true),
            vec![
                FontFamilyName::named("Preferred Code Family"),
                FontFamilyName::Generic(GenericFamily::Monospace),
                FontFamilyName::Generic(GenericFamily::SystemUi),
            ]
        );
        assert_eq!(
            font_families(&[], false, true),
            vec![
                FontFamilyName::Generic(GenericFamily::Monospace),
                FontFamilyName::Generic(GenericFamily::SystemUi),
            ]
        );
    }

    #[test]
    fn explicit_generic_families_keep_their_generic_fontique_semantics() {
        let names = vec![
            "serif".to_owned(),
            "sans-serif".to_owned(),
            "monospace".to_owned(),
        ];
        assert_eq!(
            font_families(&names, false, false),
            vec![
                FontFamilyName::Generic(GenericFamily::Serif),
                FontFamilyName::Generic(GenericFamily::SansSerif),
                FontFamilyName::Generic(GenericFamily::Monospace),
                FontFamilyName::Generic(GenericFamily::SystemUi),
            ]
        );
    }
}
