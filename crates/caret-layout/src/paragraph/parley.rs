//! 用 Parley 高层 Layout 生成段落塑形、行断和视觉几何，并映射回 Caret 行框。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use caret_doc::Paragraph;
use icu_locale_core::subtags::Script;
use icu_properties::{CodePointMapData, CodePointSetDataBorrowed, PropertyNamesShort, props};
use parley::{
    Alignment, AlignmentOptions, FontFamilyName, GenericFamily, IndentOptions, Language, Layout,
    LayoutContext, StyleProperty, WhiteSpaceCollapse,
};

use crate::{
    FontCatalog, FontLineMetrics, LineSpacing, ParagraphLayout, ParagraphLayoutError,
    ResolvedDocumentTextSettings, ShapingStyle,
    shaping::{boundaries_for_spans, is_mandatory_line_break},
};

use crate::shaping::paragraph_style_spans;

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
    font_generation: Option<u64>,
    space_adjustments_pt: std::collections::HashMap<SpaceStyleKey, f32>,
    space_font_preferences: Option<crate::FontPreferenceRules>,
    space_style_signature: Option<Vec<ShapingStyle>>,
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
        let font_generation = catalog.generation();
        if self.font_generation != Some(font_generation) {
            self.context = LayoutContext::default();
            self.font_generation = Some(font_generation);
            self.space_adjustments_pt.clear();
        }
        if self.space_font_preferences.as_ref() != Some(settings.font_preferences()) {
            self.space_font_preferences = Some(settings.font_preferences().clone());
            self.space_adjustments_pt.clear();
        }
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
        let space_style_signature: Vec<_> = spans.iter().map(|span| span.style.clone()).collect();
        if self.space_style_signature.as_ref() != Some(&space_style_signature) {
            self.space_style_signature = Some(space_style_signature);
            self.space_adjustments_pt.clear();
        }
        let text: String = spans.iter().map(|span| span.text).collect();
        let boundaries = boundaries_for_spans(&text, &spans);
        let decisions = Arc::new(line_break_decisions(&text, &boundaries));
        let available_families = Arc::clone(&catalog.family_names_lowercase);
        let mut requested_family_missing = false;
        let mut span_ranges = Vec::with_capacity(spans.len());
        let mut span_space_style_indices = Vec::with_capacity(spans.len());
        let mut space_styles = Vec::new();
        let mut space_style_indices = std::collections::HashMap::new();
        let mut offset = 0usize;
        for span in &spans {
            let end = offset + span.text.len();
            span_ranges.push((offset..end, &span.style, span.underline));
            let style_index = *space_style_indices.entry(&span.style).or_insert_with(|| {
                let index = space_styles.len();
                space_styles.push(&span.style);
                index
            });
            span_space_style_indices.push(style_index);
            offset = end;
        }
        let emoji_presentation = CodePointSetDataBorrowed::new::<props::EmojiPresentation>();
        let mut style_ranges: Vec<(Range<usize>, ShapingStyle, Script, bool, bool)> = Vec::new();
        // 普通空格只保留每种塑形样式的一个测量样本；字距规则应用到连续样式区间，
        // 避免每个空格都创建一个 Parley 样式 run。
        let mut space_samples: Vec<(Range<usize>, SpaceStyleKey)> = Vec::new();
        let mut word_spacing_ranges: Vec<(Range<usize>, SpaceStyleKey)> = Vec::new();
        let mut em_space_ranges: Vec<(Range<usize>, SpaceStyleKey)> = Vec::new();
        let mut sampled_space_keys = std::collections::HashSet::new();
        let mut span_index = 0usize;
        let first_script = boundaries
            .grapheme
            .windows(2)
            .find_map(|range| strong_script(&text[range[0]..range[1]]));
        let mut previous_script = first_script;
        for pair in boundaries.grapheme.windows(2) {
            let range = pair[0]..pair[1];
            if range.is_empty() {
                continue;
            }
            while span_ranges
                .get(span_index)
                .is_some_and(|(span_range, _, _)| span_range.end <= range.start)
            {
                span_index += 1;
            }
            let cluster_text = &text[range.clone()];
            let script = strong_script(cluster_text)
                .or(previous_script)
                .unwrap_or_else(default_script);
            previous_script = Some(script);
            let emoji = is_emoji_grapheme(&text[range.clone()], &emoji_presentation);
            let space_kind = if cluster_text.starts_with(' ') {
                Some(SpaceKind::Ordinary)
            } else if cluster_text.starts_with('\u{2003}') {
                Some(SpaceKind::Em)
            } else {
                None
            };
            let code_role = span_ranges
                .get(span_index)
                .is_some_and(|(_, style, _)| style.font_role == crate::shaping::FontRole::Code);
            // Parley 的 WordSpacing 也会调整 NBSP；把它从连续样式区间拆出，
            // 否则同一字体先排过普通空格后，NBSP 宽度会依排版历史改变。
            if !code_role
                && span_ranges.get(span_index).is_some()
                && !cluster_text.contains('\u{00a0}')
            {
                let key = SpaceStyleKey {
                    style_index: span_space_style_indices[span_index],
                    script,
                    emoji,
                    kind: SpaceKind::Ordinary,
                };
                if let Some((previous_range, previous_key)) = word_spacing_ranges.last_mut()
                    && previous_range.end == range.start
                    && *previous_key == key
                {
                    previous_range.end = range.end;
                } else {
                    word_spacing_ranges.push((range.clone(), key));
                }
            }
            if let Some(kind) = space_kind
                && !code_role
                && span_ranges.get(span_index).is_some()
            {
                let space_end = range.start
                    + match kind {
                        SpaceKind::Ordinary => ' '.len_utf8(),
                        SpaceKind::Em => '\u{2003}'.len_utf8(),
                    };
                let key = SpaceStyleKey {
                    style_index: span_space_style_indices[span_index],
                    script,
                    emoji,
                    kind,
                };
                if sampled_space_keys.insert(key) {
                    space_samples.push((range.start..space_end, key));
                }
                if kind == SpaceKind::Em {
                    em_space_ranges.push((range.start..space_end, key));
                }
            }
            let mut fragment_index = span_index;
            while let Some((span_range, style, underline)) = span_ranges.get(fragment_index) {
                if span_range.start >= range.end {
                    break;
                }
                let fragment = span_range.start.max(range.start)..span_range.end.min(range.end);
                if fragment.start < fragment.end {
                    if let Some((
                        previous,
                        previous_style,
                        previous_script,
                        previous_emoji,
                        previous_underline,
                    )) = style_ranges.last_mut()
                        && previous.end == fragment.start
                        && *previous_style == **style
                        && *previous_script == script
                        && *previous_emoji == emoji
                        && *previous_underline == *underline
                    {
                        previous.end = fragment.end;
                    } else {
                        style_ranges.push((fragment, (*style).clone(), script, emoji, *underline));
                    }
                }
                if span_range.end <= range.end {
                    fragment_index += 1;
                } else {
                    break;
                }
            }
            span_index = fragment_index;
        }
        let mut family_names_by_key =
            std::collections::HashMap::<(ShapingStyle, Script, bool), Vec<String>>::new();
        for (_, style, script, emoji, _) in &style_ranges {
            let key = (style.clone(), *script, *emoji);
            let _family_names = family_names_by_key.entry(key).or_insert_with(|| {
                let candidates = settings.font_preferences().family_candidates(
                    crate::font_selection::FontSelectionContext {
                        script: *script,
                        language: style.language.as_deref(),
                        math_context: false,
                        explicit_family: style.requested_family.as_deref(),
                        code: style.font_role == crate::shaping::FontRole::Code,
                    },
                );
                requested_family_missing |= candidates.first().is_some_and(|candidate| {
                    !available_families.contains(&candidate.family.to_lowercase())
                });
                let mut names = Vec::with_capacity(candidates.len());
                for candidate in candidates {
                    names.push(candidate.family.to_owned());
                }
                names
            });
        }

        let has_uncalibrated_spaces = space_samples
            .iter()
            .any(|(_, key)| !self.space_adjustments_pt.contains_key(key));
        let (mut layout, mut callback_count) = {
            let layout_input = ParleyLayoutInput {
                text: &text,
                base_style,
                style_ranges: &style_ranges,
                family_names_by_key: &family_names_by_key,
                word_spacing_ranges: &word_spacing_ranges,
                em_space_ranges: &em_space_ranges,
                space_adjustments_pt: &self.space_adjustments_pt,
                decisions: &decisions,
                width_pt,
                first_line_indent_chars,
            };
            build_parley_layout(&mut self.context, catalog, &layout_input)
        };
        if has_uncalibrated_spaces {
            cache_space_adjustments(
                &layout,
                &space_samples,
                &space_styles,
                &mut self.space_adjustments_pt,
            );
            let layout_input = ParleyLayoutInput {
                text: &text,
                base_style,
                style_ranges: &style_ranges,
                family_names_by_key: &family_names_by_key,
                word_spacing_ranges: &word_spacing_ranges,
                em_space_ranges: &em_space_ranges,
                space_adjustments_pt: &self.space_adjustments_pt,
                decisions: &decisions,
                width_pt,
                first_line_indent_chars,
            };
            (layout, callback_count) =
                build_parley_layout(&mut self.context, catalog, &layout_input);
        }
        if callback_count != decisions.len {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
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
        )?;
        Ok((
            text,
            ParleyParagraphLayout {
                layout,
                paragraph: paragraph_layout,
                grapheme_boundaries: boundaries.grapheme,
                line_top_offsets_pt,
                line_box_tops_pt,
                parley_line_tops_pt,
                parley_line_heights_pt,
                baseline_adjustments_pt,
                empty_line_indent_pt,
                requested_family_missing,
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
            inlines: vec![caret_doc::InlineNode::Run(caret_doc::Run {
                text: sample.to_string(),
                language: None,
                props: Default::default(),
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

struct ParleyLayoutInput<'a> {
    text: &'a str,
    base_style: &'a ShapingStyle,
    style_ranges: &'a [(Range<usize>, ShapingStyle, Script, bool, bool)],
    family_names_by_key: &'a std::collections::HashMap<(ShapingStyle, Script, bool), Vec<String>>,
    word_spacing_ranges: &'a [(Range<usize>, SpaceStyleKey)],
    em_space_ranges: &'a [(Range<usize>, SpaceStyleKey)],
    space_adjustments_pt: &'a std::collections::HashMap<SpaceStyleKey, f32>,
    decisions: &'a Arc<PackedLineBreakDecisions>,
    width_pt: f32,
    first_line_indent_chars: Option<u16>,
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
    } = *input;
    let mut builder = context.ranged_builder(catalog.parley_context(), text, 1.0, false);
    builder.push_default(StyleProperty::FontFamily(GenericFamily::SystemUi.into()));
    builder.push_default(StyleProperty::FontSize(font_size_pt(
        base_style.font_size_centipoints,
    )));
    builder.push_default(StyleProperty::WhiteSpaceCollapse(
        WhiteSpaceCollapse::BreakSpaces,
    ));

    for (range, style, script, emoji, underline) in style_ranges {
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
            StyleProperty::FontSize(font_size_pt(style.font_size_centipoints)),
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

    let callback_index = Arc::new(AtomicUsize::new(0));
    let callback_cursor = Arc::clone(&callback_index);
    let callback_decisions = Arc::clone(decisions);
    let line_break_override =
        move |_| callback_decisions.get(callback_cursor.fetch_add(1, Ordering::Relaxed));
    builder.set_line_break_override(Some(&line_break_override));
    let mut layout = builder.build(text);
    if let Some(chars) = first_line_indent_chars {
        let indent_pt = font_size_pt(base_style.font_size_centipoints) * f32::from(chars);
        layout.set_text_indent(indent_pt, IndentOptions::default());
    }
    layout.break_all_lines(Some(width_pt));
    if first_line_indent_chars.is_some() {
        layout.align(Alignment::Start, AlignmentOptions::default());
    }
    (layout, callback_index.load(Ordering::Relaxed))
}

/// 从未校准布局的实际塑形 advance 缓存 U+0020 与 U+2003 的目标宽度修正。
fn cache_space_adjustments(
    layout: &Layout<[u8; 4]>,
    space_samples: &[(Range<usize>, SpaceStyleKey)],
    space_styles: &[&ShapingStyle],
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
                let Some(style) = space_styles.get(key.style_index) else {
                    continue;
                };
                let target_advance =
                    font_size_pt(style.font_size_centipoints) * f32::from(target_percent) * 0.01;
                let adjustment = target_advance - cluster.advance();
                if target_advance.is_finite() && adjustment.is_finite() {
                    adjustments.insert(**key, adjustment);
                }
            }
        }
    }
}

fn font_size_pt(font_size_centipoints: u16) -> f32 {
    f32::from(font_size_centipoints) * 0.01
}

fn font_families(names: &[String], emoji: bool, code: bool) -> Vec<FontFamilyName<'_>> {
    let mut families = Vec::with_capacity(names.len() + 3);
    if emoji {
        families.push(FontFamilyName::Generic(GenericFamily::Emoji));
    }
    families.extend(names.iter().map(|family| FontFamilyName::named(family)));
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
}
