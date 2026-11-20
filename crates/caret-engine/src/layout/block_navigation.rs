//! 从块外侧的页面光标寻找相邻正文行、显示公式行或块边界。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{DocumentLayoutSnapshot, PageCaretGeometry, math_navigation::vertical_caret_key};
use crate::{Direction, NavigationCandidate, NavigationGeneration};
use caret_doc::NodeId;

enum RowTarget {
    Text,
    BlockEdge(NodeId, Direction),
    Math(NodeId),
}

impl DocumentLayoutSnapshot {
    /// 从真实页面光标沿上下方向查找最近一行，再取该行最靠近页面目标列的停靠点。
    /// 行框与公式片段来自此快照，翻页继续使用同一页面横坐标；没有可见行时返回空。
    pub fn vertical_candidate_from_block(
        &self,
        current: PageCaretGeometry,
        line_delta: isize,
        preferred_x: f32,
        generation: NavigationGeneration,
    ) -> Option<NavigationCandidate> {
        let step = line_delta.signum();
        if step == 0 {
            return None;
        }
        let mut page_index = current.page_index;
        while let Some(page) = self.pages.get(page_index) {
            let mut best = None;
            let mut consider = |caret: PageCaretGeometry, target: RowTarget| {
                let Some(key) = vertical_caret_key(current, caret, step, preferred_x) else {
                    return;
                };
                if best.as_ref().is_none_or(|(previous, _, _)| key < *previous) {
                    best = Some((key, caret, target));
                }
            };
            for fragment in &page.layout.fragments {
                let Some(paragraph) = self.paragraph_by_id(&fragment.paragraph_id) else {
                    continue;
                };
                let mut y = page.margin_top_pt + f64::from(fragment.y_offset_pt);
                for line_index in fragment.line_range.clone() {
                    let line = &paragraph.layout.paragraph.lines[line_index];
                    let next_y = y + f64::from(line.metrics.height_pt);
                    consider(
                        PageCaretGeometry {
                            page_index,
                            x_pt: f64::from(preferred_x),
                            y0_pt: y,
                            y1_pt: next_y,
                        },
                        RowTarget::Text,
                    );
                    y = next_y;
                }
            }
            for projection in self.math_projections_on_page(page_index) {
                if projection.inline_index.is_some() {
                    continue;
                }
                for side in [Direction::Backward, Direction::Forward] {
                    if let Some(caret) = self.math_block_boundary_caret(&projection.math_id, side)
                        && caret.page_index == page_index
                    {
                        consider(
                            caret,
                            RowTarget::BlockEdge(projection.math_id.clone(), side),
                        );
                    }
                }
                let Some((layout, _)) = self.math_projection_items(projection) else {
                    continue;
                };
                // 文档行由格子的根 MathFlow 表示，分子、下标等局部基线只参加公式内导航。
                for node in &layout.nodes {
                    if !node.is_flow || !node.path.is_empty() || node.width <= 0.0 {
                        continue;
                    }
                    let left = projection.object_x_pt + node.x;
                    let x = f64::from(preferred_x).clamp(left, left + node.width);
                    let baseline = projection.baseline_y_pt + node.baseline;
                    if projection.contains_y(baseline) {
                        consider(
                            projection.caret_at_math_baseline(x, baseline),
                            RowTarget::Math(projection.math_id.clone()),
                        );
                    }
                }
            }
            if let Some((_, caret, target)) = best {
                let y = (caret.y0_pt + caret.y1_pt) * 0.5;
                return match target {
                    RowTarget::Text => self.hit_test(page_index, caret.x_pt, y).map(|hit| {
                        NavigationCandidate::Document {
                            focus: hit.position,
                            visual_affinity: hit.visual_affinity,
                            generation,
                        }
                    }),
                    RowTarget::BlockEdge(block_id, side) => Some(NavigationCandidate::BlockEdge {
                        block_id,
                        side,
                        generation,
                    }),
                    RowTarget::Math(id) => {
                        let host = &self.math_host(&id)?.target;
                        if let Some(hit) = self.hit_test_math_text(page_index, caret.x_pt, y)
                            && &hit.target == host
                        {
                            Some(NavigationCandidate::MathTextHit { hit, generation })
                        } else {
                            self.hit_test_math_structure(page_index, caret.x_pt, y)
                                .filter(|hit| &hit.target == host)
                                .map(|hit| NavigationCandidate::MathTreeHit { hit, generation })
                        }
                    }
                };
            }
            page_index = page_index.checked_add_signed(step)?;
        }
        None
    }
}
