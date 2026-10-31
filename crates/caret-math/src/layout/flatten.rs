//! 将 latex-rust 数学盒展平为稳定的公式绘制原语和节点几何。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{
    MICRO_POINTS_PER_POINT, MathLayoutError, MathLayoutItem, MathLayoutSnapshot, MathNodeGeometry,
    transparent_child_provenance,
};
use crate::{MathSyntaxKind, MathSyntaxNode, MathSyntaxTree};
use latex_rust::{BoxContent, Dim, Error, MathBox};
use std::collections::{HashMap, HashSet};

pub(super) fn append_provenance_aliases(tree: &MathSyntaxTree, snapshot: &mut MathLayoutSnapshot) {
    let mut alias_children = HashMap::<String, Vec<String>>::new();
    collect_provenance_aliases(&tree.root, &mut alias_children);
    let mut visited = HashSet::with_capacity(snapshot.nodes.len());
    let mut pending = Vec::with_capacity(snapshot.nodes.len());
    for (index, geometry) in snapshot.nodes.iter().enumerate() {
        if visited.insert(geometry.node_id.clone()) {
            pending.push((geometry.node_id.clone(), index));
        }
    }
    while let Some((source_id, index)) = pending.pop() {
        if let Some(alias_ids) = alias_children.get(&source_id) {
            for alias_id in alias_ids {
                if !visited.insert(alias_id.clone()) {
                    continue;
                }
                let geometry = snapshot.nodes[index].clone();
                snapshot.nodes.push(MathNodeGeometry {
                    node_id: alias_id.clone(),
                    ..geometry
                });
                let alias_index = snapshot.nodes.len() - 1;
                pending.push((alias_id.clone(), alias_index));
            }
        }
    }
}

fn collect_provenance_aliases(node: &MathSyntaxNode, aliases: &mut HashMap<String, Vec<String>>) {
    if let Some(source_id) = &node.provenance {
        aliases
            .entry(source_id.clone())
            .or_default()
            .extend(node.provenance_aliases.iter().cloned());
    }
    if node.kind == MathSyntaxKind::Group
        && node.children.len() == 1
        && let (Some(group_id), Some(source_id)) = (
            node.provenance.as_ref(),
            transparent_child_provenance(&node.children[0]),
        )
    {
        aliases
            .entry(source_id.to_owned())
            .or_default()
            .push(group_id.clone());
    }
    for child in &node.children {
        collect_provenance_aliases(child, aliases);
    }
}

pub(super) fn to_micro_pt(value: &Dim, root_em_micro_pt: i64) -> Result<i64, MathLayoutError> {
    let physical_pt = value * &Dim::ratio(root_em_micro_pt, MICRO_POINTS_PER_POINT);
    physical_pt
        .checked_scaled_i64(MICRO_POINTS_PER_POINT)
        .ok_or(MathLayoutError::GeometryOverflow)
}

pub(super) fn flatten_box(
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
