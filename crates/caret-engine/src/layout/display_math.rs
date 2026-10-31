//! 将 Equation 与 Align 布局为不可拆分的显示公式块。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{DocumentLayoutError, LaidOutDisplayMath};
use crate::layout::math::{layout_visible_placeholder, micro_pt_to_points};
use caret_doc::{Align, Equation, MathNodeKind, MathTree, NodeId};
use caret_layout::{
    FontCatalog, ParagraphLayouter, ParleyMathTextShaper, ResolvedDocumentTextSettings,
    ShapingStyle,
};
use caret_math::{MathInputKind, MathLayoutBackend, MathLayoutDiagnostic, MathLayoutSnapshot};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(super) enum DisplayMathSource<'a> {
    Equation(&'a Equation),
    Align(&'a Align),
}

pub(super) fn ensure_math_backend(
    backend: &mut Option<MathLayoutBackend>,
    backend_error: &mut Option<String>,
    font_family: Option<&str>,
) {
    if backend.is_none() && backend_error.is_none() {
        match MathLayoutBackend::for_requested_font_family(font_family) {
            Ok(created) => *backend = Some(created),
            Err(error) => *backend_error = Some(error.to_string()),
        }
    }
}

impl DisplayMathSource<'_> {
    pub(super) fn id(&self) -> &NodeId {
        match self {
            Self::Equation(equation) => &equation.id,
            Self::Align(align) => &align.id,
        }
    }
}

pub(super) struct DisplayLayoutContext<'a> {
    pub backend: Option<&'a MathLayoutBackend>,
    pub backend_error: Option<&'a str>,
    pub layouter: &'a mut ParagraphLayouter,
    pub catalog: &'a mut FontCatalog,
    pub settings: &'a ResolvedDocumentTextSettings,
    pub style: &'a ShapingStyle,
    pub content_width_pt: f64,
    pub content_height_pt: f64,
}

pub(super) fn layout_display_math(
    ctx: &mut DisplayLayoutContext<'_>,
    source: DisplayMathSource<'_>,
    environment_ids: Vec<NodeId>,
) -> Result<LaidOutDisplayMath, DocumentLayoutError> {
    let id = source.id().clone();
    let root_id = match &source {
        DisplayMathSource::Equation(equation) => equation.math_tree.root.id.to_string(),
        DisplayMathSource::Align(align) => align.id.to_string(),
    };
    if matches!(source, DisplayMathSource::Align(align) if align.rows.is_empty()) {
        return visible_failure(
            ctx,
            id,
            environment_ids,
            &root_id,
            vec![MathLayoutDiagnostic::DisplayBlockFallback {
                reason: "align has no rows".to_owned(),
            }],
        );
    }

    // 正文 shaper 独占目录和布局器。失败占位必须等这次借用结束后再创建。
    let shaper = ParleyMathTextShaper::new(ctx.catalog, ctx.layouter, ctx.settings, ctx.style);
    let disposition = match source {
        DisplayMathSource::Equation(equation) if equation_is_empty(equation) => empty_or_error(
            layout_equation(ctx.backend, ctx.backend_error, equation, ctx.style, &shaper),
        ),
        DisplayMathSource::Equation(equation) => placed_or_error(
            layout_equation(ctx.backend, ctx.backend_error, equation, ctx.style, &shaper),
            ctx.content_width_pt,
            ctx.content_height_pt,
        ),
        DisplayMathSource::Align(align) => placed_or_error(
            layout_align(ctx.backend, ctx.backend_error, align, ctx.style, &shaper),
            ctx.content_width_pt,
            ctx.content_height_pt,
        ),
    };
    let placed_resources =
        matches!(disposition, LayoutDisposition::Place(_)).then(|| shaper.take_resources());
    drop(shaper);

    match disposition {
        LayoutDisposition::Place(layout) => {
            let (width_pt, ascent_pt, descent_pt) = measure_box(&layout)?;
            let Some(resources) = placed_resources else {
                return Err(DocumentLayoutError::MathLayout(
                    "display math resources were dropped".into(),
                ));
            };
            Ok(LaidOutDisplayMath {
                id,
                environment_ids: Arc::from(environment_ids),
                layout: Arc::new(layout),
                text_resources: Arc::new(resources),
                width_pt,
                ascent_pt,
                descent_pt,
            })
        }
        LayoutDisposition::Empty(mut diagnostics) => {
            diagnostics.push(MathLayoutDiagnostic::EmptyFormulaPlaceholder);
            visible_failure(ctx, id, environment_ids, &root_id, diagnostics)
        }
        LayoutDisposition::Fallback { diagnostics } => {
            visible_failure(ctx, id, environment_ids, &root_id, diagnostics)
        }
    }
}

enum LayoutDisposition {
    Place(MathLayoutSnapshot),
    Empty(Vec<MathLayoutDiagnostic>),
    Fallback {
        diagnostics: Vec<MathLayoutDiagnostic>,
    },
}

fn empty_or_error(
    result: Result<MathLayoutSnapshot, caret_math::MathLayoutError>,
) -> LayoutDisposition {
    match result {
        Ok(layout) => LayoutDisposition::Empty(layout.diagnostics),
        Err(error) => LayoutDisposition::Fallback {
            diagnostics: vec![
                MathLayoutDiagnostic::EmptyFormulaPlaceholder,
                MathLayoutDiagnostic::DisplayBlockFallback {
                    reason: error.to_string(),
                },
            ],
        },
    }
}

fn placed_or_error(
    result: Result<MathLayoutSnapshot, caret_math::MathLayoutError>,
    content_width_pt: f64,
    content_height_pt: f64,
) -> LayoutDisposition {
    match result {
        Ok(layout) => classify_layout(layout, content_width_pt, content_height_pt),
        Err(error) => LayoutDisposition::Fallback {
            diagnostics: vec![MathLayoutDiagnostic::DisplayBlockFallback {
                reason: error.to_string(),
            }],
        },
    }
}

fn layout_equation(
    backend: Option<&MathLayoutBackend>,
    backend_error: Option<&str>,
    equation: &Equation,
    style: &ShapingStyle,
    shaper: &ParleyMathTextShaper<'_>,
) -> Result<MathLayoutSnapshot, caret_math::MathLayoutError> {
    let Some(backend) = backend else {
        return Err(caret_math::MathLayoutError::Backend(
            backend_error
                .unwrap_or("math backend unavailable")
                .to_owned(),
        ));
    };
    crate::layout_math_tree(
        backend,
        &equation.math_tree,
        MathInputKind::Equation,
        style.font_size_centipoints,
        shaper,
    )
}

fn layout_align(
    backend: Option<&MathLayoutBackend>,
    backend_error: Option<&str>,
    align: &Align,
    style: &ShapingStyle,
    shaper: &ParleyMathTextShaper<'_>,
) -> Result<MathLayoutSnapshot, caret_math::MathLayoutError> {
    let Some(backend) = backend else {
        return Err(caret_math::MathLayoutError::Backend(
            backend_error
                .unwrap_or("math backend unavailable")
                .to_owned(),
        ));
    };
    crate::layout_math_align(backend, align, style.font_size_centipoints, shaper)
}

fn classify_layout(
    layout: MathLayoutSnapshot,
    content_width_pt: f64,
    content_height_pt: f64,
) -> LayoutDisposition {
    let Ok((width_pt, ascent_pt, descent_pt)) = measure_box(&layout) else {
        return LayoutDisposition::Fallback {
            diagnostics: with_fallback(
                layout.diagnostics,
                "display math geometry is not representable",
            ),
        };
    };
    let height_pt = ascent_pt + descent_pt;
    if !width_pt.is_finite() || !height_pt.is_finite() || width_pt <= 0.0 || height_pt <= 0.0 {
        return LayoutDisposition::Fallback {
            diagnostics: with_fallback(
                layout.diagnostics,
                "display math has no placeable geometry",
            ),
        };
    }
    if let Some(reason) = overflow_reason(width_pt, height_pt, content_width_pt, content_height_pt)
    {
        return LayoutDisposition::Fallback {
            diagnostics: with_fallback(layout.diagnostics, reason),
        };
    }
    LayoutDisposition::Place(layout)
}

fn with_fallback(
    mut diagnostics: Vec<MathLayoutDiagnostic>,
    reason: &str,
) -> Vec<MathLayoutDiagnostic> {
    diagnostics.push(MathLayoutDiagnostic::DisplayBlockFallback {
        reason: reason.to_owned(),
    });
    diagnostics
}

fn overflow_reason(
    width_pt: f32,
    height_pt: f32,
    content_width_pt: f64,
    content_height_pt: f64,
) -> Option<&'static str> {
    let too_wide = f64::from(width_pt) > content_width_pt;
    let too_tall = f64::from(height_pt) > content_height_pt;
    match (too_wide, too_tall) {
        (true, true) => Some("display math is wider and taller than the content box"),
        (true, false) => Some("display math is wider than the content box"),
        (false, true) => Some("display math is taller than the content box"),
        (false, false) => None,
    }
}

fn equation_is_empty(equation: &Equation) -> bool {
    tree_is_empty(&equation.math_tree)
}

fn tree_is_empty(tree: &MathTree) -> bool {
    match tree.root.kind {
        MathNodeKind::Placeholder | MathNodeKind::Row => tree.root.children.is_empty(),
        _ => false,
    }
}

fn measure_box(layout: &MathLayoutSnapshot) -> Result<(f32, f32, f32), DocumentLayoutError> {
    Ok((
        micro_pt_to_points(layout.width_micro_pt)?,
        micro_pt_to_points(layout.ascent_micro_pt)?,
        micro_pt_to_points(layout.descent_micro_pt)?,
    ))
}

fn visible_failure(
    ctx: &mut DisplayLayoutContext<'_>,
    id: NodeId,
    environment_ids: Vec<NodeId>,
    root_id: &str,
    diagnostics: Vec<MathLayoutDiagnostic>,
) -> Result<LaidOutDisplayMath, DocumentLayoutError> {
    let (layout, resources) = layout_visible_placeholder(
        ctx.catalog,
        ctx.layouter,
        ctx.settings,
        ctx.style,
        root_id,
        diagnostics,
    )?;
    let (width_pt, ascent_pt, descent_pt) = measure_box(&layout)?;
    Ok(LaidOutDisplayMath {
        id,
        environment_ids: Arc::from(environment_ids),
        layout: Arc::new(layout),
        text_resources: Arc::new(resources),
        width_pt,
        ascent_pt,
        descent_pt,
    })
}
