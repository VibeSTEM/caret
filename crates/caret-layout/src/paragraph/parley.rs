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
    FontCatalog, FontLineMetrics, LineMetricsError, LineSpacing, ParagraphLayout,
    ParagraphLayoutError, ParagraphLine, ResolvedDocumentTextSettings, ShapingStyle,
    calculate_line_metrics, shaping::boundaries_for_spans,
};

use crate::shaping::paragraph_style_spans;

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
    /// 每条行框的 Caret baseline 相对 Parley baseline 的调整量。
    pub baseline_adjustments_pt: Vec<f32>,
    /// 文档字体偏好中至少一个候选族未安装。
    pub requested_family_missing: bool,
}

/// 段落布局的版心宽度、行距策略和空段段落标记度量。
#[derive(Debug, Clone, Copy)]
pub struct ParagraphLayoutOptions {
    pub width_pt: f32,
    pub spacing: LineSpacing,
    pub empty_mark_metrics: FontLineMetrics,
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
        let decisions = Arc::new(decisions);
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
            baseline_adjustments_pt,
        } = make_paragraph_lines(&text, &layout, spacing, empty_mark_metrics)?;
        Ok((
            text,
            ParleyParagraphLayout {
                layout,
                paragraph: paragraph_layout,
                grapheme_boundaries: boundaries.grapheme,
                line_top_offsets_pt,
                line_box_tops_pt,
                parley_line_tops_pt,
                baseline_adjustments_pt,
                requested_family_missing,
            },
        ))
    }

    /// 用 Parley 生成不可见段落标记的字体度量；测量空格不进入正文。
    pub fn measure_empty_mark_metrics(
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

impl ParleyParagraphLayout {
    /// 字节偏移必须恰好落在扩展字素簇边界；非法位置不静默吸附。
    pub fn cursor_at(
        &self,
        byte_index: usize,
        affinity: parley::Affinity,
    ) -> Option<parley::editing::Cursor> {
        self.grapheme_boundaries
            .binary_search(&byte_index)
            .ok()
            .map(|_| parley::editing::Cursor::from_byte_index(&self.layout, byte_index, affinity))
    }

    /// 按视觉顺序移动，并跳过 Parley 在 Run 内部暴露的字素停靠点。
    pub fn move_visual(
        &self,
        cursor: parley::editing::Cursor,
        forward: bool,
    ) -> Option<parley::editing::Cursor> {
        let mut candidate = cursor;
        for _ in 0..=self.layout.text_len().saturating_add(1) {
            candidate = if forward {
                candidate.next_visual(&self.layout)
            } else {
                candidate.previous_visual(&self.layout)
            };
            if candidate == cursor {
                return None;
            }
            if self
                .grapheme_boundaries
                .binary_search(&candidate.index())
                .is_ok()
            {
                return Some(candidate);
            }
        }
        None
    }

    /// 点击命中时将内部字素停靠点吸附到视觉距离最近的完整字素边缘。
    pub fn cursor_from_point(&self, x: f32, y: f32) -> Option<parley::editing::Cursor> {
        use parley::editing::Cursor;

        if self.layout.text_len() == 0 {
            return Some(Cursor::from_byte_index(
                &self.layout,
                0,
                parley::Affinity::Downstream,
            ));
        }
        let (_, raw_y) = self.raw_line_y(y)?;
        let raw = Cursor::from_point(&self.layout, x, raw_y);
        if self.grapheme_boundaries.binary_search(&raw.index()).is_ok() {
            return Some(raw);
        }
        let mut previous = raw;
        let mut next = raw;
        for _ in 0..=self.layout.text_len().saturating_add(1) {
            previous = previous.previous_visual(&self.layout);
            if self
                .grapheme_boundaries
                .binary_search(&previous.index())
                .is_ok()
            {
                break;
            }
            if previous == raw {
                break;
            }
        }
        for _ in 0..=self.layout.text_len().saturating_add(1) {
            next = next.next_visual(&self.layout);
            if self
                .grapheme_boundaries
                .binary_search(&next.index())
                .is_ok()
            {
                break;
            }
            if next == raw {
                break;
            }
        }
        let previous_valid = self
            .grapheme_boundaries
            .binary_search(&previous.index())
            .is_ok();
        let next_valid = self
            .grapheme_boundaries
            .binary_search(&next.index())
            .is_ok();
        match (previous_valid, next_valid) {
            (true, true) => Some(
                if cursor_distance(previous, &self.layout, x, raw_y)
                    <= cursor_distance(next, &self.layout, x, raw_y)
                {
                    previous
                } else {
                    next
                },
            ),
            (true, false) => Some(previous),
            (false, true) => Some(next),
            (false, false) => None,
        }
    }

    /// 返回字素安全的 Parley selection 几何，并映射到 Caret 自定义行框。
    pub fn selection_geometry(
        &self,
        range: Range<usize>,
    ) -> Option<Vec<(parley::BoundingBox, usize)>> {
        if range.start > range.end || range.end > self.layout.text_len() {
            return None;
        }
        let start_index = self
            .grapheme_boundaries
            .partition_point(|boundary| *boundary <= range.start)
            .saturating_sub(1);
        let start = *self.grapheme_boundaries.get(start_index)?;
        let end_index = self
            .grapheme_boundaries
            .partition_point(|boundary| *boundary < range.end);
        let end = *self.grapheme_boundaries.get(end_index)?;
        if start == end {
            return Some(Vec::new());
        }
        let anchor = parley::editing::Cursor::from_byte_index(
            &self.layout,
            start,
            parley::Affinity::Downstream,
        );
        let focus =
            parley::editing::Cursor::from_byte_index(&self.layout, end, parley::Affinity::Upstream);
        Some(
            parley::editing::Selection::new(anchor, focus)
                .geometry(&self.layout)
                .into_iter()
                .map(|(mut bounds, line_index)| {
                    if let (Some(top_offset), Some(line)) = (
                        self.line_top_offsets_pt.get(line_index),
                        self.paragraph.lines.get(line_index),
                    ) {
                        bounds.y0 += f64::from(*top_offset);
                        bounds.y1 = bounds.y0 + f64::from(line.metrics.height_pt);
                    }
                    (bounds, line_index)
                })
                .collect(),
        )
    }

    /// 返回与自定义行框 baseline/溢出语义对齐的插入点矩形。
    pub fn cursor_geometry(
        &self,
        cursor: parley::editing::Cursor,
        width_pt: f32,
    ) -> parley::BoundingBox {
        if self.layout.text_len() == 0
            && let Some(line) = self.paragraph.lines.first()
        {
            return parley::BoundingBox::new(
                0.0,
                0.0,
                f64::from(width_pt),
                f64::from(line.metrics.height_pt),
            );
        }
        let mut bounds = cursor.geometry(&self.layout, width_pt);
        if self.parley_line_tops_pt.is_empty() {
            if let Some(line) = self.paragraph.lines.first() {
                return parley::BoundingBox::new(
                    0.0,
                    0.0,
                    f64::from(width_pt),
                    f64::from(line.metrics.height_pt),
                );
            }
            return bounds;
        }
        let cursor_mid_y = ((bounds.y0 + bounds.y1) / 2.0) as f32;
        let line_index = self
            .parley_line_tops_pt
            .partition_point(|top| *top <= cursor_mid_y)
            .saturating_sub(1)
            .min(self.parley_line_tops_pt.len() - 1);
        if let (Some(top_offset), Some(baseline_adjustment)) = (
            self.line_top_offsets_pt.get(line_index),
            self.baseline_adjustments_pt.get(line_index),
        ) {
            let offset = f64::from(*top_offset + *baseline_adjustment);
            bounds.y0 += offset;
            bounds.y1 += offset;
        }
        bounds
    }

    fn raw_line_y(&self, y: f32) -> Option<(usize, f32)> {
        if !y.is_finite() {
            return None;
        }
        let line_index = self
            .line_box_tops_pt
            .partition_point(|top| *top <= y)
            .saturating_sub(1)
            .min(self.line_box_tops_pt.len().checked_sub(1)?);
        let top = *self.line_box_tops_pt.get(line_index)?;
        let bottom = top + self.paragraph.lines.get(line_index)?.metrics.height_pt;
        let offset = *self.line_top_offsets_pt.get(line_index)?;
        Some((line_index, y.clamp(top, bottom) - offset))
    }
}

fn cursor_distance(
    cursor: parley::editing::Cursor,
    layout: &Layout<[u8; 4]>,
    x: f32,
    y: f32,
) -> f64 {
    let bounds = cursor.geometry(layout, 1.0);
    let x = f64::from(x);
    let y = f64::from(y);
    let dx = if x < bounds.x0 {
        bounds.x0 - x
    } else if x > bounds.x1 {
        x - bounds.x1
    } else {
        0.0
    };
    let dy = if y < bounds.y0 {
        bounds.y0 - y
    } else if y > bounds.y1 {
        y - bounds.y1
    } else {
        0.0
    };
    dx.mul_add(dx, dy * dy)
}

fn font_line_metrics(metrics: parley::FontMetrics) -> FontLineMetrics {
    FontLineMetrics {
        ascent_pt: metrics.ascent,
        descent_pt: metrics.descent,
        leading_pt: metrics.leading,
    }
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
            if !is_mandatory(mandatory.get(before)) && !is_mandatory(mandatory.get(character)) {
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

fn is_mandatory(value: props::LineBreak) -> bool {
    matches!(
        value,
        props::LineBreak::MandatoryBreak
            | props::LineBreak::CarriageReturn
            | props::LineBreak::LineFeed
            | props::LineBreak::NextLine
    )
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

struct ParagraphLineMapping {
    paragraph: ParagraphLayout,
    line_top_offsets_pt: Vec<f32>,
    line_box_tops_pt: Vec<f32>,
    parley_line_tops_pt: Vec<f32>,
    baseline_adjustments_pt: Vec<f32>,
}

fn make_paragraph_lines(
    text: &str,
    layout: &Layout<[u8; 4]>,
    spacing: LineSpacing,
    empty_mark_metrics: FontLineMetrics,
) -> Result<ParagraphLineMapping, ParagraphLayoutError> {
    if text.is_empty() {
        let metrics = calculate_line_metrics(&[], &[], empty_mark_metrics, spacing)?;
        return Ok(ParagraphLineMapping {
            paragraph: ParagraphLayout {
                lines: vec![ParagraphLine {
                    source_bytes: 0..0,
                    consumed_bytes: 0..0,
                    advance_pt: 0.0,
                    overflows_width: false,
                    metrics,
                }],
                total_height_pt: metrics.height_pt,
            },
            line_top_offsets_pt: vec![0.0],
            line_box_tops_pt: vec![0.0],
            parley_line_tops_pt: vec![0.0],
            baseline_adjustments_pt: vec![0.0],
        });
    }
    let lines: Vec<_> = layout.lines().collect();
    if lines.is_empty() {
        return Err(ParagraphLayoutError::InconsistentShaping);
    }
    let mut paragraph_lines = Vec::with_capacity(lines.len());
    let mut line_top_offsets_pt = Vec::with_capacity(lines.len());
    let mut line_box_tops_pt = Vec::with_capacity(lines.len());
    let mut parley_line_tops_pt = Vec::with_capacity(lines.len());
    let mut baseline_adjustments_pt = Vec::with_capacity(lines.len());
    let mut font_metrics = Vec::new();
    let mut top_pt = 0.0f32;
    for (index, line) in lines.iter().enumerate() {
        let metrics = line.metrics();
        let source_bytes = line.text_range();
        if source_bytes.start > source_bytes.end
            || source_bytes.end > text.len()
            || !text.is_char_boundary(source_bytes.start)
            || !text.is_char_boundary(source_bytes.end)
            || !metrics.advance.is_finite()
            || !metrics.line_height.is_finite()
        {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
        let consumed_end = lines
            .get(index + 1)
            .map_or(text.len(), |next| next.text_range().start);
        if consumed_end < source_bytes.end || consumed_end > text.len() {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
        font_metrics.clear();
        font_metrics.extend(
            line.runs()
                .map(|run| font_line_metrics(*run.font_metrics())),
        );
        let line_metrics = calculate_line_metrics(&font_metrics, &[], empty_mark_metrics, spacing)
            .map_err(|error| match error {
                LineMetricsError::InvalidFontMetrics
                | LineMetricsError::InvalidObjectMetrics
                | LineMetricsError::InvalidLineSpacing => {
                    ParagraphLayoutError::InvalidLineMetrics(error)
                }
            })?;
        let raw_top = metrics.block_min_coord;
        let raw_baseline_from_top = metrics.baseline - raw_top;
        if !raw_top.is_finite() || !raw_baseline_from_top.is_finite() {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
        line_top_offsets_pt.push(top_pt - raw_top);
        line_box_tops_pt.push(top_pt);
        parley_line_tops_pt.push(raw_top);
        baseline_adjustments_pt.push(line_metrics.baseline_pt - raw_baseline_from_top);
        paragraph_lines.push(ParagraphLine {
            source_bytes: source_bytes.clone(),
            consumed_bytes: source_bytes.start..consumed_end,
            advance_pt: metrics.advance,
            overflows_width: metrics.advance > layout.layout_max_advance(),
            metrics: line_metrics,
        });
        top_pt += line_metrics.height_pt;
    }
    Ok(ParagraphLineMapping {
        paragraph: ParagraphLayout {
            lines: paragraph_lines,
            total_height_pt: top_pt,
        },
        line_top_offsets_pt,
        line_box_tops_pt,
        parley_line_tops_pt,
        baseline_adjustments_pt,
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
