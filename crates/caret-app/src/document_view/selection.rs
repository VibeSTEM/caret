//! 生成拖选命中映射并提交、清除或提取 Rust 文档选区。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::DocumentViewRuntime;
use caret_engine::{
    Affinity, MathEditTarget, MathPositionAffinity, MathTextFlowPathStep, MathTextPosition, Pos,
    Selection, VisualAffinity, utf16_offset_to_utf8,
};
use std::{collections::HashMap, convert::TryFrom};
use unicode_segmentation::UnicodeSegmentation;

type MathTextDragIsland = (MathEditTarget, Vec<(usize, MathTextPosition)>);

impl DocumentViewRuntime {
    /// 返回按正文点击规则裁决后的拖选起点域，不修改当前会话状态。
    pub fn drag_start_domain_json(
        &mut self,
        page_index: i32,
        x_pt: f64,
        y_pt: f64,
    ) -> Result<String, String> {
        let page_index =
            usize::try_from(page_index).map_err(|_| "selection.invalid_page_index".to_owned())?;
        let snapshot = self.current_snapshot()?;
        let domain_id = match snapshot.hit_test_math_text(page_index, x_pt, y_pt) {
            Some(hit)
                if super::hit_test::math_text_hit_wins_over_body(
                    snapshot, page_index, x_pt, y_pt, &hit,
                ) =>
            {
                snapshot
                    .math_text_drag_stops(page_index)
                    .into_iter()
                    .find(|stop| stop.target == hit.target && stop.token == hit.token)
                    .map(|stop| math_text_domain_id(&stop.target, stop.token))
            }
            Some(_) => snapshot
                .hit_test(page_index, x_pt, y_pt)
                .map(|_| "body".to_owned()),
            None if snapshot
                .hit_test_math_structure(page_index, x_pt, y_pt)
                .is_some() =>
            {
                None
            }
            None => snapshot
                .hit_test(page_index, x_pt, y_pt)
                .map(|_| "body".to_owned()),
        };
        serde_json::to_string(&serde_json::json!({"domainId": domain_id.unwrap_or_default()}))
            .map_err(|error| format!("selection.drag_start_domain_serialize: {error}"))
    }

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
                                "domainId": "body",
                                "pageIndex": page_index,
                                "paragraphIndex": paragraph_order,
                                "startUtf8": start.to_string(),
                                "endUtf8": end.to_string(),
                                "startPosition": {
                                    "kind": "body",
                                    "paragraphIndex": paragraph_order,
                                    "offsetUtf8": start.to_string(),
                                    "orderUtf8": start.to_string(),
                                },
                                "endPosition": {
                                    "kind": "body",
                                    "paragraphIndex": paragraph_order,
                                    "offsetUtf8": end.to_string(),
                                    "orderUtf8": end.to_string(),
                                },
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
                            "domainId": "body",
                            "pageIndex": page_index,
                            "xPt": caret.x_pt,
                            "y0Pt": caret.y0_pt,
                            "y1Pt": caret.y1_pt,
                            "position": {
                                "kind": "body",
                                "domainId": "body",
                                "paragraphId": paragraph.id.to_string(),
                                "paragraphIndex": paragraph_order,
                                "offsetUtf8": offset.to_string(),
                                "orderUtf8": offset.to_string(),
                                "affinity": if logical_affinity == Affinity::Upstream { "upstream" } else { "downstream" },
                            },
                            "visualAffinity": if visual_affinity == VisualAffinity::Upstream { "upstream" } else { "downstream" },
                        }));
                    }
                }
            }
        }
        let math_stops = snapshot.math_text_drag_stops(page_index);
        let mut math_islands: HashMap<String, MathTextDragIsland> = HashMap::new();
        for stop in math_stops {
            let domain_id = math_text_domain_id(&stop.target, stop.token);
            let island = math_islands
                .entry(domain_id)
                .or_insert_with(|| (stop.target.clone(), Vec::new()));
            island.1.push((stop.order_utf8, stop.position.clone()));
            stops.push(serde_json::json!({
                "domainId": math_text_domain_id(&stop.target, stop.token),
                "pageIndex": page_index,
                "xPt": stop.caret.x_pt,
                "y0Pt": stop.caret.y0_pt,
                "y1Pt": stop.caret.y1_pt,
                "position": {
                    "kind": "math",
                    "sessionId": snapshot.session_id.as_ref().map(ToString::to_string),
                    "domainId": math_text_domain_id(&stop.target, stop.token),
                    "pageIndex": page_index,
                    "xPt": stop.caret.x_pt,
                    "yPt": (stop.caret.y0_pt + stop.caret.y1_pt) / 2.0,
                    "documentGeneration": snapshot.document_generation.to_string(),
                    "fontGeneration": snapshot.font_generation.to_string(),
                    "token": stop.token,
                    "orderUtf8": stop.order_utf8.to_string(),
                    "offsetUtf8": stop.position.offset.to_string(),
                    "affinity": math_affinity_name(stop.position.affinity),
                    "visualAffinity": visual_affinity_name(stop.visual_affinity),
                    "target": math_target_json(&stop.target),
                    "alignCell": stop.position.path.align_cell(),
                    "path": stop.position.path.steps().iter().map(math_path_step_json).collect::<Vec<_>>(),
                },
                "visualAffinity": visual_affinity_name(stop.visual_affinity),
            }));
        }
        for (domain_id, (target, mut points)) in math_islands {
            points.sort_unstable_by_key(|(offset, _)| *offset);
            points.dedup_by_key(|(offset, _)| *offset);
            for pair in points.windows(2) {
                let (start_offset, start_position) = &pair[0];
                let (end_offset, end_position) = &pair[1];
                if start_offset == end_offset {
                    continue;
                }
                let start = MathTextPosition::new(
                    start_position.path.clone(),
                    start_position.offset,
                    MathPositionAffinity::Downstream,
                );
                let end = MathTextPosition::new(
                    end_position.path.clone(),
                    end_position.offset,
                    MathPositionAffinity::Upstream,
                );
                let Some(rectangles) = snapshot.math_text_selection_geometry(&target, &start, &end)
                else {
                    continue;
                };
                for rectangle in rectangles {
                    if rectangle.page_index != page_index {
                        continue;
                    }
                    segments.push(serde_json::json!({
                        "domainId": domain_id,
                        "pageIndex": page_index,
                        "startUtf8": start_offset.to_string(),
                        "endUtf8": end_offset.to_string(),
                        "startPosition": {
                            "kind": "math",
                            "orderUtf8": start_offset.to_string(),
                        },
                        "endPosition": {
                            "kind": "math",
                            "orderUtf8": end_offset.to_string(),
                        },
                        "x0Pt": rectangle.x0_pt,
                        "y0Pt": rectangle.y0_pt,
                        "x1Pt": rectangle.x1_pt,
                        "y1Pt": rectangle.y1_pt,
                    }));
                }
            }
        }
        serde_json::to_string(&serde_json::json!({
            "documentGeneration": snapshot.document_generation.to_string(),
            "fontGeneration": snapshot.font_generation.to_string(),
            "pageIndex": page_index,
            "stops": stops,
            "segments": segments,
        }))
        .map_err(|error| format!("selection.hit_map_serialize: {error}"))
    }

    /// 将 Qt 拖选释放时提交的两个稳定文本端点设置为 Rust 选区。
    pub fn set_selection_json(&mut self, selection_json: &str) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct PositionDto {
            kind: Option<String>,
            domain_id: Option<String>,
            paragraph_id: Option<String>,
            offset_utf8: Option<String>,
            order_utf8: Option<String>,
            affinity: Option<String>,
            visual_affinity: Option<String>,
            page_index: Option<usize>,
            document_generation: Option<String>,
            font_generation: Option<String>,
            session_id: Option<String>,
            token: Option<u64>,
            x_pt: Option<f64>,
            y_pt: Option<f64>,
            target: Option<MathTargetDto>,
            align_cell: Option<Vec<usize>>,
            path: Option<Vec<MathPathStepDto>>,
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct MathTargetDto {
            kind: String,
            paragraph_id: Option<String>,
            math_id: Option<String>,
            block_id: Option<String>,
            align: Option<bool>,
        }
        #[derive(serde::Deserialize)]
        struct MathPathStepDto {
            kind: String,
            index: usize,
        }
        #[derive(serde::Deserialize)]
        struct SelectionDto {
            anchor: PositionDto,
            focus: PositionDto,
        }
        struct MathEndpoint {
            domain_id: String,
            page_index: usize,
            document_generation: u64,
            font_generation: u64,
            session_id: String,
            token: u64,
            target: MathEditTarget,
            position: MathTextPosition,
            order_utf8: usize,
            visual_affinity: VisualAffinity,
            x_pt: f64,
            y_pt: f64,
        }
        enum Endpoint {
            Body(Pos, VisualAffinity),
            Math(MathEndpoint),
        }
        fn parse_affinity(value: Option<String>) -> Result<Affinity, String> {
            match value.as_deref() {
                Some("upstream") => Ok(Affinity::Upstream),
                Some("downstream") => Ok(Affinity::Downstream),
                _ => Err("selection.invalid_affinity".to_owned()),
            }
        }
        fn parse_math_affinity(value: Option<String>) -> Result<MathPositionAffinity, String> {
            match value.as_deref() {
                Some("upstream") => Ok(MathPositionAffinity::Upstream),
                Some("downstream") => Ok(MathPositionAffinity::Downstream),
                _ => Err("selection.invalid_affinity".to_owned()),
            }
        }
        fn parse_visual_affinity(value: Option<String>) -> Result<VisualAffinity, String> {
            match value.as_deref() {
                Some("upstream") => Ok(VisualAffinity::Upstream),
                Some("downstream") => Ok(VisualAffinity::Downstream),
                _ => Err("selection.invalid_visual_affinity".to_owned()),
            }
        }
        fn parse_endpoint(
            session: &caret_engine::Session,
            dto: PositionDto,
        ) -> Result<Endpoint, String> {
            if dto.kind.as_deref() != Some("math") {
                let paragraph_id = caret_doc::NodeId::parse(
                    dto.paragraph_id
                        .ok_or_else(|| "selection.invalid_paragraph_id".to_owned())?,
                )
                .map_err(|error| format!("selection.invalid_paragraph_id: {error}"))?;
                let offset = dto
                    .offset_utf8
                    .ok_or_else(|| "selection.invalid_utf8_offset".to_owned())?
                    .parse::<usize>()
                    .map_err(|_| "selection.invalid_utf8_offset".to_owned())?;
                let affinity = parse_affinity(dto.affinity)?;
                let visual_affinity = parse_visual_affinity(dto.visual_affinity)?;
                let story = session
                    .story_for_paragraph(&paragraph_id)
                    .ok_or_else(|| "selection.unknown_paragraph".to_owned())?;
                return Ok(Endpoint::Body(
                    Pos {
                        story,
                        paragraph_id,
                        offset,
                        affinity,
                    },
                    visual_affinity,
                ));
            }
            let target_dto = dto
                .target
                .ok_or_else(|| "selection.invalid_math_target".to_owned())?;
            let target = match target_dto.kind.as_str() {
                "inline" => MathEditTarget::Inline {
                    paragraph_id: caret_doc::NodeId::parse(
                        target_dto
                            .paragraph_id
                            .ok_or_else(|| "selection.invalid_math_target".to_owned())?,
                    )
                    .map_err(|_| "selection.invalid_math_target".to_owned())?,
                    math_id: caret_doc::NodeId::parse(
                        target_dto
                            .math_id
                            .ok_or_else(|| "selection.invalid_math_target".to_owned())?,
                    )
                    .map_err(|_| "selection.invalid_math_target".to_owned())?,
                },
                "block" => MathEditTarget::Block {
                    block_id: caret_doc::NodeId::parse(
                        target_dto
                            .block_id
                            .ok_or_else(|| "selection.invalid_math_target".to_owned())?,
                    )
                    .map_err(|_| "selection.invalid_math_target".to_owned())?,
                    align: target_dto.align.unwrap_or(false),
                },
                _ => return Err("selection.invalid_math_target".to_owned()),
            };
            let token = dto
                .token
                .ok_or_else(|| "selection.invalid_math_token".to_owned())?;
            let order_utf8 = dto
                .order_utf8
                .ok_or_else(|| "selection.invalid_math_offset".to_owned())?
                .parse::<usize>()
                .map_err(|_| "selection.invalid_math_offset".to_owned())?;
            let offset = dto
                .offset_utf8
                .ok_or_else(|| "selection.invalid_utf8_offset".to_owned())?
                .parse::<usize>()
                .map_err(|_| "selection.invalid_utf8_offset".to_owned())?;
            let path_steps = dto
                .path
                .ok_or_else(|| "selection.invalid_math_path".to_owned())?
                .into_iter()
                .map(|step| match step.kind.as_str() {
                    "content" => Ok(MathTextFlowPathStep::Content(step.index)),
                    "environmentChild" => Ok(MathTextFlowPathStep::EnvironmentChild(step.index)),
                    "operand" => Ok(MathTextFlowPathStep::Operand(step.index)),
                    _ => Err("selection.invalid_math_path".to_owned()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let align_cell = dto
                .align_cell
                .map(|indices| {
                    let [row, cell] = indices.as_slice() else {
                        return Err("selection.invalid_math_path".to_owned());
                    };
                    Ok((*row, *cell))
                })
                .transpose()?;
            let position = MathTextPosition::new(
                caret_engine::MathTextFlowPath::from_interaction_path(path_steps, align_cell),
                offset,
                parse_math_affinity(dto.affinity)?,
            );
            let visual_affinity = parse_visual_affinity(dto.visual_affinity)?;
            let page_index = dto
                .page_index
                .ok_or_else(|| "selection.invalid_page_index".to_owned())?;
            let document_generation = dto
                .document_generation
                .ok_or_else(|| "selection.invalid_generation".to_owned())?
                .parse::<u64>()
                .map_err(|_| "selection.invalid_generation".to_owned())?;
            let font_generation = dto
                .font_generation
                .ok_or_else(|| "selection.invalid_generation".to_owned())?
                .parse::<u64>()
                .map_err(|_| "selection.invalid_generation".to_owned())?;
            let session_id = dto
                .session_id
                .ok_or_else(|| "selection.missing_session_id".to_owned())?;
            let domain_id = dto
                .domain_id
                .ok_or_else(|| "selection.invalid_math_domain".to_owned())?;
            if domain_id != math_text_domain_id(&target, token) {
                return Err("selection.invalid_math_domain".to_owned());
            }
            let _ = session;
            Ok(Endpoint::Math(MathEndpoint {
                domain_id,
                page_index,
                document_generation,
                font_generation,
                session_id,
                token,
                target,
                position,
                order_utf8,
                visual_affinity,
                x_pt: dto.x_pt.unwrap_or(0.0),
                y_pt: dto.y_pt.unwrap_or(0.0),
            }))
        }
        let dto: SelectionDto = serde_json::from_str(selection_json)
            .map_err(|error| format!("selection.invalid_payload: {error}"))?;
        let anchor = parse_endpoint(&self.session, dto.anchor)?;
        let focus = parse_endpoint(&self.session, dto.focus)?;
        match (anchor, focus) {
            (Endpoint::Body(anchor, anchor_visual), Endpoint::Body(focus, focus_visual)) => {
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
            }
            (Endpoint::Math(anchor), Endpoint::Math(focus)) => {
                if anchor.domain_id != focus.domain_id
                    || anchor.target != focus.target
                    || anchor.token != focus.token
                    || anchor.document_generation != focus.document_generation
                    || anchor.font_generation != focus.font_generation
                    || anchor.session_id != focus.session_id
                {
                    return Err("selection.invalid_math_range".to_owned());
                }
                let (anchor_hit, focus_hit) = {
                    let composing = self.composition_preview_session.is_some();
                    let snapshot = if composing {
                        self.base_snapshot()?
                    } else {
                        self.current_snapshot()?
                    };
                    if snapshot.document_generation != anchor.document_generation
                        || snapshot.font_generation != anchor.font_generation
                        || snapshot.session_id.as_ref().map(ToString::to_string)
                            != Some(anchor.session_id.clone())
                    {
                        return Err("selection.stale_math_hit".to_owned());
                    }
                    let resolve = |endpoint: &MathEndpoint| {
                        if composing {
                            let mut original_stops = snapshot
                                .pages
                                .iter()
                                .enumerate()
                                .flat_map(|(page_index, _)| {
                                    snapshot.math_text_drag_stops(page_index)
                                })
                                .filter(|stop| {
                                    stop.target == endpoint.target
                                        && stop.position.path == endpoint.position.path
                                })
                                .collect::<Vec<_>>();
                            original_stops.sort_by(|left, right| {
                                let distance = |stop: &caret_engine::PageMathTextDragStop| {
                                    let vertical = if endpoint.y_pt < stop.caret.y0_pt {
                                        stop.caret.y0_pt - endpoint.y_pt
                                    } else if endpoint.y_pt > stop.caret.y1_pt {
                                        endpoint.y_pt - stop.caret.y1_pt
                                    } else {
                                        0.0
                                    };
                                    (stop.caret.x_pt - endpoint.x_pt).hypot(vertical)
                                };
                                distance(left).total_cmp(&distance(right)).then_with(|| {
                                    left.caret
                                        .page_index
                                        .abs_diff(endpoint.page_index)
                                        .cmp(&right.caret.page_index.abs_diff(endpoint.page_index))
                                })
                            });
                            original_stops.into_iter().next().and_then(|stop| {
                                snapshot.math_text_drag_hit(
                                    stop.caret.page_index,
                                    stop.token,
                                    &stop.target,
                                    &stop.position,
                                    stop.order_utf8,
                                    stop.visual_affinity,
                                )
                            })
                        } else {
                            snapshot.math_text_drag_hit(
                                endpoint.page_index,
                                endpoint.token,
                                &endpoint.target,
                                &endpoint.position,
                                endpoint.order_utf8,
                                endpoint.visual_affinity,
                            )
                        }
                        .ok_or_else(|| "selection.stale_math_hit".to_owned())
                    };
                    let anchor_hit = resolve(&anchor)?;
                    let focus_hit = resolve(&focus)?;
                    (anchor_hit, focus_hit)
                };
                let effect = self
                    .session
                    .set_math_text_selection_from_hits(
                        &anchor_hit,
                        &focus_hit,
                        anchor_hit.font_generation,
                    )
                    .map_err(|error| format!("selection.math_text: {error}"))?;
                self.anchor_visual_affinity = anchor_hit.visual_affinity;
                self.preferred_visual_x_pt = None;
                if effect.composition_changed {
                    self.composition_id = None;
                    self.composition_preview_session = None;
                    self.composition_snapshot = None;
                    self.composition_range = None;
                    return self.layout_snapshot_json_after_composition_cancel();
                }
            }
            _ => return Err("selection.mixed_text_domains".to_owned()),
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
        let text = if let Some(context) = self.session.math_text_input_context() {
            let start = context.anchor_utf16.min(context.focus_utf16);
            let end = context.anchor_utf16.max(context.focus_utf16);
            utf16_range(&context.surrounding_text, start..end)
                .ok_or_else(|| "clipboard.invalid_math_selection".to_owned())?
                .to_owned()
        } else {
            self.session
                .selected_text()
                .map_err(|error| format!("clipboard.selection_text: {error}"))?
        };
        serde_json::to_string(&serde_json::json!({ "text": text }))
            .map_err(|error| format!("clipboard.serialize: {error}"))
    }
}

fn math_text_domain_id(target: &MathEditTarget, token: u64) -> String {
    let target_id = match target {
        MathEditTarget::Inline { math_id, .. } => format!("inline:{math_id}"),
        MathEditTarget::Block { block_id, align } => format!("block:{block_id}:{align}"),
    };
    format!("math:{target_id}:{token}")
}

fn math_target_json(target: &MathEditTarget) -> serde_json::Value {
    match target {
        MathEditTarget::Inline {
            paragraph_id,
            math_id,
        } => serde_json::json!({
            "kind": "inline",
            "paragraphId": paragraph_id.to_string(),
            "mathId": math_id.to_string(),
        }),
        MathEditTarget::Block { block_id, align } => serde_json::json!({
            "kind": "block",
            "blockId": block_id.to_string(),
            "align": align,
        }),
    }
}

fn math_affinity_name(affinity: MathPositionAffinity) -> &'static str {
    match affinity {
        MathPositionAffinity::Upstream => "upstream",
        MathPositionAffinity::Downstream => "downstream",
    }
}

fn visual_affinity_name(affinity: VisualAffinity) -> &'static str {
    match affinity {
        VisualAffinity::Upstream => "upstream",
        VisualAffinity::Downstream => "downstream",
    }
}

fn math_path_step_json(step: &MathTextFlowPathStep) -> serde_json::Value {
    match step {
        MathTextFlowPathStep::Content(index) => {
            serde_json::json!({ "kind": "content", "index": index })
        }
        MathTextFlowPathStep::EnvironmentChild(index) => {
            serde_json::json!({ "kind": "environmentChild", "index": index })
        }
        MathTextFlowPathStep::Operand(index) => {
            serde_json::json!({ "kind": "operand", "index": index })
        }
    }
}

fn utf16_range(text: &str, range: std::ops::Range<usize>) -> Option<&str> {
    if range.start > range.end {
        return None;
    }
    let byte_start = utf16_offset_to_utf8(text, range.start).ok()?;
    let byte_end = utf16_offset_to_utf8(text, range.end).ok()?;
    text.get(byte_start..byte_end)
}
