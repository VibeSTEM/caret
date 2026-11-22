//! 生成拖选命中映射并提交、清除或提取 Rust 文档选区。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{DocumentViewRuntime, snapshot::rectangle_fields};
use caret_engine::{
    Affinity, DocumentLayoutSnapshot, LaidOutParagraph, MathEditTarget, MathPositionAffinity,
    MathSequencePath, MathSequenceStep, MathTextFlowPathStep, MathTextPosition, MathTreePosition,
    PageCaretGeometry, PageMathStructureHit, PageMathTextDragStop, PageMathTreeDragStop,
    PageSelectionRect, Pos, Selection, VisualAffinity, utf16_offset_to_utf8,
};
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    ops::Range,
};
use unicode_segmentation::UnicodeSegmentation;

type MathTextDragIsland = (MathEditTarget, String, Vec<(usize, MathTextPosition)>);

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
                    .map(|stop| math_drag_domain_id(&stop.target, stop.token))
            }
            Some(_) => snapshot
                .hit_test(page_index, x_pt, y_pt)
                .map(|_| "body".to_owned()),
            None if snapshot
                .hit_test_math_structure(page_index, x_pt, y_pt)
                .is_some_and(|hit| matches!(&hit.target, MathEditTarget::Inline { .. })) =>
            {
                Some("body".to_owned())
            }
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
        let mut seen = HashSet::new();
        for fragment in &page.layout.fragments {
            // 单元格片段不属于正文拖选域；跨宿主范围协议接入前保持隔离。
            if !matches!(fragment.host, caret_engine::FragmentHost::Body) {
                continue;
            }
            let Some((paragraph_order, paragraph)) =
                body_fragment_paragraph(snapshot, &fragment.paragraph_id)
            else {
                continue;
            };
            let grapheme_offsets = body_grapheme_offsets(paragraph);
            let context = BodyFragmentContext {
                snapshot,
                page_index,
                paragraph_order,
                paragraph,
                grapheme_offsets: &grapheme_offsets,
            };
            let hits = body_fragment_hits(&context, fragment.line_range.clone());
            segments.extend(hits.segments);
            stops.extend(body_fragment_stops(&context, &hits.offsets, &mut seen));
        }
        let (math_stops, math_islands) = math_drag_stop_entries(snapshot, page_index);
        stops.extend(math_stops);
        segments.extend(math_drag_segments(snapshot, page_index, math_islands));
        stops.extend(
            snapshot
                .math_tree_drag_stops(page_index)
                .iter()
                .map(|stop| math_tree_drag_stop_json(snapshot, page_index, stop)),
        );
        let hit_map = serde_json::json!({
            "documentGeneration": snapshot.document_generation.to_string(),
            "fontGeneration": snapshot.font_generation.to_string(),
            "pageIndex": page_index,
            "stops": stops,
            "segments": segments,
        });
        serde_json::to_string(&hit_map)
            .map_err(|error| format!("selection.hit_map_serialize: {error}"))
    }

    /// 将 Qt 拖选释放时提交的两个稳定文本端点设置为 Rust 选区。
    pub fn set_selection_json(&mut self, selection_json: &str) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct PositionDto {
            kind: Option<String>,
            domain_id: Option<String>,
            x_pt: Option<f64>,
            y_pt: Option<f64>,
            page_index: Option<usize>,
            paragraph_id: Option<String>,
            offset_utf8: Option<String>,
            order_utf8: Option<String>,
            affinity: Option<String>,
            visual_affinity: Option<String>,
            document_generation: Option<String>,
            font_generation: Option<String>,
            session_id: Option<String>,
            token: Option<u64>,
            target: Option<MathTargetDto>,
            align_cell: Option<Vec<usize>>,
            path: Option<Vec<MathPathStepDto>>,
            tree_path: Option<Vec<MathTreePathStepDto>>,
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
        #[serde(rename_all = "camelCase")]
        struct MathTreePathStepDto {
            kind: String,
            content_index: usize,
            operand_index: Option<usize>,
        }
        #[derive(serde::Deserialize)]
        struct SelectionDto {
            anchor: PositionDto,
            focus: PositionDto,
        }
        #[derive(Clone)]
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
        #[derive(Clone)]
        struct MathTreeEndpoint {
            page_index: usize,
            document_generation: u64,
            font_generation: u64,
            session_id: String,
            target: MathEditTarget,
            position: MathTreePosition,
            visual_affinity: VisualAffinity,
        }
        #[derive(Clone)]
        struct BodyEndpoint {
            position: Pos,
            visual_affinity: VisualAffinity,
            session_id: Option<String>,
            document_generation: Option<u64>,
            font_generation: Option<u64>,
            page_index: Option<usize>,
        }
        #[derive(Clone)]
        enum Endpoint {
            Body(BodyEndpoint),
            Math(MathEndpoint),
            MathTree(MathTreeEndpoint),
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
            if dto.kind.as_deref() == Some("mathTree") {
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
                    _ => return Err("selection.invalid_math_target".to_owned()),
                };
                let math_id = match &target {
                    MathEditTarget::Inline { math_id, .. } => math_id.clone(),
                    MathEditTarget::Block { .. } => {
                        return Err("selection.invalid_math_target".to_owned());
                    }
                };
                let path = dto
                    .tree_path
                    .unwrap_or_default()
                    .into_iter()
                    .map(|step| match step.kind.as_str() {
                        "operand" => Ok(caret_engine::MathSequenceStep::Operand {
                            content_index: step.content_index,
                            operand_index: step
                                .operand_index
                                .ok_or_else(|| "selection.invalid_math_path".to_owned())?,
                        }),
                        "environmentChildren" if step.operand_index.is_none() => {
                            Ok(caret_engine::MathSequenceStep::EnvironmentChildren {
                                content_index: step.content_index,
                            })
                        }
                        _ => Err("selection.invalid_math_path".to_owned()),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let offset = dto
                    .offset_utf8
                    .ok_or_else(|| "selection.invalid_math_offset".to_owned())?
                    .parse::<usize>()
                    .map_err(|_| "selection.invalid_math_offset".to_owned())?;
                let position = MathTreePosition {
                    host: caret_engine::MathPositionHost::InlineMath { owner_id: math_id },
                    path: MathSequencePath::new(path),
                    offset,
                    affinity: parse_math_affinity(dto.affinity)?,
                };
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
                return Ok(Endpoint::MathTree(MathTreeEndpoint {
                    page_index,
                    document_generation,
                    font_generation,
                    session_id,
                    target,
                    position,
                    visual_affinity: parse_visual_affinity(dto.visual_affinity)?,
                }));
            }
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
                return Ok(Endpoint::Body(BodyEndpoint {
                    position: Pos {
                        story,
                        paragraph_id,
                        offset,
                        affinity,
                    },
                    visual_affinity,
                    session_id: dto.session_id,
                    document_generation: dto
                        .document_generation
                        .map(|value| {
                            value
                                .parse::<u64>()
                                .map_err(|_| "selection.invalid_generation".to_owned())
                        })
                        .transpose()?,
                    font_generation: dto
                        .font_generation
                        .map(|value| {
                            value
                                .parse::<u64>()
                                .map_err(|_| "selection.invalid_generation".to_owned())
                        })
                        .transpose()?,
                    page_index: dto.page_index,
                }));
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
            if domain_id != math_drag_domain_id(&target, token) {
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
        let body_is_anchor = matches!((&anchor, &focus), (Endpoint::Body(_), Endpoint::Math(_)));
        let math_is_anchor = matches!(
            (&anchor, &focus),
            (Endpoint::Math(_), Endpoint::MathTree(_))
        );
        let math_right_moves_forward = |endpoint: &Endpoint,
                                        snapshot: &DocumentLayoutSnapshot|
         -> Result<bool, String> {
            let same_line = |left: &PageCaretGeometry, right: &PageCaretGeometry| {
                (left.y0_pt - right.y0_pt).abs() < 0.5 && (left.y1_pt - right.y1_pt).abs() < 0.5
            };
            let direction_from_neighbors =
                |current_x: f64,
                 current_caret: &PageCaretGeometry,
                 neighbors: &[(f64, bool, PageCaretGeometry)]| {
                    let right = neighbors
                        .iter()
                        .filter(|(x, _, caret)| {
                            *x > current_x + 0.01 && same_line(current_caret, caret)
                        })
                        .min_by(|left, right| left.0.total_cmp(&right.0));
                    if let Some((_, forward, _)) = right {
                        return *forward;
                    }
                    neighbors
                        .iter()
                        .filter(|(x, _, caret)| {
                            *x < current_x - 0.01 && same_line(current_caret, caret)
                        })
                        .max_by(|left, right| left.0.total_cmp(&right.0))
                        .is_none_or(|(_, forward, _)| !forward)
                };
            match endpoint {
                Endpoint::Math(math) => {
                    let stops = snapshot.math_text_drag_stops(math.page_index);
                    let current = stops
                        .iter()
                        .filter(|stop| {
                            stop.target == math.target
                                && stop.token == math.token
                                && stop.order_utf8 == math.order_utf8
                        })
                        .min_by(|left, right| {
                            let distance = |stop: &PageMathTextDragStop| {
                                (stop.caret.x_pt - math.x_pt).hypot(stop.caret.y0_pt - math.y_pt)
                            };
                            distance(left).total_cmp(&distance(right))
                        })
                        .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                    let neighbors = stops
                        .iter()
                        .filter(|stop| stop.target == math.target && stop.token == math.token)
                        .map(|stop| {
                            (
                                stop.caret.x_pt,
                                stop.order_utf8 > current.order_utf8,
                                stop.caret,
                            )
                        })
                        .collect::<Vec<_>>();
                    Ok(direction_from_neighbors(
                        current.caret.x_pt,
                        &current.caret,
                        &neighbors,
                    ))
                }
                Endpoint::MathTree(tree) => {
                    let current_caret = snapshot
                        .math_tree_caret_geometry(&tree.target, &tree.position)
                        .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                    let neighbors = snapshot
                        .math_tree_drag_stops(tree.page_index)
                        .into_iter()
                        .filter(|stop| {
                            stop.target == tree.target
                                && stop.position.host == tree.position.host
                                && stop.position.path == tree.position.path
                        })
                        .map(|stop| {
                            (
                                stop.caret.x_pt,
                                stop.position.offset > tree.position.offset,
                                stop.caret,
                            )
                        })
                        .collect::<Vec<_>>();
                    Ok(direction_from_neighbors(
                        current_caret.x_pt,
                        &current_caret,
                        &neighbors,
                    ))
                }
                Endpoint::Body(_) => Ok(true),
            }
        };
        let with_math_forward = |endpoint, forward| match endpoint {
            caret_engine::DocumentRangeEndpoint::InlineMathText {
                paragraph_id,
                math_id,
                position,
                entry_direction,
                ..
            } => caret_engine::DocumentRangeEndpoint::InlineMathText {
                paragraph_id,
                math_id,
                position,
                entry_direction,
                entry_math_forward: forward,
            },
            caret_engine::DocumentRangeEndpoint::InlineMathTree {
                paragraph_id,
                position,
                entry_direction,
                ..
            } => caret_engine::DocumentRangeEndpoint::InlineMathTree {
                paragraph_id,
                position,
                entry_direction,
                entry_math_forward: forward,
            },
            body => body,
        };
        match (anchor, focus) {
            (Endpoint::Body(anchor), Endpoint::Body(focus)) => {
                let effect = self
                    .session
                    .set_pointer_selection_range(
                        anchor.position,
                        focus.position,
                        anchor.visual_affinity,
                        focus.visual_affinity,
                    )
                    .map_err(|error| format!("selection.set: {error}"))?;
                self.preferred_visual_x_pt = None;
                if effect.composition_changed {
                    self.clear_composition_state();
                    return self.layout_snapshot_json_after_composition_cancel();
                }
            }
            (Endpoint::Body(body), Endpoint::Math(math))
            | (Endpoint::Math(math), Endpoint::Body(body)) => {
                let MathEditTarget::Inline {
                    paragraph_id,
                    math_id,
                } = &math.target
                else {
                    return Err("selection.invalid_math_target".to_owned());
                };
                let snapshot = if self.composition_preview_session.is_some() {
                    self.base_snapshot()?
                } else {
                    self.current_snapshot()?
                };
                if snapshot.document_generation != math.document_generation
                    || snapshot.font_generation != math.font_generation
                    || snapshot.session_id.as_ref().map(ToString::to_string)
                        != Some(math.session_id.clone())
                    || body.document_generation != Some(snapshot.document_generation)
                    || body.font_generation != Some(snapshot.font_generation)
                    || body.session_id.as_deref()
                        != snapshot
                            .session_id
                            .as_ref()
                            .map(ToString::to_string)
                            .as_deref()
                    || body.page_index.is_none_or(|page_index| {
                        snapshot
                            .caret_geometry(&body.position, body.visual_affinity)
                            .is_none_or(|caret| caret.page_index != page_index)
                    })
                {
                    return Err("selection.stale_pointer_hit".to_owned());
                }
                snapshot
                    .math_text_drag_hit(
                        math.page_index,
                        math.token,
                        &math.target,
                        &math.position,
                        math.order_utf8,
                        math.visual_affinity,
                    )
                    .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                let math_endpoint_value = Endpoint::Math(math.clone());
                let math_forward = math_right_moves_forward(&math_endpoint_value, snapshot)?;
                let math_endpoint = caret_engine::DocumentRangeEndpoint::InlineMathText {
                    paragraph_id: paragraph_id.clone(),
                    math_id: math_id.clone(),
                    position: math.position.clone(),
                    entry_direction: caret_engine::Direction::Forward,
                    entry_math_forward: math_forward,
                };
                let body_endpoint = caret_engine::DocumentRangeEndpoint::Text(body.position);
                let range = if body_is_anchor {
                    caret_engine::StructureSelection {
                        anchor: body_endpoint,
                        focus: math_endpoint,
                    }
                } else {
                    caret_engine::StructureSelection {
                        anchor: math_endpoint,
                        focus: body_endpoint,
                    }
                };
                let effect = self
                    .session
                    .set_pointer_inline_math_selection(
                        range,
                        if body_is_anchor {
                            body.visual_affinity
                        } else {
                            math.visual_affinity
                        },
                        if body_is_anchor {
                            math.visual_affinity
                        } else {
                            body.visual_affinity
                        },
                    )
                    .map_err(|error| format!("selection.inline_math_text: {error}"))?;
                self.preferred_visual_x_pt = None;
                if effect.composition_changed {
                    self.clear_composition_state();
                    return self.layout_snapshot_json_after_composition_cancel();
                }
            }
            (Endpoint::Math(math), Endpoint::MathTree(tree))
            | (Endpoint::MathTree(tree), Endpoint::Math(math)) => {
                let snapshot = if self.composition_preview_session.is_some() {
                    self.base_snapshot()?
                } else {
                    self.current_snapshot()?
                };
                if snapshot.document_generation != math.document_generation
                    || snapshot.font_generation != math.font_generation
                    || snapshot.session_id.as_ref().map(ToString::to_string)
                        != Some(math.session_id.clone())
                    || snapshot.document_generation != tree.document_generation
                    || snapshot.font_generation != tree.font_generation
                    || snapshot.session_id.as_ref().map(ToString::to_string)
                        != Some(tree.session_id.clone())
                    || snapshot
                        .math_tree_caret_geometry(&tree.target, &tree.position)
                        .is_none_or(|caret| caret.page_index != tree.page_index)
                {
                    return Err("selection.stale_pointer_hit".to_owned());
                }
                snapshot
                    .math_text_drag_hit(
                        math.page_index,
                        math.token,
                        &math.target,
                        &math.position,
                        math.order_utf8,
                        math.visual_affinity,
                    )
                    .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                let MathEditTarget::Inline {
                    paragraph_id: math_paragraph_id,
                    math_id,
                } = &math.target
                else {
                    return Err("selection.invalid_math_target".to_owned());
                };
                let MathEditTarget::Inline {
                    paragraph_id: tree_paragraph_id,
                    ..
                } = &tree.target
                else {
                    return Err("selection.invalid_math_target".to_owned());
                };
                let focus_endpoint = if math_is_anchor {
                    Endpoint::MathTree(tree.clone())
                } else {
                    Endpoint::Math(math.clone())
                };
                let math_forward = math_right_moves_forward(&focus_endpoint, snapshot)?;
                let math_endpoint = caret_engine::DocumentRangeEndpoint::InlineMathText {
                    paragraph_id: math_paragraph_id.clone(),
                    math_id: math_id.clone(),
                    position: math.position.clone(),
                    entry_direction: caret_engine::Direction::Forward,
                    entry_math_forward: math_forward,
                };
                let tree_endpoint = caret_engine::DocumentRangeEndpoint::InlineMathTree {
                    paragraph_id: tree_paragraph_id.clone(),
                    position: tree.position.clone(),
                    entry_direction: caret_engine::Direction::Forward,
                    entry_math_forward: math_forward,
                };
                let range = if math_is_anchor {
                    caret_engine::StructureSelection {
                        anchor: math_endpoint,
                        focus: tree_endpoint,
                    }
                } else {
                    caret_engine::StructureSelection {
                        anchor: tree_endpoint,
                        focus: math_endpoint,
                    }
                };
                let effect = self
                    .session
                    .set_pointer_inline_math_selection(
                        range,
                        if math_is_anchor {
                            math.visual_affinity
                        } else {
                            tree.visual_affinity
                        },
                        if math_is_anchor {
                            tree.visual_affinity
                        } else {
                            math.visual_affinity
                        },
                    )
                    .map_err(|error| format!("selection.inline_math_mixed: {error}"))?;
                self.preferred_visual_x_pt = None;
                if effect.composition_changed {
                    self.clear_composition_state();
                    return self.layout_snapshot_json_after_composition_cancel();
                }
            }
            (Endpoint::Math(anchor), Endpoint::Math(focus)) => {
                if anchor.document_generation != focus.document_generation
                    || anchor.font_generation != focus.font_generation
                    || anchor.session_id != focus.session_id
                {
                    return Err("selection.invalid_math_range".to_owned());
                }
                if anchor.target != focus.target || anchor.token != focus.token {
                    let snapshot = if self.composition_preview_session.is_some() {
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
                    for endpoint in [&anchor, &focus] {
                        snapshot
                            .math_text_drag_hit(
                                endpoint.page_index,
                                endpoint.token,
                                &endpoint.target,
                                &endpoint.position,
                                endpoint.order_utf8,
                                endpoint.visual_affinity,
                            )
                            .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                    }
                    let endpoint = |math: &MathEndpoint, direction| {
                        let MathEditTarget::Inline {
                            paragraph_id,
                            math_id,
                        } = &math.target
                        else {
                            return Err("selection.invalid_math_target".to_owned());
                        };
                        Ok(caret_engine::DocumentRangeEndpoint::InlineMathText {
                            paragraph_id: paragraph_id.clone(),
                            math_id: math_id.clone(),
                            position: math.position.clone(),
                            entry_direction: direction,
                            entry_math_forward: true,
                        })
                    };
                    let math_forward =
                        math_right_moves_forward(&Endpoint::Math(focus.clone()), snapshot)?;
                    let range = caret_engine::StructureSelection {
                        anchor: with_math_forward(
                            endpoint(&anchor, caret_engine::Direction::Forward)?,
                            math_forward,
                        ),
                        focus: with_math_forward(
                            endpoint(&focus, caret_engine::Direction::Forward)?,
                            math_forward,
                        ),
                    };
                    let effect = self
                        .session
                        .set_pointer_inline_math_selection(
                            range,
                            anchor.visual_affinity,
                            focus.visual_affinity,
                        )
                        .map_err(|error| format!("selection.inline_math_mixed: {error}"))?;
                    self.preferred_visual_x_pt = None;
                    if effect.composition_changed {
                        self.clear_composition_state();
                        return self.layout_snapshot_json_after_composition_cancel();
                    }
                    return self.interaction_state_json();
                }
                if anchor.domain_id != focus.domain_id {
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
                self.preferred_visual_x_pt = None;
                if effect.composition_changed {
                    self.clear_composition_state();
                    return self.layout_snapshot_json_after_composition_cancel();
                }
            }
            (
                anchor @ (Endpoint::Body(_) | Endpoint::MathTree(_)),
                focus @ (Endpoint::Body(_) | Endpoint::MathTree(_)),
            ) => {
                let (first_tree, second_tree) = match (&anchor, &focus) {
                    (Endpoint::Body(_), Endpoint::MathTree(tree))
                    | (Endpoint::MathTree(tree), Endpoint::Body(_)) => (tree, None),
                    (Endpoint::MathTree(first), Endpoint::MathTree(second)) => {
                        (first, Some(second))
                    }
                    _ => return Err("selection.invalid_math_target".to_owned()),
                };
                let target = first_tree.target.clone();
                let snapshot = if self.composition_preview_session.is_some() {
                    self.base_snapshot()?
                } else {
                    self.current_snapshot()?
                };
                for tree in [Some(first_tree), second_tree].into_iter().flatten() {
                    if snapshot.document_generation != tree.document_generation
                        || snapshot.font_generation != tree.font_generation
                        || snapshot.session_id.as_ref().map(ToString::to_string)
                            != Some(tree.session_id.clone())
                        || snapshot
                            .math_tree_caret_geometry(&tree.target, &tree.position)
                            .is_none_or(|caret| caret.page_index != tree.page_index)
                    {
                        return Err("selection.stale_math_hit".to_owned());
                    }
                }
                if let Some(body) = match (&anchor, &focus) {
                    (Endpoint::Body(body), _) | (_, Endpoint::Body(body)) => Some(body),
                    _ => None,
                } && (body.document_generation != Some(snapshot.document_generation)
                    || body.font_generation != Some(snapshot.font_generation)
                    || body.session_id.as_deref()
                        != snapshot
                            .session_id
                            .as_ref()
                            .map(ToString::to_string)
                            .as_deref()
                    || body.page_index.is_none_or(|page_index| {
                        snapshot
                            .caret_geometry(&body.position, body.visual_affinity)
                            .is_none_or(|caret| caret.page_index != page_index)
                    }))
                {
                    return Err("selection.stale_body_hit".to_owned());
                }
                if let (Some(first), Some(second)) = (Some(first_tree), second_tree)
                    && first.position.host == second.position.host
                    && first.position.path == second.position.path
                    && first.position.offset == second.position.offset
                {
                    let caret = snapshot
                        .math_tree_caret_geometry(&target, &second.position)
                        .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                    self.session
                        .focus_math_structure_hit(
                            &PageMathStructureHit {
                                session_id: Some(
                                    caret_doc::NodeId::parse(&second.session_id)
                                        .map_err(|_| "selection.missing_session_id".to_owned())?,
                                ),
                                document_generation: second.document_generation,
                                font_generation: second.font_generation,
                                target,
                                position: second.position.clone(),
                                caret,
                            },
                            second.font_generation,
                        )
                        .map_err(|error| format!("selection.inline_math_tree: {error}"))?;
                    self.preferred_visual_x_pt = None;
                    return self.interaction_state_json();
                }
                let visual_affinity = |endpoint: &Endpoint| match endpoint {
                    Endpoint::Body(body) => body.visual_affinity,
                    Endpoint::MathTree(tree) => tree.visual_affinity,
                    Endpoint::Math(_) => VisualAffinity::Downstream,
                };
                let as_range_endpoint = |endpoint: &Endpoint, direction| match endpoint {
                    Endpoint::Body(body) => Ok(caret_engine::DocumentRangeEndpoint::Text(
                        body.position.clone(),
                    )),
                    Endpoint::MathTree(tree) => {
                        let MathEditTarget::Inline { paragraph_id, .. } = &tree.target else {
                            return Err("selection.invalid_math_target".to_owned());
                        };
                        Ok(caret_engine::DocumentRangeEndpoint::InlineMathTree {
                            paragraph_id: paragraph_id.clone(),
                            position: tree.position.clone(),
                            entry_direction: direction,
                            entry_math_forward: true,
                        })
                    }
                    Endpoint::Math(_) => Err("selection.invalid_pointer_domain".to_owned()),
                };
                let focus_endpoint = if matches!(&focus, Endpoint::MathTree(_)) {
                    &focus
                } else {
                    &anchor
                };
                let math_forward = math_right_moves_forward(focus_endpoint, snapshot)?;
                let range = caret_engine::StructureSelection {
                    anchor: with_math_forward(
                        as_range_endpoint(&anchor, caret_engine::Direction::Forward)?,
                        math_forward,
                    ),
                    focus: with_math_forward(
                        as_range_endpoint(&focus, caret_engine::Direction::Forward)?,
                        math_forward,
                    ),
                };
                let effect = self
                    .session
                    .set_pointer_inline_math_selection(
                        range,
                        visual_affinity(&anchor),
                        visual_affinity(&focus),
                    )
                    .map_err(|error| format!("selection.inline_math: {error}"))?;
                self.preferred_visual_x_pt = None;
                if effect.composition_changed {
                    self.clear_composition_state();
                    return self.layout_snapshot_json_after_composition_cancel();
                }
            }
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
        self.preferred_visual_x_pt = None;
        if effect.composition_changed {
            self.clear_composition_state();
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

    /// 生成系统剪贴板的双格式 JSON 载荷：Caret 片段信封与纯文本回退。
    ///
    /// `fragment` 为 null 只表示空选区；选区端点落在公式内部等无法构造完整片段的
    /// 情况按错误返回，由调用方保留原系统剪贴板，不降级为纯文本。
    pub fn copy_clipboard_json(&mut self) -> Result<String, String> {
        let copy = self.session.copy_clipboard_fragment().map_err(|error| {
            let message = error.to_string();
            if message.starts_with("clipboard.") {
                message
            } else {
                format!("clipboard.copy: {message}")
            }
        })?;
        let (fragment, text) = match copy {
            None => (serde_json::Value::Null, String::new()),
            Some(copy) => (
                serde_json::Value::String(
                    caret_doc::encode_clipboard_fragment_json(&copy.fragment)
                        .map_err(|error| format!("clipboard.copy: {error}"))?,
                ),
                copy.plain_text,
            ),
        };
        serde_json::to_string(&serde_json::json!({ "fragment": fragment, "text": text }))
            .map_err(|error| format!("clipboard.serialize: {error}"))
    }
}

/// 正文拖选停靠点的去重身份：段落 ID、逻辑偏移、逻辑亲和性与视觉亲和性侧别。
type BodyStopIdentity = (String, usize, bool, bool);

/// 已按域归并的数学文本岛字素点，供生成岛内选区矩形条目。
type MathDragIslands = HashMap<String, MathTextDragIsland>;

/// 单个正文页面片段在命中映射各构造阶段共用的页面、段落身份与字素边界。
struct BodyFragmentContext<'a> {
    snapshot: &'a DocumentLayoutSnapshot,
    page_index: usize,
    paragraph_order: usize,
    paragraph: &'a LaidOutParagraph,
    grapheme_offsets: &'a [usize],
}

/// 一次字素扫描得到的正文选区矩形条目与字素停靠偏移。
struct BodyDragHits {
    segments: Vec<serde_json::Value>,
    offsets: Vec<usize>,
}

/// 单行正文在段落逻辑文本中的字节范围；端点用于筛选字素边界。
struct BodyLineBytes {
    source_start: usize,
    consumed_end: usize,
    source_end: usize,
}

/// 在快照段落表中定位正文片段所属段落，返回段落序号与段落引用。
fn body_fragment_paragraph<'a>(
    snapshot: &'a DocumentLayoutSnapshot,
    paragraph_id: &caret_doc::NodeId,
) -> Option<(usize, &'a LaidOutParagraph)> {
    snapshot
        .paragraphs
        .iter()
        .enumerate()
        .find(|(_, paragraph)| paragraph.id == *paragraph_id)
}

/// 计算段落逻辑文本的字素边界，并追加文本长度作为末尾边界。
fn body_grapheme_offsets(paragraph: &LaidOutParagraph) -> Vec<usize> {
    let mut offsets = paragraph
        .text
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    offsets.push(paragraph.text.len());
    offsets
}

/// 按行收集正文页面片段产生的选区矩形条目与去重后的字素停靠偏移。
fn body_fragment_hits(context: &BodyFragmentContext<'_>, line_range: Range<usize>) -> BodyDragHits {
    let mut hits = BodyDragHits {
        segments: Vec::new(),
        offsets: Vec::new(),
    };
    for line_index in line_range {
        let Some(line) = context.paragraph.layout.paragraph.lines.get(line_index) else {
            continue;
        };
        let line_hits = body_line_hits(
            context,
            BodyLineBytes {
                source_start: line.source_bytes.start,
                consumed_end: line.consumed_bytes.end,
                source_end: line.source_bytes.end,
            },
        );
        hits.segments.extend(line_hits.segments);
        hits.offsets.extend(line_hits.offsets);
    }
    hits.offsets.sort_unstable();
    hits.offsets.dedup();
    hits
}

/// 收集单行覆盖的字素停靠偏移，并为相邻字素对生成正文选区矩形条目。
fn body_line_hits(context: &BodyFragmentContext<'_>, line: BodyLineBytes) -> BodyDragHits {
    let first_boundary = context
        .grapheme_offsets
        .partition_point(|offset| *offset < line.source_start);
    let boundary_end = context
        .grapheme_offsets
        .partition_point(|offset| *offset <= line.source_end);
    let offsets = context.grapheme_offsets[first_boundary..]
        .iter()
        .copied()
        .take_while(|offset| *offset <= line.consumed_end)
        .collect();
    let segments = context.grapheme_offsets[first_boundary..boundary_end]
        .windows(2)
        .flat_map(|pair| body_segment_hits(context, pair[0], pair[1]))
        .collect();
    BodyDragHits { segments, offsets }
}

/// 为一个字素对生成落在目标页内的正文选区矩形条目，保持矩形原始顺序。
fn body_segment_hits(
    context: &BodyFragmentContext<'_>,
    start: usize,
    end: usize,
) -> Vec<serde_json::Value> {
    let selection = Selection {
        anchor: Pos {
            story: context.paragraph.story.clone(),
            paragraph_id: context.paragraph.id.clone(),
            offset: start,
            affinity: Affinity::Downstream,
        },
        focus: Pos {
            story: context.paragraph.story.clone(),
            paragraph_id: context.paragraph.id.clone(),
            offset: end,
            affinity: Affinity::Downstream,
        },
    };
    let Some(rectangles) = context.snapshot.selection_geometry(&selection) else {
        return Vec::new();
    };
    rectangles
        .into_iter()
        .filter(|rectangle| rectangle.page_index == context.page_index)
        .map(|rectangle| body_segment_json(context, start, end, rectangle))
        .collect()
}

/// 构造单条正文选区矩形命中条目，字段名、类型与数值格式遵循拖选协议。
fn body_segment_json(
    context: &BodyFragmentContext<'_>,
    start: usize,
    end: usize,
    rectangle: PageSelectionRect,
) -> serde_json::Value {
    let mut segment = serde_json::json!({
        "domainId": "body",
        "paragraphIndex": context.paragraph_order,
        "startUtf8": start.to_string(),
        "endUtf8": end.to_string(),
        "startPosition": {
            "kind": "body",
            "paragraphIndex": context.paragraph_order,
            "offsetUtf8": start.to_string(),
            "orderUtf8": start.to_string(),
        },
        "endPosition": {
            "kind": "body",
            "paragraphIndex": context.paragraph_order,
            "offsetUtf8": end.to_string(),
            "orderUtf8": end.to_string(),
        },
    });
    if let Some(object) = segment.as_object_mut() {
        object.extend(rectangle_fields(rectangle));
    }
    segment
}

/// 为正文片段的每个字素停靠偏移生成两种逻辑亲和性与两种视觉亲和性的光标停靠条目。
fn body_fragment_stops(
    context: &BodyFragmentContext<'_>,
    offsets: &[usize],
    seen: &mut HashSet<BodyStopIdentity>,
) -> Vec<serde_json::Value> {
    let mut stops = Vec::new();
    for &offset in offsets {
        for logical_affinity in [Affinity::Upstream, Affinity::Downstream] {
            for visual_affinity in [VisualAffinity::Upstream, VisualAffinity::Downstream] {
                let Some(stop) =
                    body_caret_stop(context, offset, logical_affinity, visual_affinity, seen)
                else {
                    continue;
                };
                stops.push(stop);
            }
        }
    }
    stops
}

/// 生成单个正文光标停靠条目；同段落、同偏移与同侧别的重复停靠只保留首次。
fn body_caret_stop(
    context: &BodyFragmentContext<'_>,
    offset: usize,
    logical_affinity: Affinity,
    visual_affinity: VisualAffinity,
    seen: &mut HashSet<BodyStopIdentity>,
) -> Option<serde_json::Value> {
    let position = Pos {
        story: context.paragraph.story.clone(),
        paragraph_id: context.paragraph.id.clone(),
        offset,
        affinity: logical_affinity,
    };
    let caret = context
        .snapshot
        .caret_geometry(&position, visual_affinity)?;
    if caret.page_index != context.page_index {
        return None;
    }
    let identity = (
        context.paragraph.id.to_string(),
        offset,
        matches!(logical_affinity, Affinity::Upstream),
        matches!(visual_affinity, VisualAffinity::Upstream),
    );
    if !seen.insert(identity) {
        return None;
    }
    Some(body_caret_stop_json(
        context,
        offset,
        logical_affinity,
        visual_affinity,
        caret,
    ))
}

/// 构造单条正文光标停靠条目，字段名与亲和性命名遵循拖选协议。
fn body_caret_stop_json(
    context: &BodyFragmentContext<'_>,
    offset: usize,
    logical_affinity: Affinity,
    visual_affinity: VisualAffinity,
    caret: PageCaretGeometry,
) -> serde_json::Value {
    serde_json::json!({
        "domainId": "body",
        "pageIndex": context.page_index,
        "xPt": caret.x_pt,
        "y0Pt": caret.y0_pt,
        "y1Pt": caret.y1_pt,
        "position": {
            "kind": "body",
            "domainId": "body",
            "sessionId": context.snapshot.session_id.as_ref().map(ToString::to_string),
            "pageIndex": context.page_index,
            "xPt": caret.x_pt,
            "yPt": (caret.y0_pt + caret.y1_pt) / 2.0,
            "documentGeneration": context.snapshot.document_generation.to_string(),
            "fontGeneration": context.snapshot.font_generation.to_string(),
            "paragraphId": context.paragraph.id.to_string(),
            "paragraphIndex": context.paragraph_order,
            "offsetUtf8": offset.to_string(),
            "orderUtf8": offset.to_string(),
            "affinity": if logical_affinity == Affinity::Upstream { "upstream" } else { "downstream" },
        },
        "visualAffinity": if visual_affinity == VisualAffinity::Upstream { "upstream" } else { "downstream" },
    })
}

/// 收集数学文本拖选停靠条目，并按域归并岛内字素点。
fn math_drag_stop_entries(
    snapshot: &DocumentLayoutSnapshot,
    page_index: usize,
) -> (Vec<serde_json::Value>, MathDragIslands) {
    let mut stops = Vec::new();
    let mut islands = MathDragIslands::new();
    for stop in snapshot.math_text_drag_stops(page_index) {
        let island_id = math_text_domain_id(&stop.target, stop.token);
        let domain_id = math_drag_domain_id(&stop.target, stop.token);
        let island = islands
            .entry(island_id)
            .or_insert_with(|| (stop.target.clone(), domain_id.clone(), Vec::new()));
        island.2.push((stop.order_utf8, stop.position.clone()));
        stops.push(math_hit_stop_json(snapshot, page_index, &stop, &domain_id));
    }
    (stops, islands)
}

/// 构造单条数学文本光标停靠条目，内嵌位置字段与拖选协议保持一致。
fn math_hit_stop_json(
    snapshot: &DocumentLayoutSnapshot,
    page_index: usize,
    stop: &PageMathTextDragStop,
    domain_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "domainId": domain_id,
        "pageIndex": page_index,
        "xPt": stop.caret.x_pt,
        "y0Pt": stop.caret.y0_pt,
        "y1Pt": stop.caret.y1_pt,
        "position": {
            "kind": "math",
            "sessionId": snapshot.session_id.as_ref().map(ToString::to_string),
            "domainId": domain_id,
            "pageIndex": page_index,
            "xPt": stop.caret.x_pt,
            "yPt": (stop.caret.y0_pt + stop.caret.y1_pt) / 2.0,
            "documentGeneration": snapshot.document_generation.to_string(),
            "fontGeneration": snapshot.font_generation.to_string(),
            "token": stop.token,
            "orderUtf8": stop.order_utf8.to_string(),
            // MathTree 的 UTF-8 负载上限为 8 MiB，因此此数值可由 JavaScript 精确表示。
            "orderOrdinal": stop.order_utf8 as u64,
            "offsetUtf8": stop.position.offset.to_string(),
            "affinity": math_affinity_name(stop.position.affinity),
            "visualAffinity": visual_affinity_name(stop.visual_affinity),
            "target": math_target_json(&stop.target),
            "alignCell": stop.position.path.align_cell(),
            "path": stop.position.path.steps().iter().map(math_path_step_json).collect::<Vec<_>>(),
        },
        "visualAffinity": visual_affinity_name(stop.visual_affinity),
    })
}

/// 构造可由指针提交的公式树停靠点，并保留精确的序列路径。
fn math_tree_drag_stop_json(
    snapshot: &DocumentLayoutSnapshot,
    page_index: usize,
    stop: &PageMathTreeDragStop,
) -> serde_json::Value {
    serde_json::json!({
        "domainId": "body",
        "pageIndex": page_index,
        "xPt": stop.caret.x_pt,
        "y0Pt": stop.caret.y0_pt,
        "y1Pt": stop.caret.y1_pt,
        "position": {
            "kind": "mathTree",
            "domainId": "body",
            "sessionId": snapshot.session_id.as_ref().map(ToString::to_string),
            "pageIndex": page_index,
            "xPt": stop.caret.x_pt,
            "yPt": (stop.caret.y0_pt + stop.caret.y1_pt) / 2.0,
            "documentGeneration": snapshot.document_generation.to_string(),
            "fontGeneration": snapshot.font_generation.to_string(),
            "offsetUtf8": stop.position.offset.to_string(),
            "affinity": math_affinity_name(stop.position.affinity),
            "visualAffinity": visual_affinity_name(stop.visual_affinity),
            "target": math_target_json(&stop.target),
            "treePath": stop.position.path.steps().iter().map(math_tree_path_step_json).collect::<Vec<_>>(),
        },
        "visualAffinity": visual_affinity_name(stop.visual_affinity),
    })
}

/// 将公式树序列路径投影为 UI 可往返的稳定整数步骤。
fn math_tree_path_step_json(step: &MathSequenceStep) -> serde_json::Value {
    match step {
        MathSequenceStep::Operand {
            content_index,
            operand_index,
        } => serde_json::json!({
            "kind": "operand",
            "contentIndex": content_index,
            "operandIndex": operand_index,
        }),
        MathSequenceStep::EnvironmentChildren { content_index } => serde_json::json!({
            "kind": "environmentChildren",
            "contentIndex": content_index,
        }),
    }
}

/// 为每个数学文本岛生成岛内相邻字素点的选区矩形条目。
fn math_drag_segments(
    snapshot: &DocumentLayoutSnapshot,
    page_index: usize,
    islands: MathDragIslands,
) -> Vec<serde_json::Value> {
    let mut segments = Vec::new();
    for (_, (target, domain_id, mut points)) in islands {
        points.sort_unstable_by_key(|(offset, _)| *offset);
        points.dedup_by_key(|(offset, _)| *offset);
        for pair in points.windows(2) {
            segments.extend(math_pair_segments(
                snapshot, page_index, &domain_id, &target, &pair[0], &pair[1],
            ));
        }
    }
    segments
}

/// 为一个相邻数学文本点对生成落在目标页内的选区矩形条目，保持矩形原始顺序。
fn math_pair_segments(
    snapshot: &DocumentLayoutSnapshot,
    page_index: usize,
    domain_id: &str,
    target: &MathEditTarget,
    start_point: &(usize, MathTextPosition),
    end_point: &(usize, MathTextPosition),
) -> Vec<serde_json::Value> {
    let (start_offset, start_position) = start_point;
    let (end_offset, end_position) = end_point;
    if start_offset == end_offset {
        return Vec::new();
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
    let Some(rectangles) = snapshot.math_text_selection_geometry(target, &start, &end) else {
        return Vec::new();
    };
    rectangles
        .into_iter()
        .filter(|rectangle| rectangle.page_index == page_index)
        .map(|rectangle| math_segment_json(domain_id, *start_offset, *end_offset, rectangle))
        .collect()
}

/// 构造单条数学文本选区矩形命中条目，字段名、类型与数值格式遵循拖选协议。
fn math_segment_json(
    domain_id: &str,
    start_offset: usize,
    end_offset: usize,
    rectangle: PageSelectionRect,
) -> serde_json::Value {
    let mut segment = serde_json::json!({
        "domainId": domain_id,
        "startUtf8": start_offset.to_string(),
        "endUtf8": end_offset.to_string(),
        "startPosition": {
            "kind": "math",
            "orderUtf8": start_offset.to_string(),
            "orderOrdinal": start_offset as u64,
        },
        "endPosition": {
            "kind": "math",
            "orderUtf8": end_offset.to_string(),
            "orderOrdinal": end_offset as u64,
        },
    });
    if let Some(object) = segment.as_object_mut() {
        object.extend(rectangle_fields(rectangle));
    }
    segment
}

fn math_text_domain_id(target: &MathEditTarget, token: u64) -> String {
    let target_id = match target {
        MathEditTarget::Inline { math_id, .. } => format!("inline:{math_id}"),
        MathEditTarget::Block { block_id, align } => format!("block:{block_id}:{align}"),
    };
    format!("math:{target_id}:{token}")
}

/// InlineMath 的文本岛与树边界共用正文拖选域；块公式文本岛仍保持独立数学域。
fn math_drag_domain_id(target: &MathEditTarget, token: u64) -> String {
    match target {
        MathEditTarget::Inline { .. } => "body".to_owned(),
        MathEditTarget::Block { .. } => math_text_domain_id(target, token),
    }
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
