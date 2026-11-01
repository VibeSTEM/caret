//! 公式编辑时的插入点几何：把公式源码光标映射到已排版公式节点盒上。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{DocumentLayoutSnapshot, PageCaretGeometry};
use crate::editing::{MathEdit, MathEditTarget};
use caret_doc::{Block, Document, InlineNode, MathNode, MathNodeKind, MathTree, NodeId};
use caret_layout::VisualAffinity;
use caret_math::layout::MathLayout;
use std::ops::Range;

/// 源码光标落在的节点边缘。
enum Edge {
    Left,
    Right,
    Fraction(f64),
    Center,
}

impl DocumentLayoutSnapshot {
    /// 公式编辑中的插入点：竖线高度跟随所在节点（上标、分子等）而非整行。
    pub fn math_caret_geometry(
        &self,
        document: &Document,
        edit: &MathEdit,
    ) -> Option<PageCaretGeometry> {
        let (paragraph_id, math_id) = match &edit.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => (paragraph_id.clone(), math_id.clone()),
            MathEditTarget::Block { block_id, .. } => (block_id.clone(), block_id.clone()),
        };
        let paragraph = self.paragraph_by_id(&paragraph_id)?;
        let math_box = paragraph
            .math_boxes
            .iter()
            .find(|math| math.id == math_id)?;
        let object_position = crate::Pos {
            story: paragraph.story.clone(),
            paragraph_id: paragraph.id.clone(),
            offset: math_box.source_bytes.start,
            affinity: crate::Affinity::Downstream,
        };
        let object_caret = self.caret_geometry(&object_position, VisualAffinity::Downstream)?;
        let cursor = paragraph
            .layout
            .cursor_at(math_box.source_bytes.start, VisualAffinity::Downstream)?;
        let (_, line_index) = paragraph.layout.cursor_geometry_with_line(cursor, 0.0)?;
        let line = paragraph.layout.paragraph.lines.get(line_index)?;
        let location = paragraph.line_locations.get(line_index)?.as_ref()?;
        let baseline = location.y0_pt + f64::from(line.metrics.baseline_pt);
        let (tree_root, cursor) = math_tree_for_cursor(document, edit)?;
        let (node_id, edge) = node_at_cursor(&tree_root, cursor);
        let layout = &math_box.layout;
        let (x, y0, y1) = node_edge(layout, node_id.as_ref(), edge)?;
        Some(PageCaretGeometry {
            page_index: object_caret.page_index,
            x_pt: object_caret.x_pt + x,
            y0_pt: baseline + y0,
            y1_pt: baseline + y1,
        })
    }
}

/// 光标所在的数学树根及树内源码偏移；对齐公式定位到具体单元格。
fn math_tree_for_cursor(document: &Document, edit: &MathEdit) -> Option<(MathNode, usize)> {
    match &edit.target {
        MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } => {
            let tree = find_inline_tree(document, paragraph_id, math_id)?;
            Some((tree.root.clone(), edit.cursor))
        }
        MathEditTarget::Block { block_id, align } => {
            let block = find_block(&document.body, block_id)?;
            match (block, align) {
                (Block::Equation(equation), false) => {
                    Some((equation.math_tree.root.clone(), edit.cursor))
                }
                (Block::Align(value), true) => {
                    let (row, cell, offset) = edit.align_cell_at_cursor()?;
                    let tree = value.rows.get(row)?.cells.get(cell)?;
                    Some((tree.root.clone(), offset))
                }
                _ => None,
            }
        }
    }
}

fn find_inline_tree<'a>(
    document: &'a Document,
    paragraph_id: &NodeId,
    math_id: &NodeId,
) -> Option<&'a MathTree> {
    fn in_inlines<'a>(nodes: &'a [InlineNode], id: &NodeId) -> Option<&'a MathTree> {
        nodes.iter().find_map(|node| match node {
            InlineNode::Math(math) if &math.id == id => Some(&math.math_tree),
            InlineNode::Environment(environment) => in_inlines(&environment.children, id),
            _ => None,
        })
    }
    fn in_blocks<'a>(
        blocks: &'a [Block],
        paragraph_id: &NodeId,
        math_id: &NodeId,
    ) -> Option<&'a MathTree> {
        blocks.iter().find_map(|block| match block {
            Block::Paragraph(paragraph) if &paragraph.id == paragraph_id => {
                in_inlines(&paragraph.inlines, math_id)
            }
            Block::Environment(environment) => environment
                .title
                .as_ref()
                .filter(|title| &title.id == paragraph_id)
                .and_then(|title| in_inlines(&title.inlines, math_id))
                .or_else(|| in_blocks(&environment.children, paragraph_id, math_id)),
            _ => None,
        })
    }
    in_blocks(&document.body, paragraph_id, math_id)
}

fn find_block<'a>(blocks: &'a [Block], id: &NodeId) -> Option<&'a Block> {
    blocks.iter().find_map(|block| match block {
        Block::Equation(value) if &value.id == id => Some(block),
        Block::Align(value) if &value.id == id => Some(block),
        Block::Environment(environment) => find_block(&environment.children, id),
        _ => None,
    })
}

/// 按源码范围选择节点：空槽内取占位盒中央，否则取光标两侧最近叶节点的边缘。
fn node_at_cursor(root: &MathNode, cursor: usize) -> (Option<NodeId>, Edge) {
    let mut nodes = Vec::new();
    collect(root, &mut nodes);
    let placeholder = nodes.iter().rev().find(|(node, range)| {
        node.kind == MathNodeKind::Placeholder && range.start < cursor && cursor < range.end
    });
    if let Some((node, _)) = placeholder {
        return (Some(node.id.clone()), Edge::Center);
    }
    let leaves = nodes
        .iter()
        .filter(|(node, _)| node.children.is_empty() && node.kind != MathNodeKind::Placeholder);
    if let Some((node, _)) = leaves.clone().rev().find(|(_, range)| range.end == cursor) {
        return (Some(node.id.clone()), Edge::Right);
    }
    if let Some((node, _)) = leaves.clone().find(|(_, range)| range.start == cursor) {
        return (Some(node.id.clone()), Edge::Left);
    }
    if let Some((node, range)) = leaves
        .clone()
        .find(|(_, range)| range.start < cursor && cursor < range.end)
    {
        let span = (range.end - range.start) as f64;
        return (
            Some(node.id.clone()),
            Edge::Fraction((cursor - range.start) as f64 / span),
        );
    }
    let before_everything = nodes
        .iter()
        .filter(|(node, _)| node.children.is_empty())
        .all(|(_, range)| cursor <= range.start);
    if before_everything {
        (None, Edge::Left)
    } else {
        // 光标在结构的闭合花括号之后：取起点在光标之前、终点最靠后的外层结构右缘。
        let outer = nodes
            .iter()
            .filter(|(_, range)| range.end <= cursor)
            .max_by_key(|(_, range)| (range.end, std::cmp::Reverse(range.start)));
        (outer.map(|(node, _)| node.id.clone()), Edge::Right)
    }
}

fn collect<'a>(node: &'a MathNode, output: &mut Vec<(&'a MathNode, Range<usize>)>) {
    if let Some(range) = &node.source_range {
        output.push((node, range.start as usize..range.end as usize));
    }
    for child in &node.children {
        collect(child, output);
    }
}

/// 返回 (x, y0, y1)：x 相对公式盒左缘，y 相对公式基线（向下为正）。
fn node_edge(layout: &MathLayout, id: Option<&NodeId>, edge: Edge) -> Option<(f64, f64, f64)> {
    let Some(id) = id else {
        let x = match edge {
            Edge::Right => layout.width,
            _ => 0.0,
        };
        return Some((x, -layout.ascent, layout.descent));
    };
    let node = layout.nodes.iter().find(|node| &node.id == id)?;
    let x = match edge {
        Edge::Left => node.x,
        Edge::Right => node.x + node.width,
        Edge::Fraction(fraction) => node.x + node.width * fraction,
        Edge::Center => node.x + node.width / 2.0,
    };
    Some((x, node.y, node.y + node.height))
}
