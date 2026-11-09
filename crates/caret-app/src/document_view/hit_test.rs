//! 提供 caret 几何、字形栅格化和页面命中接口。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{DocumentViewRuntime, snapshot::caret_geometry_with_environment_exit};
use caret_engine::{PageCaretView, RasterizedPageGlyphs, Selection};

impl DocumentViewRuntime {
    /// 返回当前 Rust 光标位置在最新布局中的页面几何。
    pub fn current_caret_json(&mut self) -> Result<String, String> {
        let (position, visual_affinity, block_environment_exit, math_context, math_tree_context) = {
            let display_session = self
                .composition_preview_session
                .as_ref()
                .unwrap_or(&self.session);
            (
                display_session.selection().focus.clone(),
                display_session.selection_visual_affinity(),
                display_session.block_environment_exit(),
                display_session.math_text_input_context(),
                display_session.math_tree_input_context(),
            )
        };
        let snapshot = self.current_snapshot()?;
        let caret = if let Some(context) = math_context {
            snapshot
                .math_text_caret_geometry(&context.target, &context.focus, context.visual_affinity)
                .map(PageCaretView::from)
        } else if let Some((target, position)) = math_tree_context {
            snapshot
                .math_tree_caret(&target, &position)
                .map(|(caret, _)| PageCaretView::from(caret))
        } else {
            snapshot
                .caret_geometry(&position, visual_affinity)
                .map(|caret| {
                    PageCaretView::from(caret_geometry_with_environment_exit(
                        block_environment_exit,
                        snapshot,
                        caret,
                    ))
                })
        };
        serde_json::to_string(&caret).map_err(|error| format!("layout.caret_serialize: {error}"))
    }

    /// 栅格化指定页；调用方只请求当前可见页，并提供页面原点的设备像素坐标。
    pub fn rasterize_page_glyphs(
        &mut self,
        page_index: usize,
        px_per_pt: f64,
        page_origin_device_px: [f64; 2],
        default_foreground: [u8; 4],
    ) -> Result<RasterizedPageGlyphs, String> {
        let page = self
            .current_snapshot()?
            .page_glyphs(page_index)
            .map_err(|error| error.to_string())?;
        self.glyph_rasterizer
            .rasterize(&page, px_per_pt, page_origin_device_px, default_foreground)
            .map_err(|error| error.to_string())
    }

    /// 在当前已发布或惰性重建的快照中命中，并返回稳定的 Rust 文档位置。
    pub fn hit_test_json(
        &mut self,
        page_index: i32,
        x_pt: f64,
        y_pt: f64,
    ) -> Result<String, String> {
        let hit = match usize::try_from(page_index) {
            Ok(page_index) => self
                .current_snapshot()?
                .hit_test_view(page_index, x_pt, y_pt),
            Err(_) => None,
        };
        serde_json::to_string(&hit).map_err(|error| format!("layout.hit_test_serialize: {error}"))
    }

    /// 在 Rust 会话中提交页面命中的插入点，并返回提交位置与输入代际。
    pub fn place_caret_at_page_json(
        &mut self,
        page_index: i32,
        x_pt: f64,
        y_pt: f64,
    ) -> Result<String, String> {
        let page_index = match usize::try_from(page_index) {
            Ok(page_index) => page_index,
            Err(_) => return Ok("null".to_owned()),
        };
        if self.session.composition().is_none() {
            let (math_hit, math_text_hit_loses_to_body, structure_hit) = {
                let snapshot = self.current_snapshot()?;
                let math_hit = snapshot.hit_test_math_text(page_index, x_pt, y_pt);
                let math_text_wins = math_hit.as_ref().is_some_and(|math_hit| {
                    math_text_hit_wins_over_body(snapshot, page_index, x_pt, y_pt, math_hit)
                });
                let structure_hit = math_hit
                    .is_none()
                    .then(|| snapshot.hit_test_math_structure(page_index, x_pt, y_pt))
                    .flatten();
                (math_hit, !math_text_wins, structure_hit)
            };
            if let Some(hit) =
                math_hit.filter(|hit| !hit.is_zero_width || !math_text_hit_loses_to_body)
            {
                let font_generation = self.layout.font_generation();
                let effect = self
                    .session
                    .focus_math_text_hit(&hit, font_generation)
                    .map_err(|error| format!("selection.math_text_focus: {error}"))?;
                self.preferred_visual_x_pt = None;
                let mut result = serde_json::json!({
                    "valid": true,
                    "mathText": true,
                    "documentGeneration": hit.document_generation.to_string(),
                    "caret": PageCaretView::from(hit.caret),
                });
                if effect.composition_changed
                    && let Some(object) = result.as_object_mut()
                {
                    object.insert(
                        "compositionCancelled".to_owned(),
                        serde_json::Value::Bool(true),
                    );
                }
                return serde_json::to_string(&result)
                    .map_err(|error| format!("layout.place_caret_serialize: {error}"));
            }
            if let Some(hit) = structure_hit {
                let font_generation = self.layout.font_generation();
                self.session
                    .focus_math_structure_hit(&hit, font_generation)
                    .map_err(|error| format!("selection.math_structure_focus: {error}"))?;
                self.preferred_visual_x_pt = None;
                let result = serde_json::json!({
                    "valid": true,
                    "mathTree": true,
                    "documentGeneration": hit.document_generation.to_string(),
                    "caret": PageCaretView::from(hit.caret),
                });
                return serde_json::to_string(&result)
                    .map_err(|error| format!("layout.place_caret_serialize: {error}"));
            }
        }
        let (hit, result) = {
            let snapshot = self.current_snapshot()?;
            let Some(hit) = snapshot.hit_test(page_index, x_pt, y_pt) else {
                return Ok("null".to_owned());
            };
            (hit.clone(), snapshot.view_for_hit(&hit))
        };
        let position = hit.position;
        let effect = self
            .session
            .set_selection_with_visual_affinity(
                Some(Selection {
                    anchor: position.clone(),
                    focus: position,
                }),
                hit.visual_affinity,
            )
            .map_err(|error| format!("selection.set: {error}"))?;
        self.preferred_visual_x_pt = None;
        if effect.composition_changed {
            self.composition_id = None;
            self.composition_preview_session = None;
            self.composition_snapshot = None;
            self.composition_range = None;
        }
        let mut result = serde_json::to_value(result)
            .map_err(|error| format!("layout.place_caret_serialize: {error}"))?;
        if effect.composition_changed
            && let Some(object) = result.as_object_mut()
        {
            object.insert(
                "compositionCancelled".to_owned(),
                serde_json::Value::Bool(true),
            );
        }
        serde_json::to_string(&result)
            .map_err(|error| format!("layout.place_caret_serialize: {error}"))
    }
}

pub(super) fn math_text_hit_wins_over_body(
    snapshot: &caret_engine::DocumentLayoutSnapshot,
    page_index: usize,
    x_pt: f64,
    y_pt: f64,
    math_hit: &caret_engine::PageMathTextHit,
) -> bool {
    !math_hit.is_zero_width
        || !snapshot
            .hit_test(page_index, x_pt, y_pt)
            .as_ref()
            .is_some_and(|hit| body_text_contains_point(snapshot, hit, page_index, x_pt, y_pt))
}

fn body_text_contains_point(
    snapshot: &caret_engine::DocumentLayoutSnapshot,
    hit: &caret_engine::PageHit,
    page_index: usize,
    x_pt: f64,
    y_pt: f64,
) -> bool {
    let Some(paragraph) = snapshot.paragraph_by_id(&hit.position.paragraph_id) else {
        return false;
    };
    let boundaries = paragraph_text_boundaries(paragraph);
    let offset_index = match boundaries.binary_search(&hit.position.offset) {
        Ok(index) => index,
        Err(_) => return false,
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
            anchor: caret_engine::Pos {
                offset: start,
                affinity: caret_engine::Affinity::Downstream,
                ..hit.position.clone()
            },
            focus: caret_engine::Pos {
                offset: end,
                affinity: caret_engine::Affinity::Upstream,
                ..hit.position.clone()
            },
        };
        snapshot
            .selection_geometry(&selection)
            .is_some_and(|rectangles| {
                rectangles.into_iter().any(|rectangle| {
                    // 正文保留每个正宽字素的内部区域，重合边界留给零宽公式 caret。
                    let inset = 1.5_f64.min((rectangle.x1_pt - rectangle.x0_pt).max(0.0) * 0.25);
                    rectangle.page_index == page_index
                        && x_pt > rectangle.x0_pt + inset
                        && x_pt < rectangle.x1_pt - inset
                        && y_pt >= rectangle.y0_pt
                        && y_pt <= rectangle.y1_pt
                })
            })
    })
}

fn paragraph_text_boundaries(paragraph: &caret_engine::LaidOutParagraph) -> Vec<usize> {
    paragraph.layout.grapheme_boundaries.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use caret_doc::{MathFlow, MathTree, Run, TextFlow};
    use std::collections::BTreeMap;

    fn pixel_centers_in_pt_range(
        start_pt: f64,
        end_pt: f64,
        pt_per_pixel: f64,
        page_origin_pt: f64,
    ) -> Vec<f64> {
        let first_pixel = ((start_pt - page_origin_pt) / pt_per_pixel).floor() as i64;
        let last_pixel = ((end_pt - page_origin_pt) / pt_per_pixel).ceil() as i64;
        (first_pixel..=last_pixel)
            .map(|pixel| page_origin_pt + (pixel as f64 + 0.5) * pt_per_pixel)
            .filter(|center| start_pt <= *center && *center <= end_pt)
            .collect()
    }

    #[test]
    fn body_hit_keeps_following_text_after_atomic_inline_math_and_combining_mark()
    -> Result<(), String> {
        let mut document = caret_doc::Document::blank();
        let paragraph_id = match &mut document.body[0] {
            caret_doc::Block::Paragraph(paragraph) => {
                let paragraph_id = paragraph.id.clone();
                paragraph.inlines = vec![
                    caret_doc::ParagraphContent::Text(TextFlow {
                        runs: vec![Run {
                            text: "a".to_owned(),
                            language: None,
                            props: BTreeMap::new(),
                            extra: BTreeMap::new(),
                        }],
                        extra: BTreeMap::new(),
                    }),
                    caret_doc::ParagraphContent::Math(caret_doc::InlineMath {
                        id: caret_doc::NodeId::generate(),
                        math_tree: MathTree {
                            schema_version: caret_doc::MATH_TREE_SCHEMA_VERSION,
                            root: MathFlow {
                                id: caret_doc::NodeId::generate(),
                                content: vec![caret_doc::MathContent::Math(caret_doc::MathNode {
                                    id: caret_doc::NodeId::generate(),
                                    kind: caret_doc::MathNodeKind::Group,
                                    text: None,
                                    attributes: BTreeMap::new(),
                                    children: vec![MathFlow {
                                        id: caret_doc::NodeId::generate(),
                                        content: vec![
                                            caret_doc::MathContent::Text(TextFlow {
                                                runs: Vec::new(),
                                                extra: BTreeMap::new(),
                                            }),
                                            caret_doc::MathContent::Text(TextFlow {
                                                runs: vec![Run {
                                                    text: "\u{200b}".to_owned(),
                                                    language: None,
                                                    props: BTreeMap::new(),
                                                    extra: BTreeMap::new(),
                                                }],
                                                extra: BTreeMap::new(),
                                            }),
                                        ],
                                        extra: BTreeMap::new(),
                                    }],
                                    extra: BTreeMap::new(),
                                })],
                                extra: BTreeMap::new(),
                            },
                            extra: BTreeMap::new(),
                        },
                        extra: BTreeMap::new(),
                    }),
                    caret_doc::ParagraphContent::Text(TextFlow {
                        runs: vec![Run {
                            text: "\u{0301}a".to_owned(),
                            language: None,
                            props: BTreeMap::new(),
                            extra: BTreeMap::new(),
                        }],
                        extra: BTreeMap::new(),
                    }),
                ];
                paragraph_id
            }
            _ => return Err("blank document should start with a paragraph".to_owned()),
        };
        let mut runtime = DocumentViewRuntime::new(document).map_err(|error| error.to_string())?;
        let snapshot = runtime
            .current_snapshot()
            .map_err(|error| error.to_string())?
            .clone();
        let paragraph = snapshot
            .paragraph_by_id(&paragraph_id)
            .ok_or_else(|| "paragraph layout is missing".to_owned())?;
        assert_eq!(paragraph.inline_math_source_ranges.len(), 1);
        assert_eq!(paragraph.inline_math_source_ranges[0], 1..4);
        assert_eq!(paragraph.inline_math_layouts[0].width, 0.0);
        assert!(paragraph_text_boundaries(paragraph).contains(&4));
        let inline_start = paragraph
            .layout
            .cursor_at(1, caret_engine::VisualAffinity::Downstream)
            .and_then(|cursor| paragraph.layout.cursor_geometry_with_line(cursor, 0.0))
            .ok_or_else(|| "empty formula start geometry is missing".to_owned())?;
        let inline_end = paragraph
            .layout
            .cursor_at(4, caret_engine::VisualAffinity::Upstream)
            .and_then(|cursor| paragraph.layout.cursor_geometry_with_line(cursor, 0.0))
            .ok_or_else(|| "empty formula end geometry is missing".to_owned())?;
        assert_eq!(inline_end.0.x0 - inline_start.0.x0, 1.5);
        let selection = Selection {
            anchor: caret_engine::Pos {
                story: caret_engine::StoryId::Body,
                paragraph_id: paragraph_id.clone(),
                offset: 6,
                affinity: caret_engine::Affinity::Downstream,
            },
            focus: caret_engine::Pos {
                story: caret_engine::StoryId::Body,
                paragraph_id: paragraph_id.clone(),
                offset: 7,
                affinity: caret_engine::Affinity::Upstream,
            },
        };
        let rectangle = snapshot
            .selection_geometry(&selection)
            .ok_or_else(|| "combining text selection geometry is missing".to_owned())?
            .into_iter()
            .next()
            .ok_or_else(|| "combining text has no visible geometry".to_owned())?;
        let hit = caret_engine::PageHit {
            position: caret_engine::Pos {
                story: caret_engine::StoryId::Body,
                paragraph_id,
                offset: 6,
                affinity: caret_engine::Affinity::Downstream,
            },
            visual_affinity: caret_engine::VisualAffinity::Downstream,
            caret: caret_engine::PageCaretGeometry {
                page_index: rectangle.page_index,
                x_pt: (rectangle.x0_pt + rectangle.x1_pt) / 2.0,
                y0_pt: rectangle.y0_pt,
                y1_pt: rectangle.y1_pt,
            },
        };
        assert!(body_text_contains_point(
            &snapshot,
            &hit,
            rectangle.page_index,
            hit.caret.x_pt,
            (rectangle.y0_pt + rectangle.y1_pt) / 2.0,
        ));
        for page_origin_pt in [0.0, 0.375, 0.75, 1.125] {
            let pt_per_pixel = 1.5;
            let click = pixel_centers_in_pt_range(
                rectangle.y0_pt,
                rectangle.y1_pt,
                pt_per_pixel,
                page_origin_pt,
            )
            .into_iter()
            .find_map(|y| {
                pixel_centers_in_pt_range(
                    rectangle.x0_pt,
                    rectangle.x1_pt,
                    pt_per_pixel,
                    page_origin_pt,
                )
                .into_iter()
                .find_map(|x| {
                    let body_hit = snapshot.hit_test(rectangle.page_index, x, y)?;
                    body_text_contains_point(&snapshot, &body_hit, rectangle.page_index, x, y)
                        .then_some((x, y))
                })
            })
            .ok_or_else(|| "body text has no 50% zoom pixel center".to_owned())?;
            let placed = runtime
                .place_caret_at_page_json(rectangle.page_index as i32, click.0, click.1)
                .map_err(|error| error.to_string())?;
            let placed: serde_json::Value =
                serde_json::from_str(&placed).map_err(|error| error.to_string())?;
            assert_ne!(placed["mathText"], true);
            let math_click = pixel_centers_in_pt_range(
                rectangle.y0_pt - 4.0,
                rectangle.y1_pt + 4.0,
                pt_per_pixel,
                page_origin_pt,
            )
            .into_iter()
            .find_map(|y| {
                pixel_centers_in_pt_range(
                    rectangle.x0_pt - 8.0,
                    rectangle.x1_pt + 8.0,
                    pt_per_pixel,
                    page_origin_pt,
                )
                .into_iter()
                .find_map(|x| {
                    let body_text_hit =
                        snapshot
                            .hit_test(rectangle.page_index, x, y)
                            .is_some_and(|body_hit| {
                                body_text_contains_point(
                                    &snapshot,
                                    &body_hit,
                                    rectangle.page_index,
                                    x,
                                    y,
                                )
                            });
                    (snapshot
                        .hit_test_math_text(rectangle.page_index, x, y)
                        .is_some_and(|hit| hit.is_zero_width)
                        && !body_text_hit)
                        .then_some((x, y))
                })
            })
            .ok_or_else(|| "empty formula caret has no 50% zoom pixel center".to_owned())?;
            let placed = runtime
                .place_caret_at_page_json(rectangle.page_index as i32, math_click.0, math_click.1)
                .map_err(|error| error.to_string())?;
            let placed: serde_json::Value =
                serde_json::from_str(&placed).map_err(|error| error.to_string())?;
            assert_eq!(placed["mathText"], true);
        }
        Ok(())
    }
}
