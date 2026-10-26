//! 将整段 shaping clusters 按 Unicode 断点组合成行框和源文本范围。

use std::ops::Range;

use crate::{
    FontLineMetrics, InlineObjectMetrics, LineMetrics, LineMetricsError, LineSpacing,
    ShapedParagraph, calculate_line_metrics,
};
use icu_properties::{CodePointMapData, props::LineBreak as LineBreakClass};

/// 一条段落行对原文的引用和行框数据。
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphLine {
    /// 可见文字范围；软换行符不包含在这里。
    pub source_bytes: Range<usize>,
    /// 本行消费的范围，包含末尾软换行符。
    pub consumed_bytes: Range<usize>,
    /// 重新塑形后的行推进宽度，单位为 pt。
    pub advance_pt: f32,
    /// 重塑后的宽度是否仍超过可用行宽。
    pub overflows_width: bool,
    pub metrics: LineMetrics,
}

/// 段落的行布局；每行仍引用原文，不复制正文。
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphLayout {
    pub lines: Vec<ParagraphLine>,
    pub total_height_pt: f32,
}

/// 段落行布局输入非法时返回错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphLayoutError {
    InvalidWidth,
    InconsistentShaping,
    InvalidClusterMetrics,
    InvalidObjectRange,
    InvalidLineMetrics(LineMetricsError),
}

impl std::fmt::Display for ParagraphLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidWidth => "layout.invalid_width",
            Self::InconsistentShaping => "layout.inconsistent_shaping",
            Self::InvalidClusterMetrics => "layout.invalid_cluster_metrics",
            Self::InvalidObjectRange => "layout.invalid_object_range",
            Self::InvalidLineMetrics(error) => return error.fmt(formatter),
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ParagraphLayoutError {}

impl From<LineMetricsError> for ParagraphLayoutError {
    fn from(error: LineMetricsError) -> Self {
        Self::InvalidLineMetrics(error)
    }
}

#[derive(Clone)]
struct Cluster {
    bytes: Range<usize>,
    advance_pt: f32,
    metrics: FontLineMetrics,
}

/// 按 UAX #14 机会、完整字素和 Parley shaping cluster 选择断点。
///
/// 每段只塑形一次，避免逐行重新分析破坏段落双向上下文，也避免碎片化样式范围导致
/// O(n²) 的重复展平。最终行宽和字体度量来自完整段落的 shaping cluster。
pub fn layout_shaped_paragraph(
    text: &str,
    shaped: &ShapedParagraph,
    available_width_pt: f32,
    spacing: LineSpacing,
    empty_mark_metrics: FontLineMetrics,
    objects: &[InlineObjectMetrics],
) -> Result<ParagraphLayout, ParagraphLayoutError> {
    if !available_width_pt.is_finite() || available_width_pt <= 0.0 {
        return Err(ParagraphLayoutError::InvalidWidth);
    }
    let raw_clusters = shaped.text.shaped_clusters();
    if shaped.cluster_bytes.len() != raw_clusters.len() {
        return Err(ParagraphLayoutError::InconsistentShaping);
    }
    let mut raw_cluster_metrics = vec![None; raw_clusters.len()];
    for run in shaped.text.runs() {
        let start = run.shaped_clusters_range.start as usize;
        let end = run.shaped_clusters_range.end as usize;
        let Some(metrics) = raw_cluster_metrics.get_mut(start..end) else {
            return Err(ParagraphLayoutError::InconsistentShaping);
        };
        metrics.fill(Some(FontLineMetrics::from(run.font_metrics)));
    }
    let mut clusters: Vec<_> = raw_clusters
        .iter()
        .zip(shaped.cluster_bytes.iter())
        .enumerate()
        .map(|(index, (raw, bytes))| {
            let Some(Some(metrics)) = raw_cluster_metrics.get(index) else {
                return Err(ParagraphLayoutError::InconsistentShaping);
            };
            if bytes.start >= bytes.end
                || bytes.end > text.len()
                || !text.is_char_boundary(bytes.start)
                || !text.is_char_boundary(bytes.end)
                || !raw.advance.is_finite()
                || raw.advance < 0.0
            {
                return Err(ParagraphLayoutError::InvalidClusterMetrics);
            }
            let advance_pt =
                object_for_range(objects, bytes).map_or(raw.advance, |object| object.advance_pt);
            Ok(Cluster {
                bytes: bytes.clone(),
                advance_pt,
                metrics: *metrics,
            })
        })
        .collect::<Result<_, _>>()?;
    clusters.sort_by_key(|cluster| (cluster.bytes.start, cluster.bytes.end));
    validate_cluster_order(text, &clusters)?;
    if (text.is_empty() && !clusters.is_empty()) || (!text.is_empty() && clusters.is_empty()) {
        return Err(ParagraphLayoutError::InconsistentShaping);
    }
    validate_objects(text, &clusters, objects)?;
    let line_context = LineContext {
        text,
        available_width_pt,
        spacing,
        empty_mark_metrics,
        objects,
        clusters: &clusters,
    };

    let mut lines = Vec::new();
    let mut cluster_index = 0usize;
    let mut byte_cursor = 0usize;
    while cluster_index < clusters.len() {
        if let Some((newline_index, next_index, consumed_end)) =
            mandatory_break_after(text, &clusters, cluster_index)
        {
            let newline = &clusters[newline_index];
            lines.push(
                line_context
                    .make_line(byte_cursor..newline.bytes.start, byte_cursor..consumed_end)?,
            );
            byte_cursor = consumed_end;
            cluster_index = next_index;
            continue;
        }

        let start_index = cluster_index;
        let mut advance_pt = 0.0;
        let mut last_legal_end = None;
        let mut selected_end = None;
        let mut mandatory_break = None;
        while cluster_index < clusters.len() {
            let cluster = &clusters[cluster_index];
            if let Some((newline_index, next_index, consumed_end)) =
                mandatory_break_after(text, &clusters, cluster_index)
            {
                mandatory_break = Some((newline_index, next_index, consumed_end));
                break;
            }
            advance_pt += cluster.advance_pt;
            cluster_index += 1;
            let boundary = cluster.bytes.end;
            if is_legal_boundary(shaped, boundary) {
                if advance_pt > available_width_pt {
                    selected_end = Some(last_legal_end.unwrap_or((cluster_index, advance_pt)));
                    break;
                }
                last_legal_end = Some((cluster_index, advance_pt));
            }
            if advance_pt > available_width_pt && last_legal_end.is_some() {
                selected_end = last_legal_end;
                break;
            }
        }

        if let Some((newline_index, next_index, consumed_end)) = mandatory_break {
            let newline = &clusters[newline_index];
            lines.push(
                line_context
                    .make_line(byte_cursor..newline.bytes.start, byte_cursor..consumed_end)?,
            );
            byte_cursor = consumed_end;
            cluster_index = next_index;
            continue;
        }

        let (line_end, _line_advance) = selected_end.unwrap_or((cluster_index, advance_pt));
        if line_end <= start_index {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
        let end_byte = clusters[line_end - 1].bytes.end;
        lines.push(line_context.make_line(byte_cursor..end_byte, byte_cursor..end_byte)?);
        byte_cursor = end_byte;
        cluster_index = line_end;
    }

    if lines.is_empty() || byte_cursor < text.len() || ends_with_mandatory_break(text) {
        lines.push(line_context.make_line(byte_cursor..text.len(), byte_cursor..text.len())?);
    }

    let total_height_pt: f32 = lines.iter().map(|line| line.metrics.height_pt).sum();
    if !total_height_pt.is_finite() {
        return Err(ParagraphLayoutError::InvalidClusterMetrics);
    }
    Ok(ParagraphLayout {
        lines,
        total_height_pt,
    })
}

fn validate_cluster_order(text: &str, clusters: &[Cluster]) -> Result<(), ParagraphLayoutError> {
    let mut previous_end = 0;
    for cluster in clusters {
        if cluster.bytes.start != previous_end {
            return Err(ParagraphLayoutError::InvalidClusterMetrics);
        }
        previous_end = cluster.bytes.end;
    }
    if previous_end != text.len() {
        return Err(ParagraphLayoutError::InvalidClusterMetrics);
    }
    Ok(())
}

fn validate_objects(
    text: &str,
    clusters: &[Cluster],
    objects: &[InlineObjectMetrics],
) -> Result<(), ParagraphLayoutError> {
    let mut previous_end = 0;
    for object in objects {
        if object.source_bytes.start < previous_end
            || object.source_bytes.end > text.len()
            || !text.is_char_boundary(object.source_bytes.start)
            || !text.is_char_boundary(object.source_bytes.end)
            || clusters
                .binary_search_by(|cluster| {
                    (cluster.bytes.start, cluster.bytes.end)
                        .cmp(&(object.source_bytes.start, object.source_bytes.end))
                })
                .is_err()
        {
            return Err(ParagraphLayoutError::InvalidObjectRange);
        }
        previous_end = object.source_bytes.end;
    }
    Ok(())
}

fn object_for_range<'a>(
    objects: &'a [InlineObjectMetrics],
    bytes: &Range<usize>,
) -> Option<&'a InlineObjectMetrics> {
    let index = objects.partition_point(|object| object.source_bytes.start < bytes.start);
    objects
        .get(index)
        .filter(|object| object.source_bytes == *bytes)
}

fn is_legal_boundary(shaped: &ShapedParagraph, byte_offset: usize) -> bool {
    shaped.line_breaks.binary_search(&byte_offset).is_ok()
        && shaped
            .grapheme_boundaries
            .binary_search(&byte_offset)
            .is_ok()
}

fn mandatory_break_after(
    text: &str,
    clusters: &[Cluster],
    index: usize,
) -> Option<(usize, usize, usize)> {
    let cluster = clusters.get(index)?;
    let separator = text.get(cluster.bytes.clone())?;
    if separator == "\r" {
        let next = clusters.get(index + 1);
        if next.is_some_and(|next| text.get(next.bytes.clone()) == Some("\n")) {
            let next = next?;
            return Some((index, index + 2, next.bytes.end));
        }
    }
    separator
        .chars()
        .all(is_mandatory_break_character)
        .then_some((index, index + 1, cluster.bytes.end))
}

fn ends_with_mandatory_break(text: &str) -> bool {
    text.chars()
        .next_back()
        .is_some_and(is_mandatory_break_character)
}

fn is_mandatory_break_character(character: char) -> bool {
    matches!(
        CodePointMapData::<LineBreakClass>::new().get(character),
        LineBreakClass::MandatoryBreak
            | LineBreakClass::CarriageReturn
            | LineBreakClass::LineFeed
            | LineBreakClass::NextLine
    )
}

struct LineContext<'a> {
    text: &'a str,
    available_width_pt: f32,
    spacing: LineSpacing,
    empty_mark_metrics: FontLineMetrics,
    objects: &'a [InlineObjectMetrics],
    clusters: &'a [Cluster],
}

impl LineContext<'_> {
    fn make_line(
        &self,
        source_bytes: Range<usize>,
        consumed_bytes: Range<usize>,
    ) -> Result<ParagraphLine, ParagraphLayoutError> {
        let text = self.text;
        let objects = self.objects;
        if source_bytes.end > text.len()
            || !text.is_char_boundary(source_bytes.start)
            || !text.is_char_boundary(source_bytes.end)
        {
            return Err(ParagraphLayoutError::InconsistentShaping);
        }
        let first_object =
            objects.partition_point(|object| object.source_bytes.end <= source_bytes.start);
        let local_objects: Vec<_> = objects[first_object..]
            .iter()
            .take_while(|object| object.source_bytes.start < source_bytes.end)
            .filter(|object| object.source_bytes.end <= source_bytes.end)
            .cloned()
            .map(|mut object| {
                object.source_bytes.start -= source_bytes.start;
                object.source_bytes.end -= source_bytes.start;
                object
            })
            .collect();
        let mut font_metrics = Vec::new();
        let mut advance_pt = 0.0;
        let first_cluster = self
            .clusters
            .partition_point(|cluster| cluster.bytes.end <= source_bytes.start);
        for cluster in self.clusters[first_cluster..]
            .iter()
            .take_while(|cluster| cluster.bytes.start < source_bytes.end)
            .filter(|cluster| cluster.bytes.end <= source_bytes.end)
        {
            let local_range = (cluster.bytes.start - source_bytes.start)
                ..(cluster.bytes.end - source_bytes.start);
            let object = object_for_range(&local_objects, &local_range);
            if let Some(object) = object {
                advance_pt += object.advance_pt;
            } else {
                advance_pt += cluster.advance_pt;
                font_metrics.push(cluster.metrics);
            }
        }
        let metrics = calculate_line_metrics(
            &font_metrics,
            &local_objects,
            self.empty_mark_metrics,
            self.spacing,
        )?;
        Ok(ParagraphLine {
            source_bytes,
            consumed_bytes,
            advance_pt,
            overflows_width: advance_pt > self.available_width_pt,
            metrics,
        })
    }
}
