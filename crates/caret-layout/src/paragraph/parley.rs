//! 用 Parley 高层 Layout 生成段落塑形、行断和视觉几何，并映射回 Caret 行框。

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
    FontFamilyName, GenericFamily, Language, Layout, LayoutContext, StyleProperty,
    WhiteSpaceCollapse,
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
    /// 文档字体偏好中至少一个候选族未安装。
    pub requested_family_missing: bool,
}

/// 段落布局的版心宽度、行距、空段度量和文档级基线锚点。
#[derive(Debug, Clone, Copy)]
pub struct ParagraphLayoutOptions {
    pub width_pt: f32,
    pub spacing: LineSpacing,
    pub empty_mark_metrics: FontLineMetrics,
    /// 由文档默认语言和默认字体测得；各段落、脚本和字体回退共享同一基线。
    pub baseline_anchor_metrics: FontLineMetrics,
}

/// 可复用 Parley LayoutContext；字体目录和字体数据由 FontCatalog 共享。
#[derive(Default)]
pub struct ParagraphLayouter {
    context: LayoutContext<[u8; 4]>,
    font_generation: Option<u64>,
}

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
        }
        let ParagraphLayoutOptions {
            width_pt,
            spacing,
            empty_mark_metrics,
            baseline_anchor_metrics,
        } = options;
        if !width_pt.is_finite() || width_pt <= 0.0 {
            return Err(ParagraphLayoutError::InvalidWidth);
        }
        if base_style.font_size_centipoints == 0 {
            return Err(ParagraphLayoutError::InvalidFontSize);
        }
        let spans = paragraph_style_spans(paragraph, settings, base_style);
        let text: String = spans.iter().map(|span| span.text).collect();
        let boundaries = boundaries_for_spans(&text, &spans);
        let decisions = Arc::new(line_break_decisions(&text, &boundaries));
        let available_families = Arc::clone(&catalog.family_names_lowercase);
        let mut requested_family_missing = false;
        let mut builder = self
            .context
            .ranged_builder(catalog.parley_context(), &text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(GenericFamily::SystemUi.into()));
        builder.push_default(StyleProperty::FontSize(
            f32::from(base_style.font_size_centipoints) * 0.01,
        ));
        builder.push_default(StyleProperty::WhiteSpaceCollapse(
            WhiteSpaceCollapse::BreakSpaces,
        ));

        let mut span_ranges = Vec::with_capacity(spans.len());
        let mut offset = 0usize;
        for span in &spans {
            let end = offset + span.text.len();
            span_ranges.push((offset..end, &span.style));
            offset = end;
        }
        let emoji_presentation = CodePointSetDataBorrowed::new::<props::EmojiPresentation>();
        let mut style_ranges: Vec<(Range<usize>, ShapingStyle, Script, bool)> = Vec::new();
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
                .is_some_and(|(span_range, _)| span_range.end <= range.start)
            {
                span_index += 1;
            }
            let cluster_text = &text[range.clone()];
            let script = strong_script(cluster_text)
                .or(previous_script)
                .unwrap_or_else(default_script);
            previous_script = Some(script);
            let emoji = is_emoji_grapheme(&text[range.clone()], &emoji_presentation);
            let mut fragment_index = span_index;
            while let Some((span_range, style)) = span_ranges.get(fragment_index) {
                if span_range.start >= range.end {
                    break;
                }
                let fragment = span_range.start.max(range.start)..span_range.end.min(range.end);
                if fragment.start < fragment.end {
                    if let Some((previous, previous_style, previous_script, previous_emoji)) =
                        style_ranges.last_mut()
                        && previous.end == fragment.start
                        && *previous_style == **style
                        && *previous_script == script
                        && *previous_emoji == emoji
                    {
                        previous.end = fragment.end;
                    } else {
                        style_ranges.push((fragment, (*style).clone(), script, emoji));
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
        for (range, style, script, emoji) in style_ranges {
            let key = (style.clone(), script, emoji);
            let family_names = family_names_by_key.entry(key).or_insert_with(|| {
                let candidates = settings.font_preferences().family_candidates(
                    crate::font_selection::FontSelectionContext {
                        script,
                        language: style.language.as_deref(),
                        math_context: false,
                        explicit_family: style.requested_family.as_deref(),
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
            let families = font_families(family_names, emoji);
            builder.push(
                StyleProperty::FontFamily(families.as_slice().into()),
                range.clone(),
            );
            builder.push(
                StyleProperty::FontSize(f32::from(style.font_size_centipoints) * 0.01),
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
            builder.push(
                StyleProperty::Locale(
                    style
                        .language
                        .as_deref()
                        .and_then(|language| Language::parse(language).ok()),
                ),
                range,
            );
        }

        let callback_index = Arc::new(AtomicUsize::new(0));
        let callback_cursor = Arc::clone(&callback_index);
        let callback_decisions = Arc::clone(&decisions);
        let line_break_override =
            move |_| callback_decisions.get(callback_cursor.fetch_add(1, Ordering::Relaxed));
        builder.set_line_break_override(Some(&line_break_override));
        let mut layout = builder.build(&text);
        layout.break_all_lines(Some(width_pt));
        if callback_index.load(Ordering::Relaxed) != decisions.len {
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
            empty_mark_metrics,
            baseline_anchor_metrics,
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
    ) -> Result<FontLineMetrics, ParagraphLayoutError> {
        let paragraph = Paragraph {
            id: caret_doc::NodeId::generate(),
            language: language.map(str::to_owned),
            layout: None,
            inlines: vec![caret_doc::InlineNode::Run(caret_doc::Run {
                text: " ".to_owned(),
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
                empty_mark_metrics: fallback,
                baseline_anchor_metrics: fallback,
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

fn font_families(names: &[String], emoji: bool) -> Vec<FontFamilyName<'_>> {
    let mut families = Vec::with_capacity(names.len() + 2);
    if emoji {
        families.push(FontFamilyName::Generic(GenericFamily::Emoji));
    }
    families.extend(names.iter().map(|family| FontFamilyName::named(family)));
    families.push(FontFamilyName::Generic(GenericFamily::SystemUi));
    families
}

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
            font_families(&names, true),
            vec![
                FontFamilyName::Generic(GenericFamily::Emoji),
                FontFamilyName::named("Preferred Text Family"),
                FontFamilyName::Generic(GenericFamily::SystemUi),
            ]
        );
        assert_eq!(
            font_families(&names, false),
            vec![
                FontFamilyName::named("Preferred Text Family"),
                FontFamilyName::Generic(GenericFamily::SystemUi),
            ]
        );
    }
}
