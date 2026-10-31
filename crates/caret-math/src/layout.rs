//! 将 Caret 无关的数学语法树布局成稳定的 glyph、rule 和节点几何快照。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::{MathInputKind, MathSyntaxTree};
use convert::{register_provenance, to_latex_node};
use latex_rust::{
    BoxContent, Dim, Error, MathBox, MathFont, MathNode, MathStyle, TextShaper,
    layout_with_caller_context,
};
mod convert;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt::{Display, Formatter};

const MICRO_POINTS_PER_POINT: i64 = 1_000_000;
const CENTIPOINTS_PER_POINT: i64 = 100;

/// A page-independent primitive emitted by one TeX MathBox snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathLayoutItem {
    /// STIX Two Math glyph, positioned in integer micro-points.
    Glyph {
        glyph_id: u16,
        font_size_micro_pt: i64,
        x_micro_pt: i64,
        baseline_y_micro_pt: i64,
        color: Option<[u8; 3]>,
    },
    /// Caller-shaped text run; its provider retains the font/glyph resource.
    ExternalText {
        handle: u64,
        x_micro_pt: i64,
        baseline_y_micro_pt: i64,
        color: Option<[u8; 3]>,
    },
    /// Filled TeX rule, including fraction bars and array rules.
    Rule {
        x_micro_pt: i64,
        y_micro_pt: i64,
        width_micro_pt: i64,
        height_micro_pt: i64,
        color: Option<[u8; 3]>,
    },
    /// TeX line primitive used by cancellation and decoration constructs.
    Line {
        x1_micro_pt: i64,
        y1_micro_pt: i64,
        x2_micro_pt: i64,
        y2_micro_pt: i64,
        thickness_micro_pt: i64,
        color: Option<[u8; 3]>,
    },
    /// Background rectangle or stroked frame.
    Rectangle {
        x_micro_pt: i64,
        y_micro_pt: i64,
        width_micro_pt: i64,
        height_micro_pt: i64,
        fill: Option<[u8; 3]>,
        stroke: Option<(Option<[u8; 3]>, i64)>,
    },
}

/// Geometry of one stable Caret math-node ID in a layout generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathNodeGeometry {
    pub node_id: String,
    pub x_micro_pt: i64,
    pub y_micro_pt: i64,
    pub width_micro_pt: i64,
    pub ascent_micro_pt: i64,
    pub descent_micro_pt: i64,
}

/// Complete formula geometry shared by drawing, hit testing and caret projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathLayoutSnapshot {
    /// Content identity of the pinned font face used for every math glyph.
    pub font_resource_sha256: &'static str,
    pub width_micro_pt: i64,
    pub ascent_micro_pt: i64,
    pub descent_micro_pt: i64,
    pub items: Vec<MathLayoutItem>,
    pub nodes: Vec<MathNodeGeometry>,
    pub diagnostics: Vec<MathLayoutDiagnostic>,
}

/// Observable fallback from an unsupported document font preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathLayoutDiagnostic {
    UnsupportedFontFamily {
        requested_family: String,
        used_family: &'static str,
    },
}

/// Errors raised before a partial or invalid math snapshot can escape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathLayoutError {
    InvalidFontSize,
    UnsupportedTree(String),
    Backend(String),
    GeometryOverflow,
}

impl Display for MathLayoutError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFontSize => formatter.write_str("math.layout.invalid_font_size"),
            Self::UnsupportedTree(reason) => {
                write!(formatter, "math.layout.unsupported_tree: {reason}")
            }
            Self::Backend(reason) => write!(formatter, "math.layout.backend: {reason}"),
            Self::GeometryOverflow => formatter.write_str("math.layout.geometry_overflow"),
        }
    }
}

impl std::error::Error for MathLayoutError {}

struct TextLayoutTransaction<'a> {
    shaper: &'a dyn TextShaper,
    committed: bool,
}

impl TextLayoutTransaction<'_> {
    fn commit(mut self) {
        self.committed = true;
        self.shaper.finish_layout(true);
    }
}

impl Drop for TextLayoutTransaction<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.shaper.finish_layout(false);
        }
    }
}

/// Reusable STIX Two Math layout backend. It keeps the third-party font and AST private.
pub struct MathLayoutBackend {
    font: MathFont,
    font_diagnostic: Option<MathLayoutDiagnostic>,
}

impl MathLayoutBackend {
    /// Loads the pinned embedded STIX Two Math face once for a layout service.
    pub fn new() -> Result<Self, MathLayoutError> {
        Self::for_requested_font_family(None)
    }

    /// Loads STIX Two Math and records an observable fallback for unsupported preferences.
    pub fn for_requested_font_family(
        requested_family: Option<&str>,
    ) -> Result<Self, MathLayoutError> {
        let font_diagnostic = requested_family
            .filter(|family| !family.eq_ignore_ascii_case(latex_rust::STIX_TWO_MATH_NAME))
            .map(|family| MathLayoutDiagnostic::UnsupportedFontFamily {
                requested_family: family.to_owned(),
                used_family: latex_rust::STIX_TWO_MATH_NAME,
            });
        MathFont::stix_two_math()
            .map(|font| Self {
                font,
                font_diagnostic,
            })
            .map_err(|error| MathLayoutError::Backend(error.to_string()))
    }

    /// Builds a complete snapshot; provider failure or geometry overflow returns no partial value.
    pub fn layout(
        &self,
        tree: &MathSyntaxTree,
        font_size_centipoints: u16,
        text_shaper: &dyn TextShaper,
    ) -> Result<MathLayoutSnapshot, MathLayoutError> {
        if font_size_centipoints == 0 {
            return Err(MathLayoutError::InvalidFontSize);
        }
        validate_syntax_budget(tree)?;
        text_shaper
            .begin_layout()
            .map_err(MathLayoutError::Backend)?;
        let transaction = TextLayoutTransaction {
            shaper: text_shaper,
            committed: false,
        };
        let root_em_micro_pt = i64::from(font_size_centipoints)
            .checked_mul(MICRO_POINTS_PER_POINT)
            .and_then(|value| value.checked_div(CENTIPOINTS_PER_POINT))
            .ok_or(MathLayoutError::GeometryOverflow)?;
        let em_size_pt = Dim::ratio(root_em_micro_pt, MICRO_POINTS_PER_POINT);
        if em_size_pt.is_nan() {
            return Err(MathLayoutError::InvalidFontSize);
        }
        let mut provenance = HashMap::new();
        let mut ast = to_latex_node(&tree.root, &mut provenance)?;
        register_provenance(&ast, &tree.root, &mut provenance);
        let node_provenance =
            |node: &MathNode| provenance.get(&(node as *const MathNode as usize)).cloned();
        let math_style = match tree.input_kind {
            MathInputKind::Inline => MathStyle::Text,
            MathInputKind::Equation | MathInputKind::Align => MathStyle::Display,
        };
        let boxed = layout_with_caller_context(
            &ast,
            &self.font,
            math_style,
            crate::MAX_MATH_NESTING_DEPTH,
            &em_size_pt,
            text_shaper,
            &node_provenance,
        )
        .map_err(|error| MathLayoutError::Backend(error.to_string()))?;
        let mut snapshot = MathLayoutSnapshot {
            font_resource_sha256: latex_rust::STIX_TWO_MATH_SHA256,
            width_micro_pt: to_micro_pt(&boxed.width, root_em_micro_pt)?,
            ascent_micro_pt: to_micro_pt(&boxed.height, root_em_micro_pt)?,
            descent_micro_pt: to_micro_pt(&boxed.depth, root_em_micro_pt)?,
            items: Vec::new(),
            nodes: Vec::new(),
            diagnostics: self.font_diagnostic.iter().cloned().collect(),
        };
        flatten_box(&boxed, 0, 0, None, root_em_micro_pt, true, &mut snapshot)?;
        // Drop the possibly deep upstream tree iteratively through latex-rust's guarded Drop.
        ast = MathNode::Row(Vec::new());
        drop(ast);
        transaction.commit();
        Ok(snapshot)
    }
}

fn validate_syntax_budget(tree: &MathSyntaxTree) -> Result<(), MathLayoutError> {
    let mut pending = vec![(&tree.root, 1usize)];
    let mut count = 0usize;
    while let Some((node, depth)) = pending.pop() {
        count = count
            .checked_add(1)
            .ok_or(MathLayoutError::GeometryOverflow)?;
        if depth > crate::MAX_MATH_NESTING_DEPTH {
            return Err(MathLayoutError::UnsupportedTree(
                "syntax tree exceeds the configured nesting depth".into(),
            ));
        }
        if count > crate::MAX_MATH_SYNTAX_NODES {
            return Err(MathLayoutError::UnsupportedTree(
                "syntax tree exceeds the configured node budget".into(),
            ));
        }
        if node.attributes.contains_key("atomClass") {
            let expanded_atoms = node
                .text
                .as_deref()
                .map(|text| text.chars().count())
                .unwrap_or(0);
            count = count
                .checked_add(expanded_atoms.saturating_sub(1))
                .ok_or(MathLayoutError::GeometryOverflow)?;
            if count > crate::MAX_MATH_SYNTAX_NODES {
                return Err(MathLayoutError::UnsupportedTree(
                    "expanded syntax tree exceeds the configured node budget".into(),
                ));
            }
        }
        let child_depth = depth
            .checked_add(1)
            .ok_or(MathLayoutError::GeometryOverflow)?;
        pending.extend(node.children.iter().map(|child| (child, child_depth)));
    }
    Ok(())
}

fn to_micro_pt(value: &Dim, root_em_micro_pt: i64) -> Result<i64, MathLayoutError> {
    let physical_pt = value * &Dim::ratio(root_em_micro_pt, MICRO_POINTS_PER_POINT);
    physical_pt
        .checked_scaled_i64(MICRO_POINTS_PER_POINT)
        .ok_or(MathLayoutError::GeometryOverflow)
}

fn flatten_box(
    bx: &MathBox,
    origin_x_micro_pt: i64,
    parent_baseline_y_micro_pt: i64,
    color: Option<[u8; 3]>,
    root_em_micro_pt: i64,
    visible: bool,
    snapshot: &mut MathLayoutSnapshot,
) -> Result<(), MathLayoutError> {
    let shift = to_micro_pt(&bx.shift, root_em_micro_pt)?;
    let baseline_y = parent_baseline_y_micro_pt
        .checked_sub(shift)
        .ok_or(MathLayoutError::GeometryOverflow)?;
    match &bx.content {
        BoxContent::Empty | BoxContent::Kern(_) => {}
        BoxContent::ExternalText(handle) if visible => {
            snapshot.items.push(MathLayoutItem::ExternalText {
                handle: *handle,
                x_micro_pt: origin_x_micro_pt,
                baseline_y_micro_pt: baseline_y,
                color,
            });
        }
        BoxContent::ExternalText(_) => {}
        BoxContent::Phantom(inner) => flatten_box(
            inner,
            origin_x_micro_pt,
            baseline_y,
            color,
            root_em_micro_pt,
            false,
            snapshot,
        )?,
        BoxContent::Provenance(node_id, inner) => {
            snapshot.nodes.push(MathNodeGeometry {
                node_id: node_id.clone(),
                x_micro_pt: origin_x_micro_pt,
                y_micro_pt: baseline_y
                    .checked_sub(to_micro_pt(&bx.height, root_em_micro_pt)?)
                    .ok_or(MathLayoutError::GeometryOverflow)?,
                width_micro_pt: to_micro_pt(&bx.width, root_em_micro_pt)?,
                ascent_micro_pt: to_micro_pt(&bx.height, root_em_micro_pt)?,
                descent_micro_pt: to_micro_pt(&bx.depth, root_em_micro_pt)?,
            });
            flatten_box(
                inner,
                origin_x_micro_pt,
                baseline_y,
                color,
                root_em_micro_pt,
                visible,
                snapshot,
            )?;
        }
        BoxContent::Glyph {
            glyph_id, scale, ..
        } => {
            let font_size_micro_pt = to_micro_pt(scale, root_em_micro_pt)?;
            if font_size_micro_pt <= 0 {
                return Err(MathLayoutError::GeometryOverflow);
            }
            if visible {
                snapshot.items.push(MathLayoutItem::Glyph {
                    glyph_id: *glyph_id,
                    font_size_micro_pt,
                    x_micro_pt: origin_x_micro_pt,
                    baseline_y_micro_pt: baseline_y,
                    color,
                });
            }
        }
        BoxContent::Rule => {
            let height = to_micro_pt(&(&bx.height + &bx.depth), root_em_micro_pt)?;
            let width = to_micro_pt(&bx.width, root_em_micro_pt)?;
            if visible && width > 0 && height > 0 {
                snapshot.items.push(MathLayoutItem::Rule {
                    x_micro_pt: origin_x_micro_pt,
                    y_micro_pt: baseline_y
                        .checked_sub(to_micro_pt(&bx.height, root_em_micro_pt)?)
                        .ok_or(MathLayoutError::GeometryOverflow)?,
                    width_micro_pt: width,
                    height_micro_pt: height,
                    color,
                });
            }
        }
        BoxContent::Line {
            x1,
            y1,
            x2,
            y2,
            thickness,
        } if visible => snapshot.items.push(MathLayoutItem::Line {
            x1_micro_pt: origin_x_micro_pt
                .checked_add(to_micro_pt(x1, root_em_micro_pt)?)
                .ok_or(MathLayoutError::GeometryOverflow)?,
            y1_micro_pt: baseline_y
                .checked_sub(to_micro_pt(y1, root_em_micro_pt)?)
                .ok_or(MathLayoutError::GeometryOverflow)?,
            x2_micro_pt: origin_x_micro_pt
                .checked_add(to_micro_pt(x2, root_em_micro_pt)?)
                .ok_or(MathLayoutError::GeometryOverflow)?,
            y2_micro_pt: baseline_y
                .checked_sub(to_micro_pt(y2, root_em_micro_pt)?)
                .ok_or(MathLayoutError::GeometryOverflow)?,
            thickness_micro_pt: to_micro_pt(thickness, root_em_micro_pt)?,
            color,
        }),
        BoxContent::Line { .. } => {}
        BoxContent::HList(children) => {
            let mut x = origin_x_micro_pt;
            for child in children {
                flatten_box(
                    child,
                    x,
                    baseline_y,
                    color,
                    root_em_micro_pt,
                    visible,
                    snapshot,
                )?;
                x = x
                    .checked_add(to_micro_pt(&child.width, root_em_micro_pt)?)
                    .ok_or(MathLayoutError::GeometryOverflow)?;
            }
        }
        BoxContent::VList(children) => {
            if let Some(first) = children.first() {
                flatten_box(
                    first,
                    origin_x_micro_pt,
                    baseline_y,
                    color,
                    root_em_micro_pt,
                    visible,
                    snapshot,
                )?;
                let mut below = to_micro_pt(&first.depth, root_em_micro_pt)?;
                for child in children.iter().skip(1) {
                    let child_baseline = baseline_y
                        .checked_add(below)
                        .and_then(|value| {
                            value.checked_add(to_micro_pt(&child.height, root_em_micro_pt).ok()?)
                        })
                        .ok_or(MathLayoutError::GeometryOverflow)?;
                    flatten_box(
                        child,
                        origin_x_micro_pt,
                        child_baseline,
                        color,
                        root_em_micro_pt,
                        visible,
                        snapshot,
                    )?;
                    below = below
                        .checked_add(to_micro_pt(
                            &(&child.height + &child.depth),
                            root_em_micro_pt,
                        )?)
                        .ok_or(MathLayoutError::GeometryOverflow)?;
                }
            }
        }
        BoxContent::Overlap(children) => {
            for child in children {
                flatten_box(
                    child,
                    origin_x_micro_pt,
                    baseline_y,
                    color,
                    root_em_micro_pt,
                    visible,
                    snapshot,
                )?;
            }
        }
        BoxContent::Color(inner_color, inner) => {
            flatten_box(
                inner,
                origin_x_micro_pt,
                baseline_y,
                Some([inner_color.r, inner_color.g, inner_color.b]),
                root_em_micro_pt,
                visible,
                snapshot,
            )?;
        }
        BoxContent::BackColor(background, inner) => {
            if visible {
                snapshot.items.push(MathLayoutItem::Rectangle {
                    x_micro_pt: origin_x_micro_pt,
                    y_micro_pt: baseline_y
                        .checked_sub(to_micro_pt(&bx.height, root_em_micro_pt)?)
                        .ok_or(MathLayoutError::GeometryOverflow)?,
                    width_micro_pt: to_micro_pt(&bx.width, root_em_micro_pt)?,
                    height_micro_pt: to_micro_pt(&(&bx.height + &bx.depth), root_em_micro_pt)?,
                    fill: Some([background.r, background.g, background.b]),
                    stroke: None,
                });
            }
            flatten_box(
                inner,
                origin_x_micro_pt,
                baseline_y,
                color,
                root_em_micro_pt,
                visible,
                snapshot,
            )?;
        }
        BoxContent::Frame {
            thickness,
            stroke,
            inner,
        } => {
            let stroke_color = stroke.map(|color| [color.r, color.g, color.b]).or(color);
            if visible {
                snapshot.items.push(MathLayoutItem::Rectangle {
                    x_micro_pt: origin_x_micro_pt,
                    y_micro_pt: baseline_y
                        .checked_sub(to_micro_pt(&bx.height, root_em_micro_pt)?)
                        .ok_or(MathLayoutError::GeometryOverflow)?,
                    width_micro_pt: to_micro_pt(&bx.width, root_em_micro_pt)?,
                    height_micro_pt: to_micro_pt(&(&bx.height + &bx.depth), root_em_micro_pt)?,
                    fill: None,
                    stroke: Some((stroke_color, to_micro_pt(thickness, root_em_micro_pt)?)),
                });
            }
            flatten_box(
                inner,
                origin_x_micro_pt,
                baseline_y,
                color,
                root_em_micro_pt,
                visible,
                snapshot,
            )?;
        }
    }
    Ok(())
}

impl From<Error> for MathLayoutError {
    fn from(value: Error) -> Self {
        Self::Backend(value.to_string())
    }
}

/// Re-export the backend-neutral text shaping contract used by `caret-layout`.
pub use latex_rust::{
    ExternalTextMetrics as MathTextMetrics, TextShaper as MathTextShaper,
    TextStyle as MathTextStyle,
};
