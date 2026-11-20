//! 按页面坐标查询正文、公式文本和公式结构命中。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::items::{MathTextItemGeometry, hit_math_text_items, math_text_caret_in_items};
use super::*;

impl DocumentLayoutSnapshot {
    /// 将页面局部的 pt 坐标映射到最近的有效逻辑文本位置。
    pub fn hit_test(&self, page_index: usize, x_pt: f64, y_pt: f64) -> Option<PageHit> {
        if !x_pt.is_finite() || !y_pt.is_finite() {
            return None;
        }
        let page = self.pages.get(page_index)?;
        if x_pt < 0.0 || y_pt < 0.0 || x_pt > page.width_pt || y_pt > page.height_pt {
            return None;
        }
        let content_y = y_pt - page.margin_top_pt;
        let mut closest_line: Option<(f64, usize, usize, f64)> = None;
        for fragment in &page.layout.fragments {
            let Some(&paragraph_index) = self.paragraph_lookup.get(&fragment.paragraph_id) else {
                continue;
            };
            let Some(paragraph) = self.paragraphs.get(paragraph_index) else {
                continue;
            };
            let mut line_top = f64::from(fragment.y_offset_pt);
            for line_index in fragment.line_range.clone() {
                let Some(line) = paragraph.layout.paragraph.lines.get(line_index) else {
                    continue;
                };
                let line_bottom = line_top + f64::from(line.metrics.height_pt);
                let vertical_distance = if content_y < line_top {
                    line_top - content_y
                } else if content_y > line_bottom {
                    content_y - line_bottom
                } else {
                    0.0
                };
                if closest_line
                    .as_ref()
                    .is_none_or(|(distance, _, _, _)| vertical_distance < *distance)
                {
                    closest_line = Some((vertical_distance, paragraph_index, line_index, line_top));
                }
                line_top = line_bottom;
            }
        }
        let (_, paragraph_index, line_index, line_top) = closest_line?;
        let paragraph = self.paragraphs.get(paragraph_index)?;
        let cursor = paragraph
            .layout
            .cursor_from_point_on_line((x_pt - page.margin_left_pt) as f32, line_index)?;
        let cursor_bounds = paragraph.layout.cursor_geometry(cursor.cursor, 0.0);
        let line = paragraph.layout.paragraph.lines.get(line_index)?;
        let caret_y0 = page.margin_top_pt + line_top;
        Some(PageHit {
            position: Pos {
                story: paragraph.story.clone(),
                paragraph_id: paragraph.id.clone(),
                offset: cursor.cursor.byte_index(),
                affinity: match cursor.logical_affinity {
                    caret_layout::HitAffinity::Upstream => Affinity::Upstream,
                    caret_layout::HitAffinity::Downstream => Affinity::Downstream,
                },
            },
            visual_affinity: cursor.cursor.affinity(),
            caret: PageCaretGeometry {
                page_index,
                x_pt: cursor_bounds.x0 + page.margin_left_pt,
                y0_pt: caret_y0,
                y1_pt: caret_y0 + f64::from(line.metrics.height_pt),
            },
        })
    }

    /// 返回行内公式物理左右边缘对应的正文停靠点。
    pub fn inline_math_visual_edge_body_hit(
        &self,
        target: &MathEditTarget,
        position: &MathTextPosition,
        visual_affinity: VisualAffinity,
        forward: bool,
    ) -> Option<PageHit> {
        let caret = self.math_text_caret_geometry(target, position, visual_affinity)?;
        self.inline_math_body_side_hit(target, caret, forward)
    }

    /// 返回行内公式树焦点物理左右对应的正文停靠点。
    pub fn inline_math_tree_visual_edge_body_hit(
        &self,
        target: &MathEditTarget,
        position: &MathTreePosition,
        forward: bool,
    ) -> Option<PageHit> {
        let caret = self.math_tree_caret_geometry(target, position)?;
        self.inline_math_body_side_hit(target, caret, forward)
    }

    fn inline_math_body_side_hit(
        &self,
        target: &MathEditTarget,
        math_caret: PageCaretGeometry,
        forward: bool,
    ) -> Option<PageHit> {
        let MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } = target
        else {
            return None;
        };
        let paragraph = self.paragraph_by_id(paragraph_id)?;
        let index = paragraph
            .inline_math_ids
            .iter()
            .position(|candidate| candidate == math_id)?;
        let source_range = paragraph.inline_math_source_ranges.get(index)?;
        let before = Pos {
            story: paragraph.story.clone(),
            paragraph_id: paragraph_id.clone(),
            offset: source_range.start,
            affinity: Affinity::Downstream,
        };
        let after = Pos {
            story: paragraph.story.clone(),
            paragraph_id: paragraph_id.clone(),
            offset: source_range.end,
            affinity: Affinity::Upstream,
        };
        let before_geometry = self.caret_geometry(&before, VisualAffinity::Downstream)?;
        let after_geometry = self.caret_geometry(&after, VisualAffinity::Upstream)?;
        if before_geometry.page_index != math_caret.page_index
            || after_geometry.page_index != math_caret.page_index
            || (before_geometry.y0_pt - after_geometry.y0_pt).abs() >= 0.5
        {
            return None;
        }
        let (position, visual_affinity, caret) =
            if (before_geometry.x_pt < after_geometry.x_pt) == forward {
                (after, VisualAffinity::Upstream, after_geometry)
            } else {
                (before, VisualAffinity::Downstream, before_geometry)
            };
        Some(PageHit {
            position,
            visual_affinity,
            caret,
        })
    }

    /// 命中公式中的正文文本岛，并从同一快照返回宿主、TextFlow 路径与光标几何。
    pub fn hit_test_math_text(
        &self,
        page_index: usize,
        x_pt: f64,
        y_pt: f64,
    ) -> Option<PageMathTextHit> {
        if !x_pt.is_finite() || !y_pt.is_finite() {
            return None;
        }
        let page = self.pages.get(page_index)?;
        if x_pt < 0.0 || y_pt < 0.0 || x_pt > page.width_pt || y_pt > page.height_pt {
            return None;
        }
        let mut zero_width_hit = None;
        for projection in self.math_projections_on_page(page_index) {
            if !projection.contains_y(y_pt) {
                continue;
            }
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            let geometry = MathTextItemGeometry {
                object_x_pt: projection.object_x_pt,
                object_baseline_y_pt: projection.baseline_y_pt,
                projection,
                items: &layout.items,
                sidecars,
            };
            if let Some(mut hit) = hit_math_text_items(page_index, x_pt, y_pt, &geometry) {
                if !projection.contains_y_for_row(y_pt, math_row_id_for_host(&hit.host)) {
                    continue;
                }
                let Some((normalized_caret, _)) = math_text_caret_in_items(
                    &geometry,
                    &hit.target,
                    &hit.position,
                    hit.visual_affinity,
                ) else {
                    continue;
                };
                let Some(caret) = projection
                    .clip_caret_for_row(normalized_caret, math_row_id_for_host(&hit.host))
                else {
                    continue;
                };
                hit.caret = caret;
                hit.session_id = self.session_id.clone();
                hit.document_generation = self.document_generation;
                hit.font_generation = self.font_generation;
                if hit.is_zero_width {
                    zero_width_hit = Some(hit);
                } else {
                    return Some(hit);
                }
            }
        }
        zero_width_hit
    }

    /// 为通用拖选协议返回本页所有可见数学文本岛的字素停靠点。
    pub fn math_text_drag_stops(&self, page_index: usize) -> Vec<PageMathTextDragStop> {
        let mut stops = Vec::new();
        for projection in self.math_projections_on_page(page_index) {
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            let geometry = MathTextItemGeometry {
                object_x_pt: projection.object_x_pt,
                object_baseline_y_pt: projection.baseline_y_pt,
                projection,
                items: &layout.items,
                sidecars,
            };
            for item in &layout.items {
                let caret_math::layout::MathItem::TextIsland { token, .. } = item else {
                    continue;
                };
                let Some(sidecar) = sidecars.get(token) else {
                    continue;
                };
                for &order_utf8 in &sidecar.layout.grapheme_boundaries {
                    for visual_affinity in [VisualAffinity::Upstream, VisualAffinity::Downstream] {
                        let logical_affinity = sidecar
                            .layout
                            .logical_affinity_at(order_utf8, visual_affinity)
                            .map(|affinity| match affinity {
                                HitAffinity::Upstream => MathPositionAffinity::Upstream,
                                HitAffinity::Downstream => MathPositionAffinity::Downstream,
                            })
                            .unwrap_or(MathPositionAffinity::Downstream);
                        let Some(position) =
                            sidecar.position_at_offset(order_utf8, logical_affinity)
                        else {
                            continue;
                        };
                        let Some((mut caret, _)) = math_text_caret_in_items(
                            &geometry,
                            &sidecar.target,
                            &position,
                            visual_affinity,
                        ) else {
                            continue;
                        };
                        let Some((clip_top, clip_bottom)) = projection.clip_range_for_row(
                            caret.y0_pt,
                            caret.y1_pt,
                            math_row_id_for_host(&sidecar.host),
                        ) else {
                            continue;
                        };
                        caret.y0_pt = clip_top;
                        caret.y1_pt = clip_bottom;
                        stops.push(PageMathTextDragStop {
                            token: *token,
                            order_utf8,
                            target: sidecar.target.clone(),
                            position,
                            visual_affinity,
                            caret,
                        });
                    }
                }
            }
        }
        stops
    }

    /// 为行内公式的根序列及嵌套序列生成可见树停靠点。
    pub fn math_tree_drag_stops(&self, page_index: usize) -> Vec<PageMathTreeDragStop> {
        let mut stops = Vec::new();
        for projection in self.math_projections_on_page(page_index) {
            let Some((layout, _)) = self.math_projection_items(projection) else {
                continue;
            };
            let Some(host) = self.math_host(&projection.math_id) else {
                continue;
            };
            if !matches!(host.target, MathEditTarget::Inline { .. }) {
                continue;
            }
            for node in &layout.nodes {
                let Some((path, sequence_len, _)) = self.math_layout_sequence(projection, node)
                else {
                    continue;
                };
                let Some(scope_host) = host.for_scope(&node.scope_id) else {
                    continue;
                };
                if path == MathSequencePath::root() && sequence_len <= 1 {
                    continue;
                }
                let first_offset = usize::from(path == MathSequencePath::root());
                let last_offset = if path == MathSequencePath::root() {
                    sequence_len.saturating_sub(1)
                } else {
                    sequence_len
                };
                for offset in first_offset..=last_offset {
                    for affinity in [
                        MathPositionAffinity::Upstream,
                        MathPositionAffinity::Downstream,
                    ] {
                        let position = MathTreePosition {
                            host: scope_host.host.clone(),
                            path: path.clone(),
                            offset,
                            affinity,
                        };
                        if stops.iter().any(|stop: &PageMathTreeDragStop| {
                            stop.target == host.target && stop.position == position
                        }) {
                            continue;
                        }
                        let Some((caret, _)) = self.math_tree_caret(&host.target, &position) else {
                            continue;
                        };
                        if caret.page_index != page_index {
                            continue;
                        }
                        stops.push(PageMathTreeDragStop {
                            target: host.target.clone(),
                            position,
                            visual_affinity: match affinity {
                                MathPositionAffinity::Upstream => VisualAffinity::Upstream,
                                MathPositionAffinity::Downstream => VisualAffinity::Downstream,
                            },
                            caret,
                        });
                    }
                }
            }
        }
        stops
    }

    /// 从当前页同一快照重新解析拖选端点，不接受已过期或不匹配的 UI 位置。
    pub fn math_text_drag_hit(
        &self,
        page_index: usize,
        token: u64,
        target: &MathEditTarget,
        position: &MathTextPosition,
        order_utf8: usize,
        visual_affinity: VisualAffinity,
    ) -> Option<PageMathTextHit> {
        for projection in self.math_projections_on_page(page_index) {
            let Some((layout, sidecars)) = self.math_projection_items(projection) else {
                continue;
            };
            let Some(sidecar) = sidecars.get(&token) else {
                continue;
            };
            if &sidecar.target != target || sidecar.offset_at_position(position) != Some(order_utf8)
            {
                continue;
            }
            let geometry = MathTextItemGeometry {
                object_x_pt: projection.object_x_pt,
                object_baseline_y_pt: projection.baseline_y_pt,
                projection,
                items: &layout.items,
                sidecars,
            };
            let (caret, _) =
                math_text_caret_in_items(&geometry, target, position, visual_affinity)?;
            let (y0_pt, y1_pt) = projection.clip_range_for_row(
                caret.y0_pt,
                caret.y1_pt,
                math_row_id_for_host(&sidecar.host),
            )?;
            return Some(PageMathTextHit {
                token,
                session_id: self.session_id.clone(),
                document_generation: self.document_generation,
                font_generation: self.font_generation,
                target: target.clone(),
                host: sidecar.host.clone(),
                position: position.clone(),
                visual_affinity,
                is_zero_width: sidecar.layout.metrics.advance_pt <= 0.0,
                caret: PageCaretGeometry {
                    y0_pt,
                    y1_pt,
                    ..caret
                },
            });
        }
        None
    }

    /// 命中同代布局中的数学树序列，并返回其插入位置。
    pub fn hit_test_math_structure(
        &self,
        page_index: usize,
        x_pt: f64,
        y_pt: f64,
    ) -> Option<PageMathStructureHit> {
        if !x_pt.is_finite() || !y_pt.is_finite() {
            return None;
        }
        let page = self.pages.get(page_index)?;
        if x_pt < 0.0 || y_pt < 0.0 || x_pt > page.width_pt || y_pt > page.height_pt {
            return None;
        }
        // 结构命中的排序键：strict 命中优先，strict 内按面积/深度/距离，loose 命中按距离优先。
        #[derive(Clone, Copy)]
        struct MathStructureHitRank {
            distance_pt: f64,
            strict: bool,
            area_pt2: f64,
            depth: usize,
        }

        impl MathStructureHitRank {
            fn better_than(self, other: Self) -> bool {
                if self.strict != other.strict {
                    return self.strict;
                }
                if self.strict {
                    self.area_pt2 < other.area_pt2
                        || (self.area_pt2 == other.area_pt2
                            && (self.depth > other.depth
                                || (self.depth == other.depth
                                    && self.distance_pt < other.distance_pt)))
                } else {
                    self.distance_pt < other.distance_pt
                        || (self.distance_pt == other.distance_pt
                            && (self.area_pt2 < other.area_pt2
                                || (self.area_pt2 == other.area_pt2 && self.depth > other.depth)))
                }
            }
        }

        let mut best: Option<(MathStructureHitRank, PageMathStructureHit)> = None;
        for projection in self.math_projections_on_page(page_index) {
            if !projection.contains_y(y_pt) {
                continue;
            }
            let Some((layout, _)) = self.math_projection_items(projection) else {
                continue;
            };
            let Some(host) = self.math_host(&projection.math_id) else {
                continue;
            };
            for node in &layout.nodes {
                if node.is_text_flow || (!node.is_flow && !node.is_container) {
                    continue;
                }
                // 零尺寸可选 Placeholder 仅保留路径身份，不代表可见插入槽。
                if node.width <= 0.0 || node.height <= 0.0 {
                    continue;
                }
                let Some(scope_host) = host.for_scope(&node.scope_id) else {
                    continue;
                };
                if !projection.contains_y_for_row(y_pt, math_row_id_for_host(&scope_host.host)) {
                    continue;
                }
                let Some(root) = self.math_scope_root(projection, &node.scope_id) else {
                    continue;
                };
                let (path, contents, environment_children) = if node.is_flow {
                    let Some(flow) = math_flow_at_layout_path(root, &node.path) else {
                        continue;
                    };
                    let Some(path) = math_sequence_path(&node.path, MathSequenceEnd::Content)
                    else {
                        continue;
                    };
                    (path, flow.content.as_slice(), false)
                } else {
                    let Some(environment) = math_environment_at_layout_path(root, &node.path)
                    else {
                        continue;
                    };
                    let Some(path) =
                        math_sequence_path(&node.path, MathSequenceEnd::EnvironmentChildren)
                    else {
                        continue;
                    };
                    (path, environment.children.as_slice(), true)
                };
                // 空行内宿主不占正文推进，命中区收窄为以公式 caret 居中的插入槽；
                // 槽与相邻正文重叠的部分由正文优先判定吸收。
                let empty_inline_host = projection.inline_index.is_some()
                    && node.path.is_empty()
                    && math_flow_is_empty_host(root);
                let (x0_pt, x1_pt, width_pt) = if empty_inline_host {
                    let slot_width = node
                        .font_size_pt
                        .map_or(node.width, |size| (size * 0.5).max(4.0));
                    let center = projection.object_x_pt + node.x;
                    (
                        center - slot_width / 2.0,
                        center + slot_width / 2.0,
                        slot_width,
                    )
                } else {
                    let x0_pt = projection.object_x_pt + node.x;
                    (x0_pt, x0_pt + node.width, node.width)
                };
                let y0_pt = projection.baseline_y_pt + node.y;
                let height_pt = node.height;
                let y1_pt = y0_pt + height_pt;
                let caret_band = projection.caret_at_math_baseline(
                    x0_pt + width_pt / 2.0,
                    projection.baseline_y_pt + node.baseline,
                );
                let hit_y0_pt = y0_pt.min(caret_band.y0_pt);
                let hit_y1_pt = y1_pt.max(caret_band.y1_pt);
                if !x0_pt.is_finite()
                    || !y0_pt.is_finite()
                    || !x1_pt.is_finite()
                    || !y1_pt.is_finite()
                    || x_pt < x0_pt
                    || x_pt > x1_pt
                {
                    continue;
                }
                let strict_hit = y_pt >= y0_pt && y_pt <= y1_pt;
                if !strict_hit && (y_pt < hit_y0_pt || y_pt > hit_y1_pt) {
                    continue;
                }
                if empty_inline_host && self.body_text_contains_point(page_index, x_pt, y_pt) {
                    continue;
                }
                let child_step = |index| {
                    if environment_children {
                        MathPathStep::EnvironmentChild(index)
                    } else {
                        MathPathStep::Content(index)
                    }
                };
                let raw_offset = if contents.is_empty() {
                    0
                } else {
                    contents
                        .iter()
                        .enumerate()
                        .filter_map(|(index, _)| {
                            let mut child_path = node.path.clone();
                            child_path.push(child_step(index));
                            layout
                                .nodes
                                .iter()
                                .find(|child| {
                                    child.scope_id == node.scope_id && child.path == child_path
                                })
                                .filter(|child| {
                                    child.x.is_finite()
                                        && child.width.is_finite()
                                        && child.width > 0.0
                                })
                                .map(|child| (index, child))
                        })
                        .filter(|(_, child)| {
                            x_pt >= projection.object_x_pt + child.x + child.width / 2.0
                        })
                        .map(|(index, _)| index + 1)
                        .max()
                        .unwrap_or(0)
                };
                let mut offset = raw_offset;
                while !environment_children
                    && offset > 0
                    && matches!(
                        contents.get(offset - 1),
                        Some(MathContent::Math(node))
                            if node.kind == caret_doc::MathNodeKind::Placeholder
                    )
                {
                    offset -= 1;
                }
                let position = MathTreePosition {
                    host: scope_host.host,
                    path,
                    offset,
                    affinity: if offset == 0 {
                        MathPositionAffinity::Downstream
                    } else {
                        MathPositionAffinity::Upstream
                    },
                };
                let Some(caret) = self
                    .math_tree_caret(&host.target, &position)
                    .map(|(caret, _)| caret)
                else {
                    continue;
                };
                let hit = PageMathStructureHit {
                    session_id: self.session_id.clone(),
                    document_generation: self.document_generation,
                    font_generation: self.font_generation,
                    target: host.target.clone(),
                    position,
                    caret,
                };
                let rank = MathStructureHitRank {
                    distance_pt: (y_pt - (y0_pt + y1_pt) / 2.0).abs(),
                    strict: strict_hit,
                    area_pt2: width_pt * height_pt,
                    depth: node.path.len(),
                };
                if best
                    .as_ref()
                    .is_none_or(|(best_rank, _)| rank.better_than(*best_rank))
                {
                    best = Some((rank, hit));
                }
            }
        }
        best.map(|(_, hit)| hit)
    }

    /// 判断页面点是否落在正宽正文/文本岛字素内部。
    ///
    /// 正文保留正宽字素内部区域，字素间的重合边界留给零宽公式 caret；空 InlineMath
    /// 的插入槽与相邻文字重叠时据此让正文优先，避免吞掉邻字点击。app 命中裁决也复用
    /// 本查询，保持与公式插入槽相同的内缩规则。
    pub fn body_text_contains_point(&self, page_index: usize, x_pt: f64, y_pt: f64) -> bool {
        let Some(hit) = self.hit_test(page_index, x_pt, y_pt) else {
            return false;
        };
        let Some(paragraph) = self.paragraph_by_id(&hit.position.paragraph_id) else {
            return false;
        };
        let boundaries = &paragraph.layout.grapheme_boundaries;
        let Ok(offset_index) = boundaries.binary_search(&hit.position.offset) else {
            return false;
        };
        let mut candidates = Vec::with_capacity(2);
        if let Some(start_index) = offset_index.checked_sub(1) {
            candidates.push((boundaries[start_index], boundaries[offset_index]));
        }
        if let Some(end) = boundaries.get(offset_index + 1) {
            candidates.push((boundaries[offset_index], *end));
        }
        candidates.into_iter().any(|(start, end)| {
            if start >= end
                || paragraph
                    .inline_math_source_ranges
                    .iter()
                    .any(|range| range.start == start && range.end == end)
            {
                return false;
            }
            let selection = Selection {
                anchor: Pos {
                    offset: start,
                    affinity: Affinity::Downstream,
                    ..hit.position.clone()
                },
                focus: Pos {
                    offset: end,
                    affinity: Affinity::Upstream,
                    ..hit.position.clone()
                },
            };
            self.selection_geometry(&selection)
                .is_some_and(|rectangles| {
                    rectangles.into_iter().any(|rectangle| {
                        let inset =
                            1.5_f64.min((rectangle.x1_pt - rectangle.x0_pt).max(0.0) * 0.25);
                        rectangle.page_index == page_index
                            && x_pt > rectangle.x0_pt + inset
                            && x_pt < rectangle.x1_pt - inset
                            && y_pt >= rectangle.y0_pt
                            && y_pt <= rectangle.y1_pt
                    })
                })
        })
    }
}
