//! 用行内公式布局快照投影命中和 MathFocus 插入点。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{DocumentLayoutSnapshot, LaidOutMathObject, LaidOutParagraph, PageCaretGeometry};
use crate::StoryId;
use crate::editing::{InlineMathTarget, MathFocus};
use caret_doc::NodeId;
use caret_layout::PositionedInlineObject;
use caret_math::MathNodeGeometry;

/// 页面上的行内公式落点。显示公式矩形不产生此结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineMathHit {
    pub paragraph_id: NodeId,
    pub story: StoryId,
    pub inline_math_id: NodeId,
    pub source_end: usize,
    /// 落点下最小节点的 ID。快照没有对应节点时为空，进入整棵树的边缘。
    pub node_id: Option<String>,
    /// 落点在所选节点中心左侧。没有节点时按整个对象盒判断。
    pub from_left: bool,
}

impl From<&InlineMathHit> for InlineMathTarget {
    fn from(hit: &InlineMathHit) -> Self {
        Self {
            paragraph_id: hit.paragraph_id.clone(),
            story: hit.story.clone(),
            inline_math_id: hit.inline_math_id.clone(),
            source_end: hit.source_end,
            node_id: hit.node_id.clone(),
            from_left: hit.from_left,
        }
    }
}

struct NodeBox {
    x_pt: f64,
    y_pt: f64,
    width_pt: f64,
    height_pt: f64,
}

impl DocumentLayoutSnapshot {
    /// 用公式节点盒和行内对象原点投影页面插入点。
    ///
    /// 节点缺失、几何不可表示或高度不是正数时返回 `None`。调用方回退到正文 caret，不为缺失节点编造盒。
    pub fn inline_math_caret_geometry(
        &self,
        focus: &MathFocus,
        fraction: f64,
    ) -> Option<PageCaretGeometry> {
        if !fraction.is_finite() {
            return None;
        }
        let paragraph = self.paragraph_by_id(&focus.paragraph_id)?;
        let math = paragraph
            .math_objects
            .iter()
            .find(|math| math.id == focus.inline_math_id)?;
        let object = positioned_object(paragraph, math)?;
        let location = paragraph.line_locations.get(object.line_index)?.as_ref()?;
        let page = self.pages.get(location.page_index)?;
        let node_id = focus.position.path.last()?;
        let node = math
            .layout
            .nodes
            .iter()
            .rev()
            .find(|node| node.node_id == node_id.as_str())?;
        let node_box = node_box(node)?;
        let fraction = fraction.clamp(0.0, 1.0);
        let x_pt = page.margin_left_pt
            + f64::from(object.x_pt)
            + node_box.x_pt
            + fraction * node_box.width_pt;
        let baseline_y_pt = location.y0_pt + f64::from(object.y_pt) + f64::from(object.baseline_pt);
        let y0_pt = baseline_y_pt + node_box.y_pt;
        let y1_pt = y0_pt + node_box.height_pt;
        finite_caret(location.page_index, x_pt, y0_pt, y1_pt)
    }

    /// 页面坐标落在行内公式盒内时返回进入信息。
    ///
    /// 显示公式矩形优先返回 `None`，避免块公式被当成行内对象。
    pub fn inline_math_hit(
        &self,
        page_index: usize,
        x_pt: f64,
        y_pt: f64,
    ) -> Option<InlineMathHit> {
        if !x_pt.is_finite() || !y_pt.is_finite() {
            return None;
        }
        let page = self.pages.get(page_index)?;
        if x_pt < 0.0 || y_pt < 0.0 || x_pt > page.width_pt || y_pt > page.height_pt {
            return None;
        }
        if self.display_math_regions.iter().any(|region| {
            region.page_index == page_index
                && x_pt >= region.x0_pt
                && x_pt <= region.x1_pt
                && y_pt >= region.y0_pt
                && y_pt <= region.y1_pt
        }) {
            return None;
        }
        for paragraph in &self.paragraphs {
            for object in &paragraph.layout.positioned_inline_objects {
                let Some(location) = paragraph
                    .line_locations
                    .get(object.line_index)
                    .and_then(Option::as_ref)
                else {
                    continue;
                };
                if location.page_index != page_index {
                    continue;
                }
                let left_pt = page.margin_left_pt + f64::from(object.x_pt);
                let top_pt = location.y0_pt + f64::from(object.y_pt);
                let right_pt = left_pt + f64::from(object.width_pt);
                let bottom_pt = top_pt + f64::from(object.height_pt);
                if x_pt < left_pt || x_pt > right_pt || y_pt < top_pt || y_pt > bottom_pt {
                    continue;
                }
                let Some(math) = math_for_object(paragraph, object) else {
                    continue;
                };
                let local_x_pt = x_pt - left_pt;
                let local_y_pt = y_pt - (top_pt + f64::from(object.baseline_pt));
                return Some(entry_from_local(
                    paragraph,
                    math,
                    object,
                    local_x_pt,
                    Some(local_y_pt),
                ));
            }
        }
        None
    }

    /// 段落局部 x 落在目标行某个行内公式盒内部时返回进入信息。
    ///
    /// 正好停在盒的左右边缘不算进入，那是公式旁边的正文插入点。
    pub fn inline_math_on_line(
        &self,
        paragraph_id: &NodeId,
        line_index: usize,
        content_x_pt: f32,
    ) -> Option<InlineMathHit> {
        if !content_x_pt.is_finite() {
            return None;
        }
        let paragraph = self.paragraph_by_id(paragraph_id)?;
        for object in &paragraph.layout.positioned_inline_objects {
            if object.line_index != line_index {
                continue;
            }
            let left_pt = object.x_pt;
            let right_pt = left_pt + object.width_pt;
            if !(content_x_pt > left_pt && content_x_pt < right_pt) {
                continue;
            }
            let math = math_for_object(paragraph, object)?;
            return Some(entry_from_local(
                paragraph,
                math,
                object,
                f64::from(content_x_pt - left_pt),
                None,
            ));
        }
        None
    }
}

fn finite_caret(page_index: usize, x_pt: f64, y0_pt: f64, y1_pt: f64) -> Option<PageCaretGeometry> {
    if x_pt.is_finite() && y0_pt.is_finite() && y1_pt.is_finite() && y1_pt > y0_pt {
        Some(PageCaretGeometry {
            page_index,
            x_pt,
            y0_pt,
            y1_pt,
        })
    } else {
        None
    }
}

fn positioned_object<'a>(
    paragraph: &'a LaidOutParagraph,
    math: &LaidOutMathObject,
) -> Option<&'a PositionedInlineObject> {
    paragraph
        .layout
        .positioned_inline_objects
        .iter()
        .find(|object| object.id == math.inline_box_id && object.source_bytes == math.source_bytes)
}

fn math_for_object<'a>(
    paragraph: &'a LaidOutParagraph,
    object: &PositionedInlineObject,
) -> Option<&'a LaidOutMathObject> {
    paragraph
        .math_objects
        .iter()
        .find(|math| math.inline_box_id == object.id && math.source_bytes == object.source_bytes)
}

fn entry_from_local(
    paragraph: &LaidOutParagraph,
    math: &LaidOutMathObject,
    object: &PositionedInlineObject,
    local_x_pt: f64,
    local_y_pt: Option<f64>,
) -> InlineMathHit {
    let chosen = choose_node(&math.layout.nodes, local_x_pt, local_y_pt);
    let (node_id, from_left) = match chosen {
        Some((node_id, from_left)) => (Some(node_id), from_left),
        None => (None, local_x_pt < f64::from(object.width_pt) / 2.0),
    };
    InlineMathHit {
        paragraph_id: paragraph.id.clone(),
        story: paragraph.story.clone(),
        inline_math_id: math.id.clone(),
        source_end: object.source_bytes.end,
        node_id,
        from_left,
    }
}

fn choose_node(
    nodes: &[MathNodeGeometry],
    local_x_pt: f64,
    local_y_pt: Option<f64>,
) -> Option<(String, bool)> {
    let mut best: Option<(usize, i128)> = None;
    for (index, node) in nodes.iter().enumerate() {
        let Some(node_box) = node_box(node) else {
            continue;
        };
        let inside_x =
            local_x_pt >= node_box.x_pt && local_x_pt <= node_box.x_pt + node_box.width_pt;
        let inside_y = local_y_pt.is_none_or(|local_y_pt| {
            local_y_pt >= node_box.y_pt && local_y_pt <= node_box.y_pt + node_box.height_pt
        });
        if !inside_x || !inside_y {
            continue;
        }
        let area = node_area(node).unwrap_or(i128::MAX);
        let replace = match best {
            None => true,
            Some((best_index, best_area)) => {
                area < best_area || (area == best_area && index > best_index)
            }
        };
        if replace {
            best = Some((index, area));
        }
    }
    let (index, _) = best?;
    let node_box = node_box(&nodes[index])?;
    let from_left = local_x_pt < node_box.x_pt + node_box.width_pt / 2.0;
    Some((nodes[index].node_id.clone(), from_left))
}

fn node_box(node: &MathNodeGeometry) -> Option<NodeBox> {
    let x_pt = micro_to_f64(node.x_micro_pt)?;
    let y_pt = micro_to_f64(node.y_micro_pt)?;
    let width_pt = micro_to_f64(node.width_micro_pt)?;
    let ascent_pt = micro_to_f64(node.ascent_micro_pt)?;
    let descent_pt = micro_to_f64(node.descent_micro_pt)?;
    if width_pt < 0.0 || ascent_pt < 0.0 || descent_pt < 0.0 {
        return None;
    }
    let height_pt = ascent_pt + descent_pt;
    height_pt.is_finite().then_some(NodeBox {
        x_pt,
        y_pt,
        width_pt,
        height_pt,
    })
}

fn node_area(node: &MathNodeGeometry) -> Option<i128> {
    let width = i128::from(node.width_micro_pt);
    let height = i128::from(node.ascent_micro_pt).checked_add(i128::from(node.descent_micro_pt))?;
    if width < 0 || height < 0 {
        return None;
    }
    width.checked_mul(height)
}

fn micro_to_f64(value: i64) -> Option<f64> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "百万分之一磅转 pt，页面尺度内精度足够"
    )]
    let points = value as f64 / 1_000_000.0;
    points.is_finite().then_some(points)
}
