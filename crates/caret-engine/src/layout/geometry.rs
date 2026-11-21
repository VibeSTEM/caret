//! 计算块边界、正文与公式树的页面几何。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct ParagraphTextRange {
    pub(super) start_index: usize,
    pub(super) start_offset: usize,
    pub(super) end_index: usize,
    pub(super) end_offset: usize,
}

/// 结构深度超过该层后 caret 已达 1pt 最小可见高度，继续减半没有视觉收益。
const STRUCTURE_CARET_MAX_HALVINGS: u32 = 5;

impl MathPageProjection {
    /// 生成正文基础行高的 caret，用于公式根层、块边界与命中容差。
    pub(super) fn caret_at_math_baseline(
        &self,
        x_pt: f64,
        math_baseline_y_pt: f64,
    ) -> PageCaretGeometry {
        let baseline_shift = math_baseline_y_pt - self.caret_baseline_y_pt;
        PageCaretGeometry {
            page_index: self.page_index,
            x_pt,
            y0_pt: self.caret_line_top_pt + baseline_shift,
            y1_pt: self.caret_line_top_pt + baseline_shift + self.caret_height_pt,
        }
    }

    /// 结构槽插入点 caret 高度：每进入一层结构槽减半，并收进槽位布局盒。
    ///
    /// 结构槽（上下标、分式分子分母、根式等）只占宿主行高的一部分；用正文行高
    /// 会在空槽或高公式中伸出槽位与行框。高度按结构深度 `1/2^depth` 缩放，
    /// 不超过槽位盒可用高度，并保留 1pt 最小可见高度；垂直居中由调用方给出。
    pub(super) fn structure_caret_height_pt(&self, depth: u32, slot_height_pt: f64) -> f64 {
        let scale = 0.5_f64.powi(STRUCTURE_CARET_MAX_HALVINGS.min(depth) as i32);
        let scaling_height = self.caret_height_pt * scale;
        // 零高度槽（尤其是空可选占位槽）仍按深度缩放，再由 1pt 下限保证可见；
        // 只有非有限/负高度才退回正文行高作为无效几何的兜底。
        let slot_height_pt = if slot_height_pt.is_finite() && slot_height_pt >= 0.0 {
            slot_height_pt
        } else {
            self.caret_height_pt
        };
        scaling_height.min(slot_height_pt).max(1.0)
    }

    /// 在给定垂直中心生成结构槽 caret。
    pub(super) fn structure_caret_geometry(
        &self,
        x_pt: f64,
        center_y_pt: f64,
        depth: u32,
        slot_height_pt: f64,
    ) -> PageCaretGeometry {
        let height_pt = self.structure_caret_height_pt(depth, slot_height_pt);
        PageCaretGeometry {
            page_index: self.page_index,
            x_pt,
            y0_pt: center_y_pt - height_pt / 2.0,
            y1_pt: center_y_pt + height_pt / 2.0,
        }
    }

    /// 完全不可见的几何不发布；部分可见的几何取行裁切交集。
    pub(super) fn clip_range(&self, top: f64, bottom: f64) -> Option<(f64, f64)> {
        let (top, bottom) = match self.clip_y_pt {
            Some([clip_top, clip_bottom]) => (top.max(clip_top), bottom.min(clip_bottom)),
            None => (top, bottom),
        };
        (top < bottom).then_some((top, bottom))
    }

    pub(super) fn contains_y(&self, y: f64) -> bool {
        self.clip_y_pt
            .is_none_or(|[top, bottom]| y >= top && y <= bottom)
    }

    pub(super) fn row_clip(&self, row_id: Option<&NodeId>) -> Option<[f64; 2]> {
        let row_id = row_id?;
        self.row_clips
            .iter()
            .find(|clip| &clip.row_id == row_id)
            .map(|clip| [clip.y0_pt, clip.y1_pt])
    }

    pub(super) fn contains_y_for_row(&self, y: f64, row_id: Option<&NodeId>) -> bool {
        self.row_clip(row_id)
            .is_none_or(|[top, bottom]| y >= top && y <= bottom)
            && self.contains_y(y)
    }

    pub(super) fn clip_range_for_row(
        &self,
        top: f64,
        bottom: f64,
        row_id: Option<&NodeId>,
    ) -> Option<(f64, f64)> {
        let (top, bottom) = match (self.row_clip(row_id), self.clip_y_pt) {
            (Some([row_top, row_bottom]), Some([clip_top, clip_bottom])) => (
                top.max(row_top).max(clip_top),
                bottom.min(row_bottom).min(clip_bottom),
            ),
            (Some([row_top, row_bottom]), None) => (top.max(row_top), bottom.min(row_bottom)),
            (None, _) => return self.clip_range(top, bottom),
        };
        (top < bottom).then_some((top, bottom))
    }

    pub(super) fn clip_caret(&self, mut caret: PageCaretGeometry) -> Option<PageCaretGeometry> {
        let Some([clip_top, clip_bottom]) = self.clip_y_pt else {
            return Some(caret);
        };
        if let Some((top, bottom)) = self.clip_range(caret.y0_pt, caret.y1_pt) {
            (caret.y0_pt, caret.y1_pt) = (top, bottom);
            return Some(caret);
        }

        // Exactly 行距可完全裁掉数学内容；逻辑光标仍需锚定到最近可见边缘。
        let height = (caret.y1_pt - caret.y0_pt).min(clip_bottom - clip_top);
        if (caret.y0_pt + caret.y1_pt) / 2.0 < clip_top {
            caret.y0_pt = clip_top;
            caret.y1_pt = clip_top + height;
        } else {
            caret.y0_pt = clip_bottom - height;
            caret.y1_pt = clip_bottom;
        }
        Some(caret)
    }

    pub(super) fn clip_caret_for_row(
        &self,
        mut caret: PageCaretGeometry,
        row_id: Option<&NodeId>,
    ) -> Option<PageCaretGeometry> {
        let Some([row_top, row_bottom]) = self.row_clip(row_id) else {
            return self.clip_caret(caret);
        };
        let Some((top, bottom)) = self.clip_range_for_row(caret.y0_pt, caret.y1_pt, row_id) else {
            let height = (caret.y1_pt - caret.y0_pt).min(row_bottom - row_top);
            if (caret.y0_pt + caret.y1_pt) / 2.0 < row_top {
                caret.y0_pt = row_top;
                caret.y1_pt = row_top + height;
            } else {
                caret.y0_pt = row_bottom - height;
                caret.y1_pt = row_bottom;
            }
            return Some(caret);
        };
        caret.y0_pt = top;
        caret.y1_pt = bottom;
        Some(caret)
    }

    pub(crate) fn drawing_clip(&self) -> Option<[f64; 2]> {
        self.clip_y_pt
    }
}

impl DocumentLayoutSnapshot {
    /// 返回显示公式块首尾的正文高度光标几何，Align 使用首行/末行边缘。
    pub(crate) fn math_block_boundary_caret(
        &self,
        block_id: &NodeId,
        direction: crate::editing::Direction,
    ) -> Option<PageCaretGeometry> {
        let mut projections = self
            .math_projection_index
            .by_id
            .get(block_id)?
            .iter()
            .filter_map(|index| self.math_projection_index.projections.get(*index))
            .filter(|projection| projection.inline_index.is_none())
            .collect::<Vec<_>>();
        projections.sort_by_key(|projection| projection.page_index);
        let projection = match direction {
            crate::editing::Direction::Backward => projections.first()?,
            crate::editing::Direction::Forward => projections.last()?,
        };
        let page = self.pages.get(projection.page_index)?;
        let x_pt = match (direction, projection.clip_x_pt) {
            (crate::editing::Direction::Backward, Some([x0, _])) => x0,
            (crate::editing::Direction::Forward, Some([_, x1])) => x1,
            (crate::editing::Direction::Backward, None) => page.margin_left_pt,
            (crate::editing::Direction::Forward, None) => page.width_pt - page.margin_right_pt,
        };
        let row_baseline = match direction {
            crate::editing::Direction::Backward => projection.row_baselines.first(),
            crate::editing::Direction::Forward => projection.row_baselines.last(),
        };
        Some(row_baseline.map_or_else(
            || PageCaretGeometry {
                page_index: projection.page_index,
                x_pt,
                y0_pt: projection.caret_line_top_pt,
                y1_pt: projection.caret_line_top_pt + projection.caret_height_pt,
            },
            |row| projection.caret_at_math_baseline(x_pt, row.baseline_y_pt),
        ))
    }

    /// 返回块外侧停靠点的页面 x 坐标；环境容器和显示公式消费同一边界查询。
    pub fn block_boundary_x(
        &self,
        block_id: &NodeId,
        page_index: usize,
        direction: crate::editing::Direction,
    ) -> Option<f64> {
        let environment_edge = self
            .environment_ranges
            .iter()
            .filter(|range| &range.environment_id == block_id && range.page_index == page_index)
            .map(|range| match direction {
                crate::editing::Direction::Backward => range.x0_pt,
                crate::editing::Direction::Forward => range.x1_pt,
            })
            .next();
        if environment_edge.is_some() {
            return environment_edge;
        }
        let projection = self
            .math_projection_index
            .by_id
            .get(block_id)?
            .iter()
            .filter_map(|index| self.math_projection_index.projections.get(*index))
            .find(|projection| {
                projection.page_index == page_index && projection.inline_index.is_none()
            })?;
        let page = self.pages.get(page_index)?;
        match (direction, projection.clip_x_pt) {
            (crate::editing::Direction::Backward, Some([x0, _])) => Some(x0),
            (crate::editing::Direction::Forward, Some([_, x1])) => Some(x1),
            (crate::editing::Direction::Backward, None) => Some(page.margin_left_pt),
            (crate::editing::Direction::Forward, None) => {
                Some(page.width_pt - page.margin_right_pt)
            }
        }
    }

    /// 将块类型的边界几何适配合并到共享外侧停靠光标投影。
    pub fn block_boundary_caret(
        &self,
        block_id: &NodeId,
        direction: crate::editing::Direction,
        mut fallback: PageCaretGeometry,
    ) -> PageCaretGeometry {
        if let Some(caret) = self.math_block_boundary_caret(block_id, direction) {
            return caret;
        }
        fallback.x_pt = self
            .block_boundary_x(block_id, fallback.page_index, direction)
            .unwrap_or(fallback.x_pt);
        fallback
    }

    /// 按页索引取连续片段，避免命中时扫描其他页面的公式。
    pub(super) fn math_projections_on_page(&self, page_index: usize) -> &[MathPageProjection] {
        let start = self
            .math_projection_index
            .projections
            .partition_point(|projection| projection.page_index < page_index);
        let end = self
            .math_projection_index
            .projections
            .partition_point(|projection| projection.page_index <= page_index);
        &self.math_projection_index.projections[start..end]
    }

    pub(crate) fn math_projections_for_target(
        &self,
        target: &MathEditTarget,
    ) -> impl Iterator<Item = &MathPageProjection> {
        let math_id = match target {
            MathEditTarget::Inline { math_id, .. } => math_id,
            MathEditTarget::Block { block_id, .. } => block_id,
        };
        self.math_projection_index
            .by_id
            .get(math_id)
            .into_iter()
            .flatten()
            .filter_map(|index| self.math_projection_index.projections.get(*index))
    }
}

impl DocumentLayoutSnapshot {
    /// 根据同代公式结构插入点返回所属 MathFlow 的页面 caret 几何。
    pub fn math_tree_caret_geometry(
        &self,
        target: &MathEditTarget,
        position: &MathTreePosition,
    ) -> Option<PageCaretGeometry> {
        self.math_tree_caret(target, position)
            .map(|(caret, _)| caret)
    }

    /// 把公式布局节点解析为所在结构序列：路径、序列长度和子项是否为环境子节点。
    ///
    /// MathFlow 聚合成节点（非 TextFlow）对应其内容序列，透明容器或 InlineEnvironment
    /// 对应环境子节点序列；其他布局盒不提供序列位置。
    pub(super) fn math_layout_sequence(
        &self,
        projection: &MathPageProjection,
        node: &caret_math::layout::MathNodeBox,
    ) -> Option<(MathSequencePath, usize, bool)> {
        if node.is_flow && !node.is_text_flow {
            let root = self.math_scope_root(projection, &node.scope_id)?;
            let flow = math_flow_at_layout_path(root, &node.path)?;
            Some((
                math_sequence_path(&node.path, MathSequenceEnd::Content)?,
                flow.content.len(),
                false,
            ))
        } else if node.is_container {
            let root = self.math_scope_root(projection, &node.scope_id)?;
            let environment = math_environment_at_layout_path(root, &node.path)?;
            Some((
                math_sequence_path(&node.path, MathSequenceEnd::EnvironmentChildren)?,
                environment.children.len(),
                true,
            ))
        } else {
            None
        }
    }

    /// 根据树原生数学插入位置返回同一数学盒中的光标几何和有效字号。
    pub fn math_tree_caret(
        &self,
        target: &MathEditTarget,
        position: &MathTreePosition,
    ) -> Option<(PageCaretGeometry, u16)> {
        for projection in self.math_projections_for_target(target) {
            let Some((layout, _)) = self.math_projection_items(projection) else {
                continue;
            };
            let Some(host) = self.math_host(&projection.math_id) else {
                continue;
            };
            if &host.target != target {
                continue;
            }
            for node in &layout.nodes {
                let Some((sequence_path, sequence_len, environment_children)) =
                    self.math_layout_sequence(projection, node)
                else {
                    continue;
                };
                if !node.x.is_finite()
                    || !node.y.is_finite()
                    || !node.baseline.is_finite()
                    || !node.width.is_finite()
                    || !node.height.is_finite()
                    || node.height < 0.0
                {
                    continue;
                }
                let Some(scope_host) = host.for_scope(&node.scope_id) else {
                    continue;
                };
                if scope_host.host != position.host || sequence_path != position.path {
                    continue;
                }
                if position.offset > sequence_len {
                    continue;
                }
                // 结构槽 caret 按排版给出的结构深度缩放并放在槽位布局盒中心；
                // 根层序列仍用正文基础行高。边界相邻项的字号只用于报告插入点字号。
                let structural_depth = node.structural_depth;
                let edge_child_indices = math_sequence_edge_child_indices(
                    position.offset,
                    sequence_len,
                    position.affinity,
                );
                let mut edge_geometry = None;
                for child_index in edge_child_indices.into_iter().flatten() {
                    let mut child_path = node.path.clone();
                    child_path.push(if environment_children {
                        MathPathStep::EnvironmentChild(child_index)
                    } else {
                        MathPathStep::Content(child_index)
                    });
                    if let Some(child) = layout
                        .nodes
                        .iter()
                        .find(|child| child.scope_id == node.scope_id && child.path == child_path)
                    {
                        if child.is_container {
                            let nested_position =
                                self.math_scope_root(projection, &node.scope_id)
                                    .and_then(|root| {
                                        math_environment_at_layout_path(root, &child.path).and_then(
                                            |environment| {
                                                math_sequence_path(
                                                    &child.path,
                                                    MathSequenceEnd::EnvironmentChildren,
                                                )
                                                .map(|path| {
                                                    let at_start = child_index == position.offset;
                                                    MathTreePosition {
                                                        host: position.host.clone(),
                                                        path,
                                                        offset: if at_start {
                                                            0
                                                        } else {
                                                            environment.children.len()
                                                        },
                                                        affinity: if at_start {
                                                            MathPositionAffinity::Downstream
                                                        } else {
                                                            MathPositionAffinity::Upstream
                                                        },
                                                    }
                                                })
                                            },
                                        )
                                    });
                            if let Some(caret) = nested_position
                                .as_ref()
                                .and_then(|nested| self.math_tree_caret(target, nested))
                            {
                                return Some(caret);
                            }
                        }
                        if !child.x.is_finite()
                            || !child.y.is_finite()
                            || !child.height.is_finite()
                            || child.height <= 0.0
                        {
                            continue;
                        }
                        if child.is_text_flow {
                            let text_path = MathTextFlowPath::from_layout_path(
                                child.path.clone(),
                                scope_host.align_cell,
                            );
                            let text_length = self
                                .math_scope_root(projection, &node.scope_id)
                                .and_then(|root| math_text_flow_at_layout_path(root, &child.path))
                                .map(|text| {
                                    text.runs.iter().map(|run| run.text.len()).sum::<usize>()
                                });
                            if let Some(text_length) = text_length {
                                let (offset, affinity) = if child_index == position.offset {
                                    (0, MathPositionAffinity::Downstream)
                                } else {
                                    (text_length, MathPositionAffinity::Upstream)
                                };
                                let text_position =
                                    MathTextPosition::new(text_path, offset, affinity);
                                let visual_affinity = match affinity {
                                    MathPositionAffinity::Upstream => VisualAffinity::Upstream,
                                    MathPositionAffinity::Downstream => VisualAffinity::Downstream,
                                };
                                if let Some(caret) = self
                                    .math_text_caret_with_clipping(
                                        target,
                                        &text_position,
                                        visual_affinity,
                                        true,
                                    )
                                    .map(|(caret, _)| caret)
                                {
                                    let font_size_centipoints = child
                                        .font_size_pt
                                        .and_then(|size| visible_font_size_centipoints(size as f32))
                                        .unwrap_or(self.default_font_size_centipoints);
                                    return Some((caret, font_size_centipoints));
                                }
                            }
                        }
                        if edge_geometry.is_none() {
                            edge_geometry = Some((
                                if child_index == position.offset {
                                    child.x
                                } else {
                                    child.x + child.width
                                },
                                child.baseline,
                                child.font_size_pt,
                            ));
                        }
                        break;
                    }
                }
                let (local_x, caret_baseline, edge_font_size) =
                    edge_geometry.unwrap_or_else(|| {
                        let local_x = if position.offset == 0 {
                            node.x
                        } else if position.offset == sequence_len {
                            node.x + node.width
                        } else {
                            let mut child_path = node.path.clone();
                            child_path.push(if environment_children {
                                MathPathStep::EnvironmentChild(position.offset)
                            } else {
                                MathPathStep::Content(position.offset)
                            });
                            layout
                                .nodes
                                .iter()
                                .find(|child| {
                                    child.scope_id == node.scope_id && child.path == child_path
                                })
                                .map_or(node.x + node.width, |child| child.x)
                        };
                        (local_x, node.baseline, node.font_size_pt)
                    });
                let raw_caret = if structural_depth == 0 {
                    projection.caret_at_math_baseline(
                        projection.object_x_pt + local_x,
                        projection.baseline_y_pt + caret_baseline,
                    )
                } else {
                    projection.structure_caret_geometry(
                        projection.object_x_pt + local_x,
                        projection.baseline_y_pt + node.y + node.height / 2.0,
                        structural_depth,
                        node.height,
                    )
                };
                let Some(caret) =
                    projection.clip_caret_for_row(raw_caret, math_row_id_for_host(&position.host))
                else {
                    continue;
                };
                let font_size_centipoints = edge_font_size
                    .and_then(|size| visible_font_size_centipoints(size as f32))
                    .unwrap_or(self.default_font_size_centipoints);
                return Some((caret, font_size_centipoints));
            }
        }
        None
    }

    /// 返回公式树内部同一序列中锚点到焦点的页面选区矩形。
    ///
    /// 只接受同一宿主、同一序列路径的端点：扩选不进入嵌套结构，焦点离开锚点所在
    /// 序列时由调用方转换为整个公式宿主的文档范围，因此跨路径端点不属于本查询。
    /// 每个被选结构项独立投影：嵌套 Math/Environment 作为原子取整盒；纵向范围使用
    /// 同位置 caret 的正文行框，Align 按所属行裁切。空范围返回空矩形而不是过期投影。
    pub fn math_tree_selection_geometry(
        &self,
        target: &MathEditTarget,
        anchor: &MathTreePosition,
        focus: &MathTreePosition,
    ) -> Option<Vec<PageSelectionRect>> {
        if anchor.host != focus.host || anchor.path != focus.path {
            return None;
        }
        let (start, end) = if anchor.offset <= focus.offset {
            (anchor.offset, focus.offset)
        } else {
            (focus.offset, anchor.offset)
        };
        if start == end {
            return Some(Vec::new());
        }
        for projection in self.math_projections_for_target(target) {
            let Some((layout, _)) = self.math_projection_items(projection) else {
                continue;
            };
            let Some(host) = self.math_host(&projection.math_id) else {
                continue;
            };
            if &host.target != target {
                continue;
            }
            for node in &layout.nodes {
                let Some((sequence_path, sequence_len, environment_children)) =
                    self.math_layout_sequence(projection, node)
                else {
                    continue;
                };
                let Some(scope_host) = host.for_scope(&node.scope_id) else {
                    continue;
                };
                if scope_host.host != anchor.host || sequence_path != anchor.path {
                    continue;
                }
                if end > sequence_len {
                    continue;
                }
                // 选区矩形与同槽 caret 共用结构深度与槽位盒高度。
                let structural_depth = node.structural_depth;
                let row_id = math_row_id_for_host(&anchor.host);
                let mut rectangles = Vec::with_capacity(end - start);
                for index in start..end {
                    let mut child_path = node.path.clone();
                    child_path.push(if environment_children {
                        MathPathStep::EnvironmentChild(index)
                    } else {
                        MathPathStep::Content(index)
                    });
                    let Some(child) = layout
                        .nodes
                        .iter()
                        .find(|child| child.scope_id == node.scope_id && child.path == child_path)
                    else {
                        continue;
                    };
                    if !child.x.is_finite()
                        || !child.y.is_finite()
                        || !child.width.is_finite()
                        || !child.height.is_finite()
                        || !child.baseline.is_finite()
                        || child.width <= 0.0
                    {
                        continue;
                    }
                    let x0_pt = projection.object_x_pt + child.x;
                    let x1_pt = x0_pt + child.width;
                    let caret_band = if structural_depth == 0 {
                        projection.caret_at_math_baseline(
                            x0_pt,
                            projection.baseline_y_pt + child.baseline,
                        )
                    } else {
                        projection.structure_caret_geometry(
                            x0_pt,
                            projection.baseline_y_pt + child.y + child.height / 2.0,
                            structural_depth,
                            child.height,
                        )
                    };
                    let Some((y0_pt, y1_pt)) =
                        projection.clip_range_for_row(caret_band.y0_pt, caret_band.y1_pt, row_id)
                    else {
                        continue;
                    };
                    rectangles.push(PageSelectionRect {
                        page_index: projection.page_index,
                        x0_pt,
                        y0_pt,
                        x1_pt,
                        y1_pt,
                    });
                }
                return Some(rectangles);
            }
        }
        None
    }
}

impl DocumentLayoutSnapshot {
    /// 根据已提交的 Rust 光标位置返回插入点几何，覆盖分页片段和自定义行框。
    pub fn caret_geometry(
        &self,
        position: &Pos,
        visual_affinity: VisualAffinity,
    ) -> Option<PageCaretGeometry> {
        let paragraph = self.paragraph_by_id(&position.paragraph_id)?;
        if paragraph.story != position.story {
            return None;
        }
        let cursor = paragraph
            .layout
            .cursor_at(position.offset, visual_affinity)?;
        let (bounds, line_index) = paragraph.layout.cursor_geometry_with_line(cursor, 0.0)?;
        let line = paragraph.layout.paragraph.lines.get(line_index)?;
        let location = paragraph.line_locations.get(line_index)?.as_ref()?;
        let page = self.pages.get(location.page_index)?;
        let mut geometry = PageCaretGeometry {
            page_index: location.page_index,
            x_pt: location.x0_pt + bounds.x0,
            y0_pt: location.y0_pt,
            y1_pt: location.y0_pt + f64::from(line.metrics.height_pt),
        };
        if let Some(clip) = location.clip
            && (!clip.clamp_caret(&mut geometry) || geometry.y0_pt > page.height_pt)
        {
            return None;
        }
        Some(geometry)
    }

    /// 当前焦点所在单元格的编辑辅助线矩形；焦点不在表格内时为空。
    ///
    /// 辅助线只表达编辑焦点，不进入 `.cdoc`，也不参与命中。
    pub fn table_guidelines(&self, position: &Pos) -> Vec<PageSelectionRect> {
        let Some(paragraph) = self.paragraph_by_id(&position.paragraph_id) else {
            return Vec::new();
        };
        if paragraph.story != position.story {
            return Vec::new();
        }
        let Some(cell) = paragraph.container.innermost_cell() else {
            return Vec::new();
        };
        let Some(grid) = self.table_layouts.get(&cell.table_id) else {
            return Vec::new();
        };
        let Some(row) = grid.rows.get(cell.row_index) else {
            return Vec::new();
        };
        let Some(cell_geometry) = row.cells.get(cell.cell_index) else {
            return Vec::new();
        };
        for (page_index, page) in self.pages.iter().enumerate() {
            for fragment in &page.layout.table_fragments {
                if fragment.table_id != cell.table_id
                    || !fragment.row_range.contains(&cell.row_index)
                {
                    continue;
                }
                let x0_pt = page.margin_left_pt
                    + f64::from(fragment.x_offset_pt)
                    + cell_geometry.content_x_pt;
                let y0_pt = page.margin_top_pt + f64::from(fragment.y_offset_pt) + row.top_pt;
                let x1_pt = x0_pt + cell_geometry.content_width_pt;
                let y1_pt = y0_pt + row.height_pt;
                let x0_pt = x0_pt.clamp(0.0, page.width_pt);
                let x1_pt = x1_pt.clamp(0.0, page.width_pt);
                let y0_pt = y0_pt.clamp(0.0, page.height_pt);
                let y1_pt = y1_pt.clamp(0.0, page.height_pt);
                if !x0_pt.is_finite()
                    || !y0_pt.is_finite()
                    || !x1_pt.is_finite()
                    || !y1_pt.is_finite()
                    || x1_pt <= x0_pt
                    || y1_pt <= y0_pt
                {
                    continue;
                }
                return vec![PageSelectionRect {
                    page_index,
                    x0_pt,
                    y0_pt,
                    x1_pt,
                    y1_pt,
                }];
            }
        }
        Vec::new()
    }

    /// 返回裁切到可见分页片段内的选区矩形。
    pub fn selection_geometry(&self, selection: &Selection) -> Option<Vec<PageSelectionRect>> {
        self.range_geometry(selection, false)
    }

    /// 返回按字体度量定位的预编辑下划线。
    pub fn composition_geometry(&self, selection: &Selection) -> Option<Vec<PageSelectionRect>> {
        self.range_geometry(selection, true)
    }

    /// 返回跨 Story 结构选区中环境外文字的几何；选中的块环境由环境矩形单独绘制。
    pub(super) fn structure_selection_geometry(
        &self,
        anchor: &Pos,
        focus: &Pos,
        selected_environment_ids: &[NodeId],
    ) -> Option<Vec<PageSelectionRect>> {
        let anchor_index = *self.paragraph_lookup.get(&anchor.paragraph_id)?;
        let focus_index = *self.paragraph_lookup.get(&focus.paragraph_id)?;
        let anchor_paragraph = self.paragraphs.get(anchor_index)?;
        let focus_paragraph = self.paragraphs.get(focus_index)?;
        if anchor_paragraph.story != anchor.story
            || focus_paragraph.story != focus.story
            || !Self::valid_text_offset(anchor_paragraph, anchor.offset)
            || !Self::valid_text_offset(focus_paragraph, focus.offset)
        {
            return None;
        }
        let (start_index, start_offset, end_index, end_offset) =
            if (anchor_index, anchor.offset) <= (focus_index, focus.offset) {
                (anchor_index, anchor.offset, focus_index, focus.offset)
            } else {
                (focus_index, focus.offset, anchor_index, anchor.offset)
            };
        self.text_range_geometry(
            ParagraphTextRange {
                start_index,
                start_offset,
                end_index,
                end_offset,
            },
            false,
            None,
            selected_environment_ids,
        )
    }

    /// 返回选中块级公式在同一代布局中的整栏矩形。
    ///
    /// 每个矩形横跨页面版心宽度，纵向采用该公式片段的 MathBox 范围并裁切到页面，
    /// 供选区层与活动公式高亮共用同一几何来源；不进入公式内部 MathBox。
    /// 任一公式 ID 在当前快照中没有任何分页片段时返回 `None`，让调用方拒绝过期投影。
    ///
    /// 退化片段（零高度或裁切后纵向区间为空）在当前布局契约下不可达：Equation
    /// 无 soft break 时宿主行框由正文行网格撑开正高度（`calculate_math_host_metrics`），
    /// 含 soft break 的 Equation 与 Align 由 `calculate_math_align_host_metrics`
    /// 拒绝非正高度，分页再拒绝负高度；即使公式行高于整页，页边距校验也保证裁切后
    /// 仍为正高度。因此下面的跳过分支只是防御性检查，不应据此断言快照过期。
    pub fn math_block_geometry(&self, block_ids: &[NodeId]) -> Option<Vec<PageSelectionRect>> {
        if block_ids.is_empty() {
            return Some(Vec::new());
        }
        let mut found = vec![false; block_ids.len()];
        let mut rectangles = Vec::new();
        for (page_index, page) in self.pages.iter().enumerate() {
            let x0_pt = page.margin_left_pt;
            let x1_pt = page.width_pt - page.margin_right_pt;
            if !x0_pt.is_finite() || !x1_pt.is_finite() || x1_pt <= x0_pt {
                return None;
            }
            for fragment in &page.layout.math_fragments {
                let Some(index) = block_ids
                    .iter()
                    .position(|block_id| block_id == &fragment.math_id)
                else {
                    continue;
                };
                let y0_pt = (page.margin_top_pt + f64::from(fragment.y_offset_pt))
                    .clamp(0.0, page.height_pt);
                let y1_pt = (page.margin_top_pt
                    + f64::from(fragment.y_offset_pt)
                    + f64::from(fragment.height_pt))
                .clamp(0.0, page.height_pt);
                if !y0_pt.is_finite() || !y1_pt.is_finite() || y1_pt <= y0_pt {
                    continue;
                }
                // 只有真正产出矩形后才标记该 ID 已定位，退化片段不能让整体误判为成功。
                // 当前布局契约保证片段高度为正且裁切区间非空，见函数文档。
                found[index] = true;
                rectangles.push(PageSelectionRect {
                    page_index,
                    x0_pt,
                    y0_pt,
                    x1_pt,
                    y1_pt,
                });
            }
            for placement in &page.table_math_placements {
                let Some(index) = block_ids
                    .iter()
                    .position(|block_id| block_id == &placement.math_id)
                else {
                    continue;
                };
                let x0_pt = placement.object_x_pt.clamp(0.0, page.width_pt);
                let x1_pt =
                    (placement.object_x_pt + placement.width_pt).clamp(x0_pt, page.width_pt);
                let mut y0_pt =
                    (placement.baseline_y_pt - placement.ascent_pt).clamp(0.0, page.height_pt);
                let mut y1_pt =
                    (placement.baseline_y_pt + placement.descent_pt).clamp(0.0, page.height_pt);
                if let Some([clip_y0, clip_y1]) = placement.clip_y_pt {
                    y0_pt = y0_pt.max(clip_y0);
                    y1_pt = y1_pt.min(clip_y1);
                }
                if y1_pt <= y0_pt || x1_pt <= x0_pt {
                    continue;
                }
                found[index] = true;
                rectangles.push(PageSelectionRect {
                    page_index,
                    x0_pt,
                    y0_pt,
                    x1_pt,
                    y1_pt,
                });
            }
        }
        // 表格单元格内被版心裁剪掉（不可见/不可导航）的公式：降级为空高亮，
        // 让编辑继续发布快照，而不是把所有 table math 误判为过期投影。
        for (index, block_id) in block_ids.iter().enumerate() {
            if !found[index] && self.table_math_ids.contains(block_id) {
                found[index] = true;
            }
        }
        found.iter().all(|found| *found).then_some(rectangles)
    }

    /// 返回结构选区在同一代布局中的全部可见矩形：环境外正文、被选块环境与公式块。
    ///
    /// 文本端点必须与本快照同代；选中的公式 ID 任一缺少分页片段时整体
    /// 返回 `None`。空块环境没有排版内容，因此没有环境矩形，这不视为过期投影。
    pub fn structure_selection_rectangles(
        &self,
        anchor: &Pos,
        focus: &Pos,
        selected_environment_ids: &[NodeId],
        selected_math_block_ids: &[NodeId],
    ) -> Option<Vec<PageSelectionRect>> {
        let mut rectangles =
            self.structure_selection_geometry(anchor, focus, selected_environment_ids)?;
        rectangles.extend(
            self.environment_ranges
                .iter()
                .filter(|range| selected_environment_ids.contains(&range.environment_id))
                .map(|range| PageSelectionRect {
                    page_index: range.page_index,
                    x0_pt: range.x0_pt,
                    y0_pt: range.y0_pt,
                    x1_pt: range.x1_pt,
                    y1_pt: range.y1_pt,
                }),
        );
        rectangles.extend(self.math_block_geometry(selected_math_block_ids)?);
        Some(rectangles)
    }

    pub(super) fn range_geometry(
        &self,
        selection: &Selection,
        composition: bool,
    ) -> Option<Vec<PageSelectionRect>> {
        if selection.anchor.story != selection.focus.story {
            return None;
        }
        let anchor_index = *self.paragraph_lookup.get(&selection.anchor.paragraph_id)?;
        let focus_index = *self.paragraph_lookup.get(&selection.focus.paragraph_id)?;
        let anchor_paragraph = self.paragraphs.get(anchor_index)?;
        let focus_paragraph = self.paragraphs.get(focus_index)?;
        if anchor_paragraph.story != selection.anchor.story
            || focus_paragraph.story != selection.focus.story
            || selection.anchor.story != selection.focus.story
        {
            return None;
        }
        if !Self::valid_text_offset(anchor_paragraph, selection.anchor.offset)
            || !Self::valid_text_offset(focus_paragraph, selection.focus.offset)
        {
            return None;
        }
        if selection.anchor == selection.focus {
            return Some(Vec::new());
        }
        let (start, start_paragraph, end, end_paragraph) =
            if (anchor_index, selection.anchor.offset) <= (focus_index, selection.focus.offset) {
                (
                    &selection.anchor,
                    anchor_index,
                    &selection.focus,
                    focus_index,
                )
            } else {
                (
                    &selection.focus,
                    focus_index,
                    &selection.anchor,
                    anchor_index,
                )
            };
        self.text_range_geometry(
            ParagraphTextRange {
                start_index: start_paragraph,
                start_offset: start.offset,
                end_index: end_paragraph,
                end_offset: end.offset,
            },
            composition,
            Some(&start.story),
            &[],
        )
    }

    pub(super) fn valid_text_offset(paragraph: &LaidOutParagraph, offset: usize) -> bool {
        offset <= paragraph.text.len() && paragraph.text.is_char_boundary(offset)
    }

    pub(super) fn text_range_geometry(
        &self,
        range: ParagraphTextRange,
        composition: bool,
        story: Option<&StoryId>,
        excluded_environment_ids: &[NodeId],
    ) -> Option<Vec<PageSelectionRect>> {
        let mut rectangles = Vec::new();
        for paragraph_index in range.start_index..=range.end_index {
            let paragraph = self.paragraphs.get(paragraph_index)?;
            if story.is_some_and(|story| &paragraph.story != story)
                || paragraph
                    .container
                    .environment_ids()
                    .iter()
                    .any(|id| excluded_environment_ids.contains(id))
            {
                continue;
            }
            let range_start = if paragraph_index == range.start_index {
                range.start_offset
            } else {
                0
            };
            let range_end = if paragraph_index == range.end_index {
                range.end_offset
            } else {
                paragraph.text.len()
            };
            if range_start >= range_end {
                continue;
            }
            let line_rectangles = if composition {
                paragraph
                    .layout
                    .composition_text_geometry(&paragraph.text, range_start..range_end)?
            } else {
                paragraph
                    .layout
                    .selection_text_geometry(&paragraph.text, range_start..range_end)?
            };
            for (bounds, line_index) in line_rectangles {
                let line = paragraph.layout.paragraph.lines.get(line_index)?;
                let location = paragraph.line_locations.get(line_index)?.as_ref()?;
                let page = self.pages.get(location.page_index)?;
                let (y0, y1) = if composition {
                    (
                        (location.y0_pt + bounds.y0).max(0.0),
                        (location.y0_pt + bounds.y1).min(page.height_pt),
                    )
                } else {
                    (
                        location.y0_pt,
                        location.y0_pt + f64::from(line.metrics.height_pt),
                    )
                };
                let mut rectangle = PageSelectionRect {
                    page_index: location.page_index,
                    x0_pt: bounds.x0 + location.x0_pt,
                    y0_pt: y0,
                    x1_pt: bounds.x1 + location.x0_pt,
                    y1_pt: y1,
                };
                if let Some(clip) = location.clip
                    && !clip.clamp_rect(&mut rectangle)
                {
                    continue;
                }
                // Qt 快照校验拒绝零宽几何；退化行内对象不应让整份选区投影失败。
                if rectangle.x1_pt <= rectangle.x0_pt || rectangle.y1_pt <= rectangle.y0_pt {
                    continue;
                }
                rectangles.push(rectangle);
            }
        }
        Some(rectangles)
    }
}
