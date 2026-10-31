//! 生成拖选命中映射并提交、清除或提取 Rust 文档选区。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::DocumentViewRuntime;
use caret_engine::{Affinity, Pos, Selection, VisualAffinity};
use std::convert::TryFrom;
use unicode_segmentation::UnicodeSegmentation;

impl DocumentViewRuntime {
    /// 为指针拖选提供单页命中点与字素选择片段缓存。
    pub fn drag_hit_map_json(&mut self, page_index: i32) -> Result<String, String> {
        let page_index =
            usize::try_from(page_index).map_err(|_| "selection.invalid_page_index".to_owned())?;
        let snapshot = self.current_snapshot()?;
        let page = snapshot
            .pages
            .get(page_index)
            .ok_or_else(|| "selection.invalid_page_index".to_owned())?;
        let mut stops = Vec::new();
        let mut segments = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for fragment in &page.layout.fragments {
            let Some((paragraph_order, paragraph)) = snapshot
                .paragraphs
                .iter()
                .enumerate()
                .find(|(_, paragraph)| paragraph.id == fragment.paragraph_id)
            else {
                continue;
            };
            let mut grapheme_offsets = paragraph
                .text
                .grapheme_indices(true)
                .map(|(offset, _)| offset)
                .collect::<Vec<_>>();
            grapheme_offsets.push(paragraph.text.len());
            let mut offsets = Vec::new();
            for line_index in fragment.line_range.clone() {
                let Some(line) = paragraph.layout.paragraph.lines.get(line_index) else {
                    continue;
                };
                let first_boundary =
                    grapheme_offsets.partition_point(|offset| *offset < line.source_bytes.start);
                offsets.extend(
                    grapheme_offsets[first_boundary..]
                        .iter()
                        .copied()
                        .take_while(|offset| *offset <= line.consumed_bytes.end),
                );
                let boundary_end =
                    grapheme_offsets.partition_point(|offset| *offset <= line.source_bytes.end);
                for pair in grapheme_offsets[first_boundary..boundary_end].windows(2) {
                    let start = pair[0];
                    let end = pair[1];
                    let selection = Selection {
                        anchor: Pos {
                            story: paragraph.story.clone(),
                            paragraph_id: paragraph.id.clone(),
                            offset: start,
                            affinity: Affinity::Downstream,
                        },
                        focus: Pos {
                            story: paragraph.story.clone(),
                            paragraph_id: paragraph.id.clone(),
                            offset: end,
                            affinity: Affinity::Downstream,
                        },
                    };
                    if let Some(rectangles) = snapshot.selection_geometry(&selection) {
                        for rectangle in rectangles {
                            if rectangle.page_index != page_index {
                                continue;
                            }
                            segments.push(serde_json::json!({
                                "pageIndex": page_index,
                                "paragraphIndex": paragraph_order,
                                "startUtf8": start.to_string(),
                                "endUtf8": end.to_string(),
                                "x0Pt": rectangle.x0_pt,
                                "y0Pt": rectangle.y0_pt,
                                "x1Pt": rectangle.x1_pt,
                                "y1Pt": rectangle.y1_pt,
                            }));
                        }
                    }
                }
            }
            offsets.sort_unstable();
            offsets.dedup();
            for offset in offsets {
                for logical_affinity in [Affinity::Upstream, Affinity::Downstream] {
                    for visual_affinity in [VisualAffinity::Upstream, VisualAffinity::Downstream] {
                        let position = Pos {
                            story: paragraph.story.clone(),
                            paragraph_id: paragraph.id.clone(),
                            offset,
                            affinity: logical_affinity,
                        };
                        let Some(caret) = snapshot.caret_geometry(&position, visual_affinity)
                        else {
                            continue;
                        };
                        if caret.page_index != page_index {
                            continue;
                        }
                        let identity = (
                            paragraph.id.to_string(),
                            offset,
                            matches!(logical_affinity, Affinity::Upstream),
                            matches!(visual_affinity, VisualAffinity::Upstream),
                        );
                        if !seen.insert(identity) {
                            continue;
                        }
                        stops.push(serde_json::json!({
                            "pageIndex": page_index,
                            "xPt": caret.x_pt,
                            "y0Pt": caret.y0_pt,
                            "y1Pt": caret.y1_pt,
                            "position": {
                                "paragraphId": paragraph.id.to_string(),
                            "paragraphIndex": paragraph_order,
                                "offsetUtf8": offset.to_string(),
                                "affinity": if logical_affinity == Affinity::Upstream { "upstream" } else { "downstream" },
                            },
                            "visualAffinity": if visual_affinity == VisualAffinity::Upstream { "upstream" } else { "downstream" },
                        }));
                    }
                }
            }
        }
        let blocked = snapshot
            .display_math_bounds(page_index)
            .into_iter()
            .map(|[x0_pt, y0_pt, x1_pt, y1_pt]| {
                serde_json::json!({
                    "x0Pt": x0_pt,
                    "y0Pt": y0_pt,
                    "x1Pt": x1_pt,
                    "y1Pt": y1_pt,
                })
            })
            .collect::<Vec<_>>();
        serde_json::to_string(&serde_json::json!({
            "documentGeneration": snapshot.document_generation.to_string(),
            "fontGeneration": snapshot.font_generation.to_string(),
            "pageIndex": page_index,
            "stops": stops,
            "segments": segments,
            "blocked": blocked,
        }))
        .map_err(|error| format!("selection.hit_map_serialize: {error}"))
    }

    /// 将 Qt 拖选释放时提交的两个稳定正文位置设置为 Rust 选区。
    pub fn set_selection_json(&mut self, selection_json: &str) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct PositionDto {
            paragraph_id: String,
            offset_utf8: String,
            affinity: String,
            visual_affinity: String,
        }
        #[derive(serde::Deserialize)]
        struct SelectionDto {
            anchor: PositionDto,
            focus: PositionDto,
        }
        fn to_position(
            session: &caret_engine::Session,
            dto: PositionDto,
        ) -> Result<(Pos, VisualAffinity), String> {
            let paragraph_id = caret_doc::NodeId::parse(dto.paragraph_id)
                .map_err(|error| format!("selection.invalid_paragraph_id: {error}"))?;
            let offset = dto
                .offset_utf8
                .parse::<usize>()
                .map_err(|_| "selection.invalid_utf8_offset".to_owned())?;
            let affinity = match dto.affinity.as_str() {
                "upstream" => Affinity::Upstream,
                "downstream" => Affinity::Downstream,
                _ => return Err("selection.invalid_affinity".to_owned()),
            };
            let visual_affinity = match dto.visual_affinity.as_str() {
                "upstream" => VisualAffinity::Upstream,
                "downstream" => VisualAffinity::Downstream,
                _ => return Err("selection.invalid_visual_affinity".to_owned()),
            };
            let story = session
                .story_for_paragraph(&paragraph_id)
                .ok_or_else(|| "selection.unknown_paragraph".to_owned())?;
            Ok((
                Pos {
                    story,
                    paragraph_id,
                    offset,
                    affinity,
                },
                visual_affinity,
            ))
        }
        let dto: SelectionDto = serde_json::from_str(selection_json)
            .map_err(|error| format!("selection.invalid_payload: {error}"))?;
        let (anchor, anchor_visual) = to_position(&self.session, dto.anchor)?;
        let (focus, focus_visual) = to_position(&self.session, dto.focus)?;
        let effect = self
            .session
            .set_pointer_selection_range(anchor, focus, focus_visual)
            .map_err(|error| format!("selection.set: {error}"))?;
        self.anchor_visual_affinity = anchor_visual;
        self.preferred_visual_x_pt = None;
        if effect.composition_changed {
            self.composition_id = None;
            self.composition_preview_session = None;
            self.composition_snapshot = None;
            self.composition_range = None;
            return self.layout_snapshot_json_after_composition_cancel();
        }
        self.interaction_state_json()
    }

    /// 清除当前选区并保留焦点位置，供画布空白处点击使用。
    pub fn clear_selection_json(&mut self) -> Result<String, String> {
        let visual_affinity = self.session.selection_visual_affinity();
        let effect = self
            .session
            .set_selection_with_visual_affinity(None, visual_affinity)
            .map_err(|error| format!("selection.clear: {error}"))?;
        self.anchor_visual_affinity = visual_affinity;
        self.preferred_visual_x_pt = None;
        if effect.composition_changed {
            self.composition_id = None;
            self.composition_preview_session = None;
            self.composition_snapshot = None;
            self.composition_range = None;
            return self.layout_snapshot_json_after_composition_cancel();
        }
        self.interaction_state_json()
    }

    /// 按文档顺序提取当前选区纯文本，跨段落以换行分隔。
    pub fn selected_text_json(&mut self) -> Result<String, String> {
        let text = self
            .session
            .selected_text()
            .map_err(|error| format!("clipboard.selection_text: {error}"))?;
        serde_json::to_string(&serde_json::json!({ "text": text }))
            .map_err(|error| format!("clipboard.serialize: {error}"))
    }
}
