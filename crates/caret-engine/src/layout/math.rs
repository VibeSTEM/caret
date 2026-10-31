//! 为段落中的 InlineMath 生成公式快照、正文资源和分页占位盒。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::LaidOutMathObject;
use crate::DocumentLayoutError;
use caret_layout::{
    FontCatalog, InlineMathAnchor, MathInlineBoxMetrics, ParagraphLayouter, ParleyMathTextShaper,
    ResolvedDocumentTextSettings,
};
use caret_math::{
    MathInputKind, MathLayoutBackend, MathLayoutDiagnostic, MathLayoutItem, MathLayoutSnapshot,
    MathNodeGeometry, MathTextShaper, MathTextStyle,
};
use std::sync::Arc;

pub(super) fn layout_inline_math_objects(
    backend: Option<&MathLayoutBackend>,
    backend_error: Option<&str>,
    layouter: &mut ParagraphLayouter,
    catalog: &mut FontCatalog,
    settings: &ResolvedDocumentTextSettings,
    formulas: &[InlineMathAnchor<'_>],
) -> Result<
    (
        Vec<LaidOutMathObject>,
        Vec<caret_layout::InlineObjectPlacement>,
    ),
    DocumentLayoutError,
> {
    let mut objects = Vec::with_capacity(formulas.len());
    let mut placements = Vec::with_capacity(formulas.len());
    for (index, anchor) in formulas.iter().enumerate() {
        let math = anchor.math;
        let inline_box_id = u64::try_from(index)
            .map_err(|error| DocumentLayoutError::MathLayout(error.to_string()))?;
        let (layout, text_resources) = {
            let shaper =
                ParleyMathTextShaper::new(catalog, layouter, settings, &anchor.shaping_style);
            let layout_result = backend.map_or_else(
                || {
                    Err(caret_math::MathLayoutError::Backend(
                        backend_error
                            .unwrap_or("math backend unavailable")
                            .to_owned(),
                    ))
                },
                |backend| {
                    crate::layout_math_tree(
                        backend,
                        &math.math_tree,
                        MathInputKind::Inline,
                        anchor.shaping_style.font_size_centipoints,
                        &shaper,
                    )
                },
            );
            match layout_result {
                Ok(layout) if is_empty_formula(math) => {
                    drop(shaper);
                    let (layout, resources) = layout_source_fallback(
                        catalog,
                        layouter,
                        settings,
                        &anchor.shaping_style,
                        InlineFallback {
                            root_id: &math.math_tree.root.id.to_string(),
                            error: None,
                            empty_formula: true,
                            diagnostics: layout.diagnostics,
                        },
                    )?;
                    (Arc::new(layout), Arc::new(resources))
                }
                Ok(layout)
                    if layout.width_micro_pt < 0
                        || layout.ascent_micro_pt < 0
                        || layout.descent_micro_pt < 0 =>
                {
                    let diagnostics = layout.diagnostics.clone();
                    drop(shaper);
                    let (layout, resources) = layout_source_fallback(
                        catalog,
                        layouter,
                        settings,
                        &anchor.shaping_style,
                        InlineFallback {
                            root_id: &math.math_tree.root.id.to_string(),
                            error: Some(
                                "math geometry cannot be represented by an inline box".into(),
                            ),
                            empty_formula: false,
                            diagnostics,
                        },
                    )?;
                    (Arc::new(layout), Arc::new(resources))
                }
                Ok(layout) => (Arc::new(layout), Arc::new(shaper.take_resources())),
                Err(error) => {
                    drop(shaper);
                    let (layout, resources) = layout_source_fallback(
                        catalog,
                        layouter,
                        settings,
                        &anchor.shaping_style,
                        InlineFallback {
                            root_id: &math.math_tree.root.id.to_string(),
                            error: Some(error.to_string()),
                            empty_formula: is_empty_formula(math),
                            diagnostics: Vec::new(),
                        },
                    )?;
                    (Arc::new(layout), Arc::new(resources))
                }
            }
        };
        let (layout, text_resources) = if inline_geometry(&layout).is_ok() {
            (layout, text_resources)
        } else {
            let diagnostics = layout.diagnostics.clone();
            let (fallback, resources) = layout_source_fallback(
                catalog,
                layouter,
                settings,
                &anchor.shaping_style,
                InlineFallback {
                    root_id: &math.math_tree.root.id.to_string(),
                    error: Some("math geometry exceeds Parley's inline box range".into()),
                    empty_formula: false,
                    diagnostics,
                },
            )?;
            (Arc::new(fallback), Arc::new(resources))
        };
        let source_bytes = anchor.source_bytes.clone();
        let metrics = inline_geometry(&layout)?;
        placements.push(
            metrics
                .inline_box(inline_box_id, anchor.source_bytes.start)
                .map_err(DocumentLayoutError::MathLayout)?,
        );
        objects.push(LaidOutMathObject {
            inline_box_id,
            id: math.id.clone(),
            source_bytes,
            layout,
            text_resources,
        });
    }
    Ok((objects, placements))
}

fn inline_geometry(
    layout: &MathLayoutSnapshot,
) -> Result<MathInlineBoxMetrics, DocumentLayoutError> {
    Ok(MathInlineBoxMetrics {
        width_pt: micro_pt_to_points(layout.width_micro_pt)?,
        ascent_pt: micro_pt_to_points(layout.ascent_micro_pt)?,
        descent_pt: micro_pt_to_points(layout.descent_micro_pt)?,
    })
}

fn is_empty_formula(math: &caret_doc::InlineMath) -> bool {
    use caret_doc::MathNodeKind;

    match math.math_tree.root.kind {
        MathNodeKind::Placeholder => math.math_tree.root.children.is_empty(),
        MathNodeKind::Row => math.math_tree.root.children.is_empty(),
        _ => false,
    }
}

struct InlineFallback<'a> {
    root_id: &'a str,
    error: Option<String>,
    empty_formula: bool,
    diagnostics: Vec<MathLayoutDiagnostic>,
}

fn layout_source_fallback(
    catalog: &mut FontCatalog,
    layouter: &mut ParagraphLayouter,
    settings: &ResolvedDocumentTextSettings,
    style: &caret_layout::ShapingStyle,
    fallback: InlineFallback<'_>,
) -> Result<
    (
        MathLayoutSnapshot,
        std::collections::HashMap<u64, Arc<caret_layout::ParleyParagraphLayout>>,
    ),
    DocumentLayoutError,
> {
    let display_text = "□";
    let shaper = ParleyMathTextShaper::new(catalog, layouter, settings, style);
    let font_size_micro_pt = i64::from(style.font_size_centipoints) * 10_000;
    shaper
        .begin_layout()
        .map_err(DocumentLayoutError::MathLayout)?;
    let metrics = match shaper.shape_text(display_text, MathTextStyle::Text, font_size_micro_pt) {
        Ok(metrics) => metrics,
        Err(error) => {
            shaper.finish_layout(false);
            return Err(DocumentLayoutError::MathLayout(error));
        }
    };
    shaper.finish_layout(true);
    let resources = shaper.take_resources();
    let nodes = vec![MathNodeGeometry {
        node_id: fallback.root_id.to_owned(),
        x_micro_pt: 0,
        y_micro_pt: -metrics.ascent_micro_pt,
        width_micro_pt: metrics.width_micro_pt,
        ascent_micro_pt: metrics.ascent_micro_pt,
        descent_micro_pt: metrics.descent_micro_pt,
    }];
    let mut diagnostics = fallback.diagnostics;
    if fallback.empty_formula {
        diagnostics.push(MathLayoutDiagnostic::EmptyFormulaPlaceholder);
    }
    if let Some(reason) = fallback.error {
        diagnostics.push(MathLayoutDiagnostic::InlinePlaceholderFallback { reason });
    }
    Ok((
        MathLayoutSnapshot {
            font_resource_sha256: caret_math::MATH_FONT_SHA256,
            width_micro_pt: metrics.width_micro_pt,
            ascent_micro_pt: metrics.ascent_micro_pt,
            descent_micro_pt: metrics.descent_micro_pt,
            items: vec![MathLayoutItem::ExternalText {
                handle: metrics.handle,
                x_micro_pt: 0,
                baseline_y_micro_pt: 0,
                color: None,
            }],
            nodes,
            diagnostics,
        },
        resources,
    ))
}

fn micro_pt_to_points(value: i64) -> Result<f32, DocumentLayoutError> {
    if value < 0 {
        return Err(DocumentLayoutError::MathLayout(
            "negative inline math geometry".into(),
        ));
    }
    #[expect(clippy::cast_precision_loss, reason = "转换为 Parley 的 f32 pt 度量")]
    let points = value as f64 / 1_000_000.0;
    if points > f64::from(f32::MAX) {
        return Err(DocumentLayoutError::MathLayout(
            "inline math geometry exceeds Parley's range".into(),
        ));
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Parley 的 inline box 几何使用 f32 pt"
    )]
    let points = points as f32;
    if points.is_finite() {
        Ok(points)
    } else {
        Err(DocumentLayoutError::MathLayout(
            "inline math geometry exceeds Parley's range".into(),
        ))
    }
}
