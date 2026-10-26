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
            parley_line_heights_pt,
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
                parley_line_heights_pt,
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
    pub fn cursor_at(&self, byte_index: usize, affinity: VisualAffinity) -> Option<VisualCursor> {
        self.grapheme_boundaries
            .binary_search(&byte_index)
            .ok()
            .map(|_| VisualCursor {
                byte_index,
                affinity,
            })
    }

    /// 按视觉顺序移动，并跳过 Parley 在 Run 内部暴露的字素停靠点。
    pub fn move_visual(&self, cursor: VisualCursor, forward: bool) -> Option<VisualCursor> {
        let mut candidate = self.to_parley_cursor(cursor);
        for _ in 0..=self.layout.text_len().saturating_add(1) {
            candidate = if forward {
                candidate.next_visual(&self.layout)
            } else {
                candidate.previous_visual(&self.layout)
            };
            if candidate == self.to_parley_cursor(cursor) {
                return None;
            }
            if self
                .grapheme_boundaries
                .binary_search(&candidate.index())
                .is_ok()
            {
                return Some(Self::from_parley_cursor(candidate));
            }
        }
        None
    }

    /// 点击命中时将内部字素停靠点吸附到视觉距离最近的完整字素边缘。
    pub fn cursor_from_point(&self, x: f32, y: f32) -> Option<ParagraphHit> {
        if self.layout.text_len() == 0 {
            return self
                .cursor_at(0, VisualAffinity::Downstream)
                .map(|cursor| ParagraphHit {
                    cursor,
                    logical_affinity: HitAffinity::Downstream,
                });
        }
        let (line_index, _) = self.raw_line_y(y)?;
        self.cursor_from_point_on_line(x, line_index)
    }

    /// 在已选定的 Caret 行内按 x 坐标命中，避免行高差异将命中带到相邻 Parley 行。
    pub fn cursor_from_point_on_line(&self, x: f32, line_index: usize) -> Option<ParagraphHit> {
        use parley::editing::Cursor;

        if !x.is_finite() {
            return None;
        }
        if self.layout.text_len() == 0 {
            return self
                .cursor_at(0, VisualAffinity::Downstream)
                .map(|cursor| ParagraphHit {
                    cursor,
                    logical_affinity: HitAffinity::Downstream,
                });
        }
        let raw_top = *self.parley_line_tops_pt.get(line_index)?;
        let raw_height = *self.parley_line_heights_pt.get(line_index)?;
        if !raw_top.is_finite() || !raw_height.is_finite() || raw_height <= 0.0 {
            return None;
        }
        let raw_y = raw_top + raw_height / 2.0;
        let raw = Cursor::from_point(&self.layout, x, raw_y);
        let raw_index = raw.index();
        let (byte_index, logical_affinity) = match (
            self.grapheme_boundaries.binary_search(&raw_index),
            raw.affinity(),
        ) {
            (Ok(_), affinity) => {
                let cursor = Self::from_parley_cursor(raw);
                let bounds = self.cursor_geometry(cursor, 0.0);
                let same_position = [parley::Affinity::Upstream, parley::Affinity::Downstream]
                    .into_iter()
                    .filter(|candidate_affinity| *candidate_affinity != affinity)
                    .any(|candidate_affinity| {
                        let other =
                            Cursor::from_byte_index(&self.layout, raw_index, candidate_affinity);
                        let other_bounds = other.geometry(&self.layout, 0.0);
                        (bounds.x0 - other_bounds.x0).abs() <= 0.0001
                    });
                let logical_affinity =
                    if same_position && (f64::from(x) - bounds.x0).abs() <= 0.0001 {
                        HitAffinity::Downstream
                    } else {
                        hit_affinity(affinity)
                    };
                let visual_cursor = if same_position && (f64::from(x) - bounds.x0).abs() <= 0.0001 {
                    Self::from_parley_cursor(Cursor::from_byte_index(
                        &self.layout,
                        raw_index,
                        parley::Affinity::Downstream,
                    ))
                } else {
                    cursor
                };
                return Some(ParagraphHit {
                    cursor: visual_cursor,
                    logical_affinity,
                });
            }
            (Err(index), parley::Affinity::Downstream) => (
                *self.grapheme_boundaries.get(index.saturating_sub(1))?,
                HitAffinity::Downstream,
            ),
            (Err(index), parley::Affinity::Upstream) => {
                (*self.grapheme_boundaries.get(index)?, HitAffinity::Upstream)
            }
        };
        let affinity = match raw.affinity() {
            parley::Affinity::Upstream => VisualAffinity::Upstream,
            parley::Affinity::Downstream => VisualAffinity::Downstream,
        };
        Some(ParagraphHit {
            cursor: self.cursor_at(byte_index, affinity)?,
            logical_affinity,
        })
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
    pub fn cursor_geometry(&self, cursor: VisualCursor, width_pt: f32) -> parley::BoundingBox {
        self.cursor_geometry_with_line(cursor, width_pt)
            .map(|(bounds, _)| bounds)
            .unwrap_or_else(|| {
                self.to_parley_cursor(cursor)
                    .geometry(&self.layout, width_pt)
            })
    }

    /// 返回插入点矩形和所属的 Caret 行索引。
    pub fn cursor_geometry_with_line(
        &self,
        cursor: VisualCursor,
        width_pt: f32,
    ) -> Option<(parley::BoundingBox, usize)> {
        let cursor = self.to_parley_cursor(cursor);
        if self.layout.text_len() == 0
            && let Some(line) = self.paragraph.lines.first()
        {
            return Some((
                parley::BoundingBox::new(
                    0.0,
                    0.0,
                    f64::from(width_pt),
                    f64::from(line.metrics.height_pt),
                ),
                0,
            ));
        }
        let (mut bounds, line_index) =
            cursor_geometry_on_logical_cluster(&self.layout, cursor, width_pt).unwrap_or_else(
                || {
                    let bounds = cursor.geometry(&self.layout, width_pt);
                    let cursor_mid_y = ((bounds.y0 + bounds.y1) / 2.0) as f32;
                    let line_index = self
                        .parley_line_tops_pt
                        .partition_point(|top| *top <= cursor_mid_y)
                        .saturating_sub(1)
                        .min(self.parley_line_tops_pt.len().saturating_sub(1));
                    (bounds, line_index)
                },
            );
        if self.parley_line_tops_pt.is_empty() {
            if let Some(line) = self.paragraph.lines.first() {
                return Some((
                    parley::BoundingBox::new(
                        0.0,
                        0.0,
                        f64::from(width_pt),
                        f64::from(line.metrics.height_pt),
                    ),
                    0,
                ));
            }
            return Some((bounds, line_index));
        }
        if let (Some(top_offset), Some(baseline_adjustment)) = (
            self.line_top_offsets_pt.get(line_index),
            self.baseline_adjustments_pt.get(line_index),
        ) {
            let offset = f64::from(*top_offset + *baseline_adjustment);
            bounds.y0 += offset;
            bounds.y1 += offset;
        }
        Some((bounds, line_index))
    }

    fn to_parley_cursor(&self, cursor: VisualCursor) -> parley::editing::Cursor {
        parley::editing::Cursor::from_byte_index(
            &self.layout,
            cursor.byte_index,
            match cursor.affinity {
                VisualAffinity::Upstream => parley::Affinity::Upstream,
                VisualAffinity::Downstream => parley::Affinity::Downstream,
            },
        )
    }

    fn from_parley_cursor(cursor: parley::editing::Cursor) -> VisualCursor {
        VisualCursor {
            byte_index: cursor.index(),
            affinity: match cursor.affinity() {
                parley::Affinity::Upstream => VisualAffinity::Upstream,
                parley::Affinity::Downstream => VisualAffinity::Downstream,
            },
        }
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
        if y < top || y > bottom {
            return None;
        }
        let raw_top = *self.parley_line_tops_pt.get(line_index)?;
        let raw_height = *self.parley_line_heights_pt.get(line_index)?;
        if !raw_top.is_finite() || !raw_height.is_finite() || raw_height <= 0.0 {
            return None;
        }
        Some((line_index, raw_top + raw_height / 2.0))
    }
}

fn cursor_geometry_on_logical_cluster(
    layout: &Layout<[u8; 4]>,
    cursor: parley::editing::Cursor,
    width: f32,
) -> Option<(parley::BoundingBox, usize)> {
    let [upstream, downstream] = cursor.logical_clusters(layout);
    if upstream.is_some_and(|cluster| cluster.is_hard_line_break()) {
        if let Some(cluster) = downstream {
            return cluster_cursor_geometry(cluster, cluster.is_rtl(), width);
        }
        let line_index = layout.len().checked_sub(1)?;
        let line = layout.get(line_index)?;
        let metrics = line.metrics();
        return Some((
            parley::BoundingBox::new(
                f64::from(metrics.offset),
                f64::from(metrics.block_min_coord.min(metrics.content_block_min_coord)),
                f64::from(metrics.offset + width),
                f64::from(metrics.block_max_coord.max(metrics.content_block_max_coord)),
            ),
            line_index,
        ));
    }
    let (cluster, at_end) = match cursor.affinity() {
        parley::Affinity::Upstream => {
            let cluster = upstream.or(downstream)?;
            (cluster, !cluster.is_rtl())
        }
        parley::Affinity::Downstream => {
            let cluster = downstream.or(upstream)?;
            (cluster, cluster.is_rtl())
        }
    };
    cluster_cursor_geometry(cluster, at_end, width)
}

fn cluster_cursor_geometry(
    cluster: parley::layout::Cluster<'_, [u8; 4]>,
    at_end: bool,
    width: f32,
) -> Option<(parley::BoundingBox, usize)> {
    let mut x = cluster.visual_offset()?;
    if at_end {
        x += cluster.advance();
    }
    let line = cluster.line();
    let metrics = line.metrics();
    let bounds = parley::BoundingBox::new(
        f64::from(x),
        f64::from(metrics.block_min_coord.min(metrics.content_block_min_coord)),
        f64::from(x + width),
        f64::from(metrics.block_max_coord.max(metrics.content_block_max_coord)),
    );
    Some((bounds, cluster.path().line_index()))
}

fn hit_affinity(affinity: parley::Affinity) -> HitAffinity {
    match affinity {
        parley::Affinity::Upstream => HitAffinity::Upstream,
        parley::Affinity::Downstream => HitAffinity::Downstream,
    }
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
    parley_line_heights_pt: Vec<f32>,
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
            parley_line_heights_pt: vec![0.0],
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
    let mut parley_line_heights_pt = Vec::with_capacity(lines.len());
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
            || metrics.line_height <= 0.0
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
        parley_line_heights_pt.push(metrics.line_height);
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
        parley_line_heights_pt,
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
