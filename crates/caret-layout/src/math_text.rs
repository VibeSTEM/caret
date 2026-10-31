//! 使用 Caret 的 Parley 正文排版和字体回退塑形数学 `\text{...}`。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::{
    FontCatalog, FontLineMetrics, GlyphFontData, ParagraphLayoutOptions, ParagraphLayouter,
    ParleyParagraphLayout, ResolvedDocumentTextSettings, ShapingStyle,
};
use caret_doc::{NodeId, Paragraph, Run};
use caret_math::{MathTextMetrics, MathTextShaper, MathTextStyle};
use linebender_resource_handle::Blob;
use parley::{InlineBox, InlineBoxKind, VerticalAlign};
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, OnceLock},
};

const MATH_TEXT_LAYOUT_WIDTH_PT: f32 = 10_000.0;
const MICRO_POINTS_PER_POINT: f64 = 1_000_000.0;
const MICRO_POINTS_PER_CENTIPOINT: i64 = 10_000;

/// Shared embedded STIX Two Math face for glyph-id rasterization.
pub fn stix_two_math_font_data() -> GlyphFontData {
    static FONT: OnceLock<GlyphFontData> = OnceLock::new();
    FONT.get_or_init(|| GlyphFontData::new(Blob::new(Arc::new(caret_math::MATH_FONT_BYTES)), 0))
        .clone()
}

/// Provider-local Parley layouts referenced by `MathLayoutItem::ExternalText` handles.
pub struct ParleyMathTextShaper<'a> {
    state: RefCell<ProviderState<'a>>,
}

struct ProviderState<'a> {
    catalog: &'a mut FontCatalog,
    layouter: &'a mut ParagraphLayouter,
    settings: &'a ResolvedDocumentTextSettings,
    base_style: &'a ShapingStyle,
    next_handle: u64,
    active_handles: Option<Vec<u64>>,
    line_metrics: HashMap<(ShapingStyle, Option<String>), FontLineMetrics>,
    pending_resources: HashMap<u64, Arc<ParleyParagraphLayout>>,
    resources: HashMap<u64, Arc<ParleyParagraphLayout>>,
}

impl<'a> ParleyMathTextShaper<'a> {
    /// Borrows the active document's font catalog and reusable Parley layouter.
    pub fn new(
        catalog: &'a mut FontCatalog,
        layouter: &'a mut ParagraphLayouter,
        settings: &'a ResolvedDocumentTextSettings,
        base_style: &'a ShapingStyle,
    ) -> Self {
        Self {
            state: RefCell::new(ProviderState {
                catalog,
                layouter,
                settings,
                base_style,
                next_handle: 1,
                active_handles: None,
                line_metrics: HashMap::new(),
                pending_resources: HashMap::new(),
                resources: HashMap::new(),
            }),
        }
    }

    /// Moves successfully committed Parley layouts into the owning document snapshot.
    pub fn take_resources(&self) -> HashMap<u64, Arc<ParleyParagraphLayout>> {
        std::mem::take(&mut self.state.borrow_mut().resources)
    }
}

impl MathTextShaper for ParleyMathTextShaper<'_> {
    fn begin_layout(&self) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        if state.active_handles.is_some() {
            return Err("a Parley math text transaction is already active".into());
        }
        state.active_handles = Some(Vec::new());
        Ok(())
    }

    fn shape_text(
        &self,
        text: &str,
        math_style: MathTextStyle,
        font_size_micro_pt: i64,
    ) -> Result<MathTextMetrics, String> {
        if text.is_empty() || font_size_micro_pt <= 0 {
            return Err("math text must be nonempty and have a positive font size".into());
        }
        let centipoints = font_size_micro_pt
            .checked_add(MICRO_POINTS_PER_CENTIPOINT / 2)
            .and_then(|value| value.checked_div(MICRO_POINTS_PER_CENTIPOINT))
            .and_then(|value| u16::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| "math text font size is outside Parley's range".to_owned())?;
        let mut state = self.state.borrow_mut();
        if state.active_handles.is_none() {
            return Err("math text shaping requires an active layout transaction".into());
        }

        let mut style = state.base_style.clone();
        style.font_size_centipoints = centipoints;
        match math_style {
            MathTextStyle::Rm | MathTextStyle::Text => {
                style.bold = false;
                style.italic = false;
                style.requested_family = None;
            }
            MathTextStyle::Bf => style.bold = true,
            MathTextStyle::It => style.italic = true,
            MathTextStyle::Sf => style.requested_family = Some("sans-serif".into()),
            MathTextStyle::Tt => style.requested_family = Some("monospace".into()),
            MathTextStyle::Boldsymbol | MathTextStyle::Pmb => style.bold = true,
            unsupported => {
                return Err(format!(
                    "math text style {unsupported:?} has no Caret font mapping"
                ));
            }
        }
        let language = style
            .language
            .clone()
            .or_else(|| state.settings.default_language().map(str::to_owned));
        let metric_key = (style.clone(), language.clone());
        let line_grid_metrics = if let Some(metrics) = state.line_metrics.get(&metric_key) {
            *metrics
        } else {
            let metrics = {
                let ProviderState {
                    catalog,
                    layouter,
                    settings,
                    ..
                } = &mut *state;
                layouter
                    .measure_font_metrics(catalog, settings, &style, language.as_deref(), Some('M'))
                    .map_err(|error| error.to_string())?
            };
            state.line_metrics.insert(metric_key, metrics);
            metrics
        };

        let paragraph = text_paragraph(text, language.as_deref());
        let (shaped_text, layout) = {
            let ProviderState {
                catalog,
                layouter,
                settings,
                ..
            } = &mut *state;
            layouter
                .layout(
                    catalog,
                    &paragraph,
                    settings,
                    &style,
                    ParagraphLayoutOptions {
                        width_pt: MATH_TEXT_LAYOUT_WIDTH_PT,
                        spacing: crate::LineSpacing::Natural,
                        line_grid_metrics,
                        line_grid_font_size_centipoints: centipoints,
                        first_line_indent_chars: None,
                    },
                )
                .map_err(|error| error.to_string())?
        };
        if shaped_text != text || layout.paragraph.lines.len() != 1 {
            return Err("math text did not shape as one complete line".into());
        }
        let line = layout
            .paragraph
            .lines
            .first()
            .ok_or_else(|| "Parley returned no line for math text".to_owned())?;
        let baseline = line.metrics.baseline_pt;
        let descent = line.metrics.height_pt - baseline;
        let width_micro_pt = to_micro_points(line.advance_pt)?;
        let ascent_micro_pt = to_micro_points(baseline)?;
        let descent_micro_pt = to_micro_points(descent)?;
        let handle = state.next_handle;
        state.next_handle = state
            .next_handle
            .checked_add(1)
            .ok_or_else(|| "math text handle space exhausted".to_owned())?;
        let active_handles = state
            .active_handles
            .as_mut()
            .ok_or_else(|| "math text transaction ended while shaping".to_owned())?;
        active_handles.push(handle);
        state.pending_resources.insert(handle, Arc::new(layout));
        Ok(MathTextMetrics {
            handle,
            width_micro_pt,
            ascent_micro_pt,
            descent_micro_pt,
        })
    }

    fn finish_layout(&self, committed: bool) {
        let mut state = self.state.borrow_mut();
        let Some(handles) = state.active_handles.take() else {
            return;
        };
        for handle in handles {
            if committed {
                if let Some(layout) = state.pending_resources.remove(&handle) {
                    state.resources.insert(handle, layout);
                }
            } else {
                state.pending_resources.remove(&handle);
            }
        }
    }
}

fn text_paragraph(text: &str, language: Option<&str>) -> Paragraph {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.language = language.map(str::to_owned);
    paragraph.inlines.push(caret_doc::InlineNode::Run(Run {
        text: text.to_owned(),
        language: language.map(str::to_owned),
        props: Default::default(),
        extra: Default::default(),
    }));
    paragraph
}

fn to_micro_points(points: f32) -> Result<i64, String> {
    if !points.is_finite() || points < 0.0 {
        return Err("Parley returned invalid math text geometry".into());
    }
    let scaled = (f64::from(points) * MICRO_POINTS_PER_POINT).round();
    if !scaled.is_finite() || scaled > i64::MAX as f64 {
        return Err("Parley math text geometry exceeds the integer range".into());
    }
    // SAFETY: `scaled` is finite, nonnegative, integral after round, and <= i64::MAX.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let integer = scaled as i64;
    Ok(integer)
}

/// Inline-box metrics used by Parley to reserve the formula's advance in a paragraph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MathInlineBoxMetrics {
    pub width_pt: f32,
    pub ascent_pt: f32,
    pub descent_pt: f32,
}

impl MathInlineBoxMetrics {
    /// Builds an in-flow Parley box with its baseline at the formula baseline.
    pub fn inline_box(self, id: u64, index: usize) -> Result<InlineBox, String> {
        if [self.width_pt, self.ascent_pt, self.descent_pt]
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
            || self.ascent_pt + self.descent_pt <= 0.0
        {
            return Err("invalid math inline-box geometry".into());
        }
        let height = self.ascent_pt + self.descent_pt;
        if !height.is_finite() {
            return Err("math inline-box height exceeds Parley's range".into());
        }
        Ok(InlineBox {
            id,
            kind: InlineBoxKind::InFlow,
            index,
            width: self.width_pt,
            height,
            baseline: Some(self.ascent_pt),
            vertical_align: VerticalAlign::BASELINE,
        })
    }
}

#[cfg(test)]
mod tests;
