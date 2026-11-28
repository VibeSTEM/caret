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
        let math_structure_hit = snapshot.hit_test_math_structure(page_index, x_pt, y_pt);
        let inside_table_cell = snapshot
            .table_cell_hit_regions(page_index)
            .iter()
            .any(|region| {
                x_pt >= region.x0_pt
                    && x_pt <= region.x1_pt
                    && y_pt >= region.y0_pt
                    && y_pt <= region.y1_pt
            });
        let domain_id = match snapshot.hit_test_math_text(page_index, x_pt, y_pt) {
            Some(hit)
                if super::hit_test::math_text_hit_wins_over_body(
                    snapshot, page_index, x_pt, y_pt, &hit,
                ) =>
            {
                Some(math_drag_domain_id(&hit.target, hit.token))
            }
            Some(_) => snapshot
                .hit_test(page_index, x_pt, y_pt)
                .map(|_| "body".to_owned()),
            None if math_structure_hit.as_ref().is_some_and(|hit| {
                matches!(&hit.target, MathEditTarget::Inline { .. })
                    || snapshot
                        .math_target_cell_id(page_index, &hit.target)
                        .is_some()
            }) =>
            {
                Some("body".to_owned())
            }
            None if math_structure_hit.is_some() => None,
            None if inside_table_cell => Some("body".to_owned()),
            None => snapshot
                .hit_test(page_index, x_pt, y_pt)
                .map(|_| "body".to_owned()),
        };
        serde_json::to_string(&serde_json::json!({
            "domainId": domain_id.unwrap_or_default(),
        }))
        .map_err(|error| format!("selection.drag_start_domain_serialize: {error}"))
    }

    /// 为指针拖选提供单页命中点与字素选择片段缓存。
    pub fn drag_hit_map_json(&mut self, page_index: i32) -> Result<String, String> {
        let page_index =
            usize::try_from(page_index).map_err(|_| "selection.invalid_page_index".to_owned())?;
        let composition_revision = self.composition_revision;
        let snapshot = self.current_snapshot()?;
        let page = snapshot
            .pages
            .get(page_index)
            .ok_or_else(|| "selection.invalid_page_index".to_owned())?;
        let mut stops = Vec::new();
        let mut segments = Vec::new();
        let mut seen = HashSet::new();
        for fragment in &page.layout.fragments {
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
                composition_revision,
            };
            let hits = body_fragment_hits(&context, fragment.line_range.clone());
            segments.extend(hits.segments);
            stops.extend(body_fragment_stops(&context, &hits.offsets, &mut seen));
        }
        let (math_stops, math_islands) =
            math_drag_stop_entries(snapshot, page_index, composition_revision);
        stops.extend(math_stops);
        segments.extend(math_drag_segments(snapshot, page_index, math_islands));
        stops.extend(
            snapshot
                .math_tree_drag_stops(page_index)
                .iter()
                .map(|stop| {
                    math_tree_drag_stop_json(snapshot, page_index, stop, composition_revision)
                }),
        );
        let cell_hit_regions = snapshot
            .table_cell_hit_regions(page_index)
            .iter()
            .map(|region| {
                serde_json::json!({
                    "cellId": region.cell_id.to_string(),
                    "pageIndex": page_index,
                    "x0Pt": region.x0_pt,
                    "y0Pt": region.y0_pt,
                    "x1Pt": region.x1_pt,
                    "y1Pt": region.y1_pt,
                })
            })
            .collect::<Vec<_>>();
        let hit_map = serde_json::json!({
            "documentGeneration": snapshot.document_generation.to_string(),
            "fontGeneration": snapshot.font_generation.to_string(),
            "pageIndex": page_index,
            "stops": stops,
            "segments": segments,
            "cellHitRegions": cell_hit_regions,
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
            composition_revision: Option<String>,
            target: Option<MathTargetDto>,
            block_id: Option<String>,
            side: Option<String>,
            tree_host: Option<MathTreeHostDto>,
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
        #[serde(rename_all = "camelCase")]
        struct MathTreeHostDto {
            kind: String,
            owner_id: Option<String>,
            row_id: Option<String>,
            cell_id: Option<String>,
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
            composition_revision: Option<u64>,
        }
        impl MathEndpoint {
            fn matches_layout(&self, snapshot: &DocumentLayoutSnapshot) -> bool {
                self.document_generation == snapshot.document_generation
                    && self.font_generation == snapshot.font_generation
                    && Some(self.session_id.as_str())
                        == snapshot.session_id.as_ref().map(|id| id.as_str())
            }

            fn hit(
                &self,
                snapshot: &DocumentLayoutSnapshot,
            ) -> Option<caret_engine::PageMathTextHit> {
                snapshot.math_text_drag_hit(
                    self.page_index,
                    self.token,
                    &self.target,
                    &self.position,
                    self.order_utf8,
                    self.visual_affinity,
                )
            }

            fn committed_drag_stop(
                &self,
                snapshot: &DocumentLayoutSnapshot,
            ) -> Option<PageMathTextDragStop> {
                let caret = snapshot.math_text_caret_geometry(
                    &self.target,
                    &self.position,
                    self.visual_affinity,
                )?;
                snapshot
                    .math_text_drag_stops(caret.page_index)
                    .into_iter()
                    .find(|stop| {
                        stop.target == self.target
                            && stop.position == self.position
                            && stop.visual_affinity == self.visual_affinity
                    })
            }

            fn committed_anchor_json(
                &self,
                snapshot: &DocumentLayoutSnapshot,
            ) -> Option<serde_json::Value> {
                let stop = self.committed_drag_stop(snapshot)?;
                Some(math_hit_stop_json(
                    snapshot,
                    stop.caret.page_index,
                    &stop,
                    &math_drag_domain_id(&stop.target, stop.token),
                    None,
                ))
            }
        }
        #[derive(Clone)]
        struct BodyEndpoint {
            position: Pos,
            visual_affinity: VisualAffinity,
            session_id: Option<String>,
            document_generation: Option<u64>,
            font_generation: Option<u64>,
            page_index: Option<usize>,
            composition_revision: Option<u64>,
        }
        impl BodyEndpoint {
            fn has_layout_identity(&self) -> bool {
                self.session_id.is_some()
                    || self.document_generation.is_some()
                    || self.font_generation.is_some()
            }

            fn matches_layout(&self, snapshot: &DocumentLayoutSnapshot) -> bool {
                self.document_generation == Some(snapshot.document_generation)
                    && self.font_generation == Some(snapshot.font_generation)
                    && self.session_id.as_deref()
                        == snapshot.session_id.as_ref().map(|id| id.as_str())
                    && self.page_index.is_some_and(|page_index| {
                        snapshot
                            .caret_geometry(&self.position, self.visual_affinity)
                            .is_some_and(|caret| caret.page_index == page_index)
                    })
            }

            fn map_composition_endpoint(
                &self,
                snapshot: &DocumentLayoutSnapshot,
                composition_range: Option<&caret_engine::CompositionRange>,
                replaced_selection: &Selection,
                preedit_len: usize,
            ) -> Option<Self> {
                let mut position = self.position.clone();
                if let Some(caret_engine::CompositionRange::Paragraph(range)) = composition_range
                    && position.paragraph_id == range.anchor.paragraph_id
                    && range.anchor.paragraph_id == range.focus.paragraph_id
                    && replaced_selection.anchor.paragraph_id
                        == replaced_selection.focus.paragraph_id
                    && replaced_selection.anchor.paragraph_id == position.paragraph_id
                {
                    let original_start = replaced_selection
                        .anchor
                        .offset
                        .min(replaced_selection.focus.offset);
                    let original_end = replaced_selection
                        .anchor
                        .offset
                        .max(replaced_selection.focus.offset);
                    let preview_start = original_start;
                    let preview_end = preview_start.checked_add(preedit_len)?;
                    position.offset = if position.offset <= preview_start {
                        position.offset
                    } else if position.offset >= preview_end {
                        original_end.checked_add(position.offset - preview_end)?
                    } else if position.affinity == Affinity::Upstream {
                        original_start
                    } else {
                        original_end
                    };
                }
                let caret = snapshot.caret_geometry(&position, self.visual_affinity)?;
                Some(Self {
                    position,
                    visual_affinity: self.visual_affinity,
                    session_id: snapshot.session_id.as_ref().map(ToString::to_string),
                    document_generation: Some(snapshot.document_generation),
                    font_generation: Some(snapshot.font_generation),
                    page_index: Some(caret.page_index),
                    composition_revision: None,
                })
            }
        }
        #[derive(Clone)]
        enum Endpoint {
            Body(BodyEndpoint),
            BlockEdge(BlockEdgeEndpoint),
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
            preview_session: Option<&caret_engine::Session>,
            dto: PositionDto,
        ) -> Result<Endpoint, String> {
            if dto.kind.as_deref() == Some("blockEdge") {
                if dto.domain_id.as_deref() != Some("body") {
                    return Err("selection.invalid_pointer_domain".to_owned());
                }
                let block_id = caret_doc::NodeId::parse(
                    dto.block_id
                        .ok_or_else(|| "selection.invalid_block_edge".to_owned())?,
                )
                .map_err(|_| "selection.invalid_block_edge".to_owned())?;
                let side = match dto.side.as_deref() {
                    Some("backward") => caret_engine::Direction::Backward,
                    Some("forward") => caret_engine::Direction::Forward,
                    _ => return Err("selection.invalid_block_edge".to_owned()),
                };
                dto.y_pt
                    .ok_or_else(|| "selection.invalid_block_edge".to_owned())?;
                return Ok(Endpoint::BlockEdge(BlockEdgeEndpoint {
                    block_id,
                    side,
                    visual_affinity: parse_visual_affinity(dto.visual_affinity)?,
                    session_id: dto
                        .session_id
                        .ok_or_else(|| "selection.missing_session_id".to_owned())?,
                    document_generation: dto
                        .document_generation
                        .ok_or_else(|| "selection.invalid_generation".to_owned())?
                        .parse::<u64>()
                        .map_err(|_| "selection.invalid_generation".to_owned())?,
                    font_generation: dto
                        .font_generation
                        .ok_or_else(|| "selection.invalid_generation".to_owned())?
                        .parse::<u64>()
                        .map_err(|_| "selection.invalid_generation".to_owned())?,
                    page_index: dto
                        .page_index
                        .ok_or_else(|| "selection.invalid_page_index".to_owned())?,
                    x_pt: dto
                        .x_pt
                        .ok_or_else(|| "selection.invalid_block_edge".to_owned())?,
                    y_pt: dto
                        .y_pt
                        .ok_or_else(|| "selection.invalid_block_edge".to_owned())?,
                    composition_revision: dto
                        .composition_revision
                        .map(|value| {
                            value
                                .parse::<u64>()
                                .map_err(|_| "selection.invalid_composition_revision".to_owned())
                        })
                        .transpose()?,
                }));
            }
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
                let host = match &target {
                    MathEditTarget::Inline { math_id, .. } => {
                        if dto.tree_host.as_ref().is_some_and(|host| {
                            host.kind != "inlineMath"
                                || host.owner_id.as_deref() != Some(math_id.as_str())
                        }) {
                            return Err("selection.invalid_math_target".to_owned());
                        }
                        caret_engine::MathPositionHost::InlineMath {
                            owner_id: math_id.clone(),
                        }
                    }
                    MathEditTarget::Block {
                        block_id,
                        align: false,
                    } => {
                        let host = dto
                            .tree_host
                            .ok_or_else(|| "selection.invalid_math_target".to_owned())?;
                        if host.kind != "equation"
                            || host.owner_id.as_deref() != Some(block_id.as_str())
                        {
                            return Err("selection.invalid_math_target".to_owned());
                        }
                        caret_engine::MathPositionHost::Equation {
                            owner_id: block_id.clone(),
                        }
                    }
                    MathEditTarget::Block { align: true, .. } => {
                        let host = dto
                            .tree_host
                            .ok_or_else(|| "selection.invalid_math_target".to_owned())?;
                        if host.kind != "alignCell" {
                            return Err("selection.invalid_math_target".to_owned());
                        }
                        caret_engine::MathPositionHost::AlignCell {
                            row_id: caret_doc::NodeId::parse(
                                host.row_id
                                    .ok_or_else(|| "selection.invalid_math_target".to_owned())?,
                            )
                            .map_err(|_| "selection.invalid_math_target".to_owned())?,
                            cell_id: caret_doc::NodeId::parse(
                                host.cell_id
                                    .ok_or_else(|| "selection.invalid_math_target".to_owned())?,
                            )
                            .map_err(|_| "selection.invalid_math_target".to_owned())?,
                        }
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
                    host,
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
                    x_pt: dto
                        .x_pt
                        .ok_or_else(|| "selection.invalid_pointer_hit".to_owned())?,
                    y_pt: dto
                        .y_pt
                        .ok_or_else(|| "selection.invalid_pointer_hit".to_owned())?,
                    composition_revision: dto
                        .composition_revision
                        .map(|value| {
                            value
                                .parse::<u64>()
                                .map_err(|_| "selection.invalid_composition_revision".to_owned())
                        })
                        .transpose()?,
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
                    .or_else(|| {
                        preview_session
                            .and_then(|preview| preview.story_for_paragraph(&paragraph_id))
                    })
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
                    composition_revision: dto
                        .composition_revision
                        .map(|value| {
                            value
                                .parse::<u64>()
                                .map_err(|_| "selection.invalid_composition_revision".to_owned())
                        })
                        .transpose()?,
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
                composition_revision: dto
                    .composition_revision
                    .map(|value| {
                        value
                            .parse::<u64>()
                            .map_err(|_| "selection.invalid_composition_revision".to_owned())
                    })
                    .transpose()?,
            }))
        }
        let dto: SelectionDto = serde_json::from_str(selection_json)
            .map_err(|error| format!("selection.invalid_payload: {error}"))?;
        let anchor = parse_endpoint(
            &self.session,
            self.composition_preview_session.as_ref(),
            dto.anchor,
        )?;
        let focus = parse_endpoint(
            &self.session,
            self.composition_preview_session.as_ref(),
            dto.focus,
        )?;
        for endpoint in [&anchor, &focus] {
            if let Endpoint::Body(body) = endpoint
                && body.has_layout_identity()
                && body.composition_revision != self.composition_revision
            {
                return Err("selection.stale_body_hit".to_owned());
            }
            if let Endpoint::BlockEdge(edge) = endpoint
                && edge.composition_revision != self.composition_revision
            {
                return Err("selection.stale_body_hit".to_owned());
            }
            if let Endpoint::MathTree(tree) = endpoint
                && tree.composition_revision != self.composition_revision
            {
                return Err("selection.stale_math_hit".to_owned());
            }
            if let Endpoint::Math(math) = endpoint
                && math.composition_revision != self.composition_revision
            {
                return Err("selection.stale_math_hit".to_owned());
            }
        }
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
                Endpoint::BlockEdge(_) => Ok(true),
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
        let composition_hit_active = self.composition_preview_session.is_some();
        let (anchor, focus) = if composition_hit_active {
            let preview = self.current_snapshot()?.clone();
            let base = self.base_snapshot()?.clone();
            let composition_range = self.composition_range.clone();
            let composing_math = matches!(
                composition_range,
                Some(caret_engine::CompositionRange::MathText { .. })
            )
            .then(|| self.session.current_math_edit_target())
            .flatten();
            let replaced_selection = self.session.selection().clone();
            let preedit_len = self
                .session
                .composition()
                .map_or(0, |composition| composition.preedit.len());
            let remap = |endpoint: Endpoint| -> Result<Endpoint, String> {
                let Endpoint::Body(body) = endpoint else {
                    if let Endpoint::Math(mut math) = endpoint {
                        if !math.matches_layout(&preview) {
                            return Err("selection.stale_math_hit".to_owned());
                        }
                        // 当前组字岛的字节偏移随预编辑文本变化，继续由岛内命中规则映射。
                        if composing_math != Some(&math.target) {
                            math.hit(&preview)
                                .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                            let stop = math
                                .committed_drag_stop(&base)
                                .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                            math.page_index = stop.caret.page_index;
                            math.token = stop.token;
                            math.order_utf8 = stop.order_utf8;
                            math.domain_id = math_drag_domain_id(&stop.target, stop.token);
                            math.x_pt = stop.caret.x_pt;
                            math.y_pt = (stop.caret.y0_pt + stop.caret.y1_pt) / 2.0;
                            math.composition_revision = None;
                        }
                        return Ok(Endpoint::Math(math));
                    }
                    if let Endpoint::MathTree(mut tree) = endpoint {
                        if tree.composition_revision != self.composition_revision
                            || tree.document_generation != preview.document_generation
                            || tree.font_generation != preview.font_generation
                            || Some(tree.session_id.as_str())
                                != preview.session_id.as_ref().map(|id| id.as_str())
                        {
                            return Err("selection.stale_math_hit".to_owned());
                        }
                        preview
                            .math_tree_caret_geometry(&tree.target, &tree.position)
                            .filter(|caret| caret.page_index == tree.page_index)
                            .filter(|caret| {
                                (caret.x_pt - tree.x_pt).abs() < 0.01
                                    && ((caret.y0_pt + caret.y1_pt) / 2.0 - tree.y_pt).abs() < 0.01
                            })
                            .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                        let base_caret = base
                            .math_tree_caret_geometry(&tree.target, &tree.position)
                            .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                        tree.page_index = base_caret.page_index;
                        tree.x_pt = base_caret.x_pt;
                        tree.y_pt = (base_caret.y0_pt + base_caret.y1_pt) / 2.0;
                        tree.composition_revision = None;
                        return Ok(Endpoint::MathTree(tree));
                    }
                    if let Endpoint::BlockEdge(edge) = &endpoint
                        && !block_edge_matches_snapshot(edge, &preview, &self.session)
                    {
                        return Err("selection.stale_body_hit".to_owned());
                    }
                    return Ok(endpoint);
                };
                if !body.has_layout_identity() {
                    return Ok(Endpoint::Body(body));
                }
                if !body.matches_layout(&preview) {
                    return Err("selection.stale_body_hit".to_owned());
                }
                if self
                    .session
                    .story_for_paragraph(&body.position.paragraph_id)
                    .is_none()
                    && self
                        .composition_preview_session
                        .as_ref()
                        .is_some_and(|preview| {
                            preview.story_for_paragraph(&body.position.paragraph_id)
                                == Some(body.position.story.clone())
                        })
                    && self.session.block_boundary_exit().is_some()
                {
                    let (block_id, side) = self
                        .session
                        .block_boundary_exit()
                        .ok_or_else(|| "selection.stale_body_hit".to_owned())?;
                    let fallback = base
                        .caret_geometry(
                            &self.session.selection().focus,
                            self.session.selection_visual_affinity(),
                        )
                        .ok_or_else(|| "selection.stale_body_hit".to_owned())?;
                    let caret = base.block_boundary_caret(&block_id, side, fallback);
                    return Ok(Endpoint::BlockEdge(BlockEdgeEndpoint {
                        block_id,
                        side,
                        visual_affinity: body.visual_affinity,
                        session_id: base
                            .session_id
                            .as_ref()
                            .map(ToString::to_string)
                            .ok_or_else(|| "selection.missing_session_id".to_owned())?,
                        document_generation: base.document_generation,
                        font_generation: base.font_generation,
                        page_index: caret.page_index,
                        x_pt: caret.x_pt,
                        y_pt: (caret.y0_pt + caret.y1_pt) / 2.0,
                        composition_revision: body.composition_revision,
                    }));
                }
                body.map_composition_endpoint(
                    &base,
                    composition_range.as_ref(),
                    &replaced_selection,
                    preedit_len,
                )
                .map(Endpoint::Body)
                .ok_or_else(|| "selection.stale_body_hit".to_owned())
            };
            (remap(anchor)?, remap(focus)?)
        } else {
            (anchor, focus)
        };
        let body_block_edge_range = matches!(
            (&anchor, &focus),
            (
                Endpoint::BlockEdge(_),
                Endpoint::Body(_) | Endpoint::BlockEdge(_)
            ) | (Endpoint::Body(_), Endpoint::BlockEdge(_))
        );
        if body_block_edge_range {
            let snapshot = if composition_hit_active {
                self.base_snapshot()?.clone()
            } else {
                self.current_snapshot()?.clone()
            };
            let validate = |edge: &BlockEdgeEndpoint| {
                edge.document_generation == snapshot.document_generation
                    && edge.font_generation == snapshot.font_generation
                    && snapshot.session_id.as_ref().map(ToString::to_string)
                        == Some(edge.session_id.clone())
                    && block_edge_matches_snapshot(edge, &snapshot, &self.session)
            };
            let as_range_endpoint = |endpoint: &Endpoint| match endpoint {
                Endpoint::Body(body) => Ok(caret_engine::DocumentRangeEndpoint::Text(
                    body.position.clone(),
                )),
                Endpoint::BlockEdge(edge) => {
                    if !validate(edge) {
                        return Err("selection.stale_body_hit".to_owned());
                    }
                    Ok(caret_engine::DocumentRangeEndpoint::BlockEdge {
                        block_id: edge.block_id.clone(),
                        side: edge.side,
                    })
                }
                _ => Err("selection.invalid_pointer_domain".to_owned()),
            };
            if let Endpoint::Body(body) = &anchor
                && body.has_layout_identity()
                && !body.matches_layout(&snapshot)
            {
                return Err("selection.stale_body_hit".to_owned());
            }
            if let Endpoint::Body(body) = &focus
                && body.has_layout_identity()
                && !body.matches_layout(&snapshot)
            {
                return Err("selection.stale_body_hit".to_owned());
            }
            let anchor_endpoint = as_range_endpoint(&anchor)?;
            let focus_endpoint = as_range_endpoint(&focus)?;
            let visual_affinity = |endpoint: &Endpoint| match endpoint {
                Endpoint::Body(body) => body.visual_affinity,
                Endpoint::BlockEdge(edge) => edge.visual_affinity,
                _ => VisualAffinity::Downstream,
            };
            let effect = self
                .session
                .set_pointer_structure_selection(
                    caret_engine::StructureSelection {
                        anchor: anchor_endpoint,
                        focus: focus_endpoint,
                    },
                    visual_affinity(&anchor),
                    visual_affinity(&focus),
                )
                .map_err(|error| format!("selection.block_edge: {error}"))?;
            self.preferred_visual_x_pt = None;
            if effect.composition_changed {
                let anchor_stop = match &anchor {
                    Endpoint::BlockEdge(edge) => {
                        committed_block_edge_drag_stop(&snapshot, edge, &self.session)
                    }
                    Endpoint::Body(body) => {
                        committed_body_drag_stop(&snapshot, &body.position, body.visual_affinity)
                    }
                    _ => None,
                };
                self.clear_composition_state();
                return self.layout_snapshot_json_after_pointer_composition_cancel(anchor_stop);
            }
            return self.interaction_state_json();
        }
        match (anchor, focus) {
            (Endpoint::Body(anchor), Endpoint::Body(focus)) => {
                if !composition_hit_active
                    && (anchor.has_layout_identity() || focus.has_layout_identity())
                {
                    let snapshot = self.current_snapshot()?;
                    if [&anchor, &focus]
                        .into_iter()
                        .any(|body| body.has_layout_identity() && !body.matches_layout(snapshot))
                    {
                        return Err("selection.stale_body_hit".to_owned());
                    }
                }
                let effect = self
                    .session
                    .set_pointer_selection_range(
                        anchor.position.clone(),
                        focus.position,
                        anchor.visual_affinity,
                        focus.visual_affinity,
                    )
                    .map_err(|error| format!("selection.set: {error}"))?;
                self.preferred_visual_x_pt = None;
                if effect.composition_changed {
                    self.clear_composition_state();
                    let snapshot = self.base_snapshot()?;
                    let anchor_stop = committed_body_drag_stop(
                        snapshot,
                        &anchor.position,
                        anchor.visual_affinity,
                    );
                    return self.layout_snapshot_json_after_pointer_composition_cancel(anchor_stop);
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
                let snapshot = if composition_hit_active {
                    self.base_snapshot()?.clone()
                } else {
                    self.current_snapshot()?.clone()
                };
                if !math.matches_layout(&snapshot) || !body.matches_layout(&snapshot) {
                    return Err("selection.stale_pointer_hit".to_owned());
                }
                math.hit(&snapshot)
                    .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                let math_endpoint_value = Endpoint::Math(math.clone());
                let math_forward = math_right_moves_forward(&math_endpoint_value, &snapshot)?;
                let math_endpoint = caret_engine::DocumentRangeEndpoint::InlineMathText {
                    paragraph_id: paragraph_id.clone(),
                    math_id: math_id.clone(),
                    position: math.position.clone(),
                    entry_direction: caret_engine::Direction::Forward,
                    entry_math_forward: math_forward,
                };
                let body_endpoint =
                    caret_engine::DocumentRangeEndpoint::Text(body.position.clone());
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
                    let anchor_stop = if body_is_anchor {
                        committed_body_drag_stop(&snapshot, &body.position, body.visual_affinity)
                    } else {
                        math.committed_anchor_json(&snapshot)
                    };
                    self.clear_composition_state();
                    return self.layout_snapshot_json_after_pointer_composition_cancel(anchor_stop);
                }
            }
            (Endpoint::Math(math), Endpoint::MathTree(tree))
            | (Endpoint::MathTree(tree), Endpoint::Math(math)) => {
                let snapshot = if composition_hit_active {
                    self.base_snapshot()?
                } else {
                    self.current_snapshot()?
                };
                if !math.matches_layout(snapshot)
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
                math.hit(snapshot)
                    .ok_or_else(|| "selection.stale_math_hit".to_owned())?;
                let MathEditTarget::Inline {
                    paragraph_id: math_paragraph_id,
                    math_id,
                } = &math.target
                else {
                    return Err("selection.invalid_math_target".to_owned());
                };
                let (tree_endpoint, display_tree_as_block_edge) = match &tree.target {
                    MathEditTarget::Inline {
                        paragraph_id: tree_paragraph_id,
                        ..
                    } => (
                        caret_engine::DocumentRangeEndpoint::InlineMathTree {
                            paragraph_id: tree_paragraph_id.clone(),
                            position: tree.position.clone(),
                            entry_direction: caret_engine::Direction::Forward,
                            entry_math_forward: true,
                        },
                        false,
                    ),
                    MathEditTarget::Block { block_id, .. } => {
                        if snapshot
                            .math_target_cell_id(tree.page_index, &tree.target)
                            .is_none()
                            || !snapshot
                                .math_tree_drag_stops(tree.page_index)
                                .iter()
                                .any(|stop| {
                                    stop.target == tree.target && stop.position == tree.position
                                })
                        {
                            return Err("selection.invalid_math_target".to_owned());
                        }
                        (
                            caret_engine::DocumentRangeEndpoint::BlockEdge {
                                block_id: block_id.clone(),
                                side: math_tree_block_edge_side(&tree.position),
                            },
                            true,
                        )
                    }
                };
                let math_direction_endpoint = if math_is_anchor && !display_tree_as_block_edge {
                    Endpoint::MathTree(tree.clone())
                } else {
                    Endpoint::Math(math.clone())
                };
                let math_forward = math_right_moves_forward(&math_direction_endpoint, snapshot)?;
                let math_endpoint = caret_engine::DocumentRangeEndpoint::InlineMathText {
                    paragraph_id: math_paragraph_id.clone(),
                    math_id: math_id.clone(),
                    position: math.position.clone(),
                    entry_direction: caret_engine::Direction::Forward,
                    entry_math_forward: math_forward,
                };
                let tree_endpoint = with_math_forward(tree_endpoint, math_forward);
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
                let anchor_stop = if composition_hit_active {
                    if math_is_anchor {
                        math.committed_anchor_json(snapshot)
                    } else {
                        committed_math_tree_drag_stop(snapshot, &tree)
                    }
                } else {
                    None
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
                    return self.layout_snapshot_json_after_pointer_composition_cancel(anchor_stop);
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
                        if endpoint.composition_revision.is_some() {
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
                anchor @ (Endpoint::Body(_) | Endpoint::BlockEdge(_) | Endpoint::MathTree(_)),
                focus @ (Endpoint::Body(_) | Endpoint::BlockEdge(_) | Endpoint::MathTree(_)),
            ) => {
                let snapshot = if composition_hit_active {
                    self.base_snapshot()?.clone()
                } else {
                    self.current_snapshot()?.clone()
                };
                let (first_tree, second_tree) = match (&anchor, &focus) {
                    (Endpoint::Body(_), Endpoint::MathTree(tree))
                    | (Endpoint::MathTree(tree), Endpoint::Body(_))
                    | (Endpoint::BlockEdge(_), Endpoint::MathTree(tree))
                    | (Endpoint::MathTree(tree), Endpoint::BlockEdge(_)) => (tree, None),
                    (Endpoint::MathTree(first), Endpoint::MathTree(second)) => {
                        (first, Some(second))
                    }
                    _ => return Err("selection.invalid_math_target".to_owned()),
                };
                let target = first_tree.target.clone();
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
                } && !body.matches_layout(&snapshot)
                {
                    return Err("selection.stale_body_hit".to_owned());
                }
                if let Some(edge) = match (&anchor, &focus) {
                    (Endpoint::BlockEdge(edge), _) | (_, Endpoint::BlockEdge(edge)) => Some(edge),
                    _ => None,
                } && (edge.document_generation != snapshot.document_generation
                    || edge.font_generation != snapshot.font_generation
                    || snapshot.session_id.as_ref().map(ToString::to_string)
                        != Some(edge.session_id.clone())
                    || !block_edge_matches_snapshot(edge, &snapshot, &self.session))
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
                    Endpoint::BlockEdge(edge) => edge.visual_affinity,
                    Endpoint::MathTree(tree) => tree.visual_affinity,
                    Endpoint::Math(_) => VisualAffinity::Downstream,
                };
                let as_range_endpoint = |endpoint: &Endpoint, direction| match endpoint {
                    Endpoint::Body(body) => Ok(caret_engine::DocumentRangeEndpoint::Text(
                        body.position.clone(),
                    )),
                    Endpoint::BlockEdge(edge) => {
                        Ok(caret_engine::DocumentRangeEndpoint::BlockEdge {
                            block_id: edge.block_id.clone(),
                            side: edge.side,
                        })
                    }
                    Endpoint::MathTree(tree) => match &tree.target {
                        MathEditTarget::Inline { paragraph_id, .. } => {
                            Ok(caret_engine::DocumentRangeEndpoint::InlineMathTree {
                                paragraph_id: paragraph_id.clone(),
                                position: tree.position.clone(),
                                entry_direction: direction,
                                entry_math_forward: true,
                            })
                        }
                        MathEditTarget::Block { block_id, .. } => {
                            Ok(caret_engine::DocumentRangeEndpoint::BlockEdge {
                                block_id: block_id.clone(),
                                side: math_tree_block_edge_side(&tree.position),
                            })
                        }
                    },
                    Endpoint::Math(_) => Err("selection.invalid_pointer_domain".to_owned()),
                };
                let focus_endpoint = if matches!(&focus, Endpoint::MathTree(_)) {
                    &focus
                } else {
                    &anchor
                };
                let math_forward = math_right_moves_forward(focus_endpoint, &snapshot)?;
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
                let visual_affinities = (visual_affinity(&anchor), visual_affinity(&focus));
                let effect = self
                    .session
                    .set_pointer_inline_math_selection(
                        range,
                        visual_affinities.0,
                        visual_affinities.1,
                    )
                    .map_err(|error| format!("selection.inline_math: {error}"))?;
                self.preferred_visual_x_pt = None;
                if effect.composition_changed {
                    let anchor_stop = match &anchor {
                        Endpoint::Body(body) => committed_body_drag_stop(
                            &snapshot,
                            &body.position,
                            body.visual_affinity,
                        ),
                        Endpoint::BlockEdge(edge) => {
                            committed_block_edge_drag_stop(&snapshot, edge, &self.session)
                        }
                        Endpoint::MathTree(tree) => committed_math_tree_drag_stop(&snapshot, tree),
                        Endpoint::Math(_) => None,
                    };
                    self.clear_composition_state();
                    return self.layout_snapshot_json_after_pointer_composition_cancel(anchor_stop);
                }
            }
            _ => return Err("selection.invalid_pointer_domain".to_owned()),
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
    composition_revision: Option<u64>,
}

#[derive(Clone)]
struct BlockEdgeEndpoint {
    block_id: caret_doc::NodeId,
    side: caret_engine::Direction,
    visual_affinity: VisualAffinity,
    session_id: String,
    document_generation: u64,
    font_generation: u64,
    page_index: usize,
    x_pt: f64,
    y_pt: f64,
    composition_revision: Option<u64>,
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
    x_pt: f64,
    y_pt: f64,
    composition_revision: Option<u64>,
}

/// 将显示公式树停靠侧别映射为所属块的共享结构边缘。
fn math_tree_block_edge_side(position: &MathTreePosition) -> caret_engine::Direction {
    if position.offset == 0 && position.affinity == MathPositionAffinity::Downstream {
        caret_engine::Direction::Backward
    } else {
        caret_engine::Direction::Forward
    }
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
    let index = snapshot.paragraph_index_by_id(paragraph_id)?;
    Some((index, snapshot.paragraphs.get(index)?))
}

/// 在已提交布局中投影逻辑锚点，避免预编辑收起后的屏幕坐标改写锚点。
fn committed_body_drag_stop(
    snapshot: &DocumentLayoutSnapshot,
    position: &Pos,
    visual_affinity: VisualAffinity,
) -> Option<serde_json::Value> {
    let (paragraph_order, paragraph) = body_fragment_paragraph(snapshot, &position.paragraph_id)?;
    let caret = snapshot.caret_geometry(position, visual_affinity)?;
    let context = BodyFragmentContext {
        snapshot,
        page_index: caret.page_index,
        paragraph_order,
        paragraph,
        grapheme_offsets: &[],
        composition_revision: None,
    };
    Some(body_caret_stop_json(
        &context,
        position.offset,
        position.affinity,
        visual_affinity,
        caret,
    ))
}

/// 已提交快照中的块边缘命中必须重投影，不能依赖预览中即将消失的临时段坐标。
fn committed_block_edge_drag_stop(
    snapshot: &DocumentLayoutSnapshot,
    endpoint: &BlockEdgeEndpoint,
    session: &caret_engine::Session,
) -> Option<serde_json::Value> {
    let fallback = snapshot.caret_geometry(
        &session.selection().focus,
        session.selection_visual_affinity(),
    )?;
    let caret = snapshot.block_boundary_caret(&endpoint.block_id, endpoint.side, fallback);
    Some(block_edge_drag_stop_json(
        snapshot,
        &endpoint.block_id,
        endpoint.side,
        endpoint.visual_affinity,
        caret,
        None,
    ))
}

/// 预编辑取消后重投影版本化的公式树锚点，供 QML 在新布局中继续拖选。
fn committed_math_tree_drag_stop(
    snapshot: &DocumentLayoutSnapshot,
    endpoint: &MathTreeEndpoint,
) -> Option<serde_json::Value> {
    let stop = snapshot
        .math_tree_drag_stops(endpoint.page_index)
        .into_iter()
        .find(|stop| stop.target == endpoint.target && stop.position == endpoint.position)?;
    Some(math_tree_drag_stop_json(
        snapshot,
        endpoint.page_index,
        &stop,
        None,
    ))
}

/// 在当前快照重算块边缘几何，核对停止点身份和页面列是否仍有效。
fn block_edge_matches_snapshot(
    endpoint: &BlockEdgeEndpoint,
    snapshot: &DocumentLayoutSnapshot,
    _session: &caret_engine::Session,
) -> bool {
    if snapshot.pages.get(endpoint.page_index).is_none() {
        return false;
    }
    let fallback = PageCaretGeometry {
        page_index: endpoint.page_index,
        x_pt: endpoint.x_pt,
        y0_pt: endpoint.y_pt,
        y1_pt: endpoint.y_pt,
    };
    let caret = snapshot.block_boundary_caret(&endpoint.block_id, endpoint.side, fallback);
    caret.page_index == endpoint.page_index && (caret.x_pt - endpoint.x_pt).abs() < 0.01
}

/// 构造共享正文拖选域的块边缘停靠点，并携带可拒绝过期预览的布局身份。
fn block_edge_drag_stop_json(
    snapshot: &DocumentLayoutSnapshot,
    block_id: &caret_doc::NodeId,
    side: caret_engine::Direction,
    visual_affinity: VisualAffinity,
    caret: PageCaretGeometry,
    composition_revision: Option<u64>,
) -> serde_json::Value {
    let mut position = serde_json::json!({
        "kind": "blockEdge",
        "domainId": "body",
        "sessionId": snapshot.session_id.as_ref().map(ToString::to_string),
        "pageIndex": caret.page_index,
        "xPt": caret.x_pt,
        "yPt": (caret.y0_pt + caret.y1_pt) / 2.0,
        "documentGeneration": snapshot.document_generation.to_string(),
        "fontGeneration": snapshot.font_generation.to_string(),
        "blockId": block_id.to_string(),
        "side": if side == caret_engine::Direction::Backward { "backward" } else { "forward" },
        "visualAffinity": visual_affinity_name(visual_affinity),
    });
    if let Some(revision) = composition_revision {
        position["compositionRevision"] = serde_json::json!(revision.to_string());
    }
    serde_json::json!({
        "domainId": "body",
        "cellId": "",
        "pageIndex": caret.page_index,
        "xPt": caret.x_pt,
        "y0Pt": caret.y0_pt,
        "y1Pt": caret.y1_pt,
        "position": position,
        "visualAffinity": visual_affinity_name(visual_affinity),
    })
}

/// 返回正文停靠所属单元格；单元格 ID 由版面命中与光标条目共同使用。
fn paragraph_cell_id(paragraph: &LaidOutParagraph) -> Option<String> {
    paragraph
        .container
        .innermost_cell()
        .map(|cell| cell.cell_id.to_string())
}

/// 将同页公式投影的单元格标识转换为拖选线格式。
fn target_cell_id(
    snapshot: &DocumentLayoutSnapshot,
    page_index: usize,
    target: &MathEditTarget,
) -> Option<String> {
    snapshot
        .math_target_cell_id(page_index, target)
        .map(ToString::to_string)
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
    let mut position = serde_json::json!({
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
    });
    if let Some(revision) = context.composition_revision {
        position["compositionRevision"] = serde_json::json!(revision.to_string());
    }
    serde_json::json!({
        "domainId": "body",
        "cellId": paragraph_cell_id(context.paragraph).unwrap_or_default(),
        "pageIndex": context.page_index,
        "xPt": caret.x_pt,
        "y0Pt": caret.y0_pt,
        "y1Pt": caret.y1_pt,
        "position": position,
        "visualAffinity": if visual_affinity == VisualAffinity::Upstream { "upstream" } else { "downstream" },
    })
}

/// 收集数学文本拖选停靠条目，并按域归并岛内字素点。
fn math_drag_stop_entries(
    snapshot: &DocumentLayoutSnapshot,
    page_index: usize,
    composition_revision: Option<u64>,
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
        stops.push(math_hit_stop_json(
            snapshot,
            page_index,
            &stop,
            &domain_id,
            composition_revision,
        ));
    }
    (stops, islands)
}

/// 构造单条数学文本光标停靠条目，内嵌位置字段与拖选协议保持一致。
fn math_hit_stop_json(
    snapshot: &DocumentLayoutSnapshot,
    page_index: usize,
    stop: &PageMathTextDragStop,
    domain_id: &str,
    composition_revision: Option<u64>,
) -> serde_json::Value {
    let mut position = serde_json::json!({
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
    });
    if let Some(revision) = composition_revision {
        position["compositionRevision"] = serde_json::json!(revision.to_string());
    }
    serde_json::json!({
        "domainId": domain_id,
        "cellId": target_cell_id(snapshot, page_index, &stop.target).unwrap_or_default(),
        "pageIndex": page_index,
        "xPt": stop.caret.x_pt,
        "y0Pt": stop.caret.y0_pt,
        "y1Pt": stop.caret.y1_pt,
        "position": position,
        "visualAffinity": visual_affinity_name(stop.visual_affinity),
    })
}

/// 构造可由指针提交的公式树停靠点，并保留精确的序列路径。
fn math_tree_drag_stop_json(
    snapshot: &DocumentLayoutSnapshot,
    page_index: usize,
    stop: &PageMathTreeDragStop,
    composition_revision: Option<u64>,
) -> serde_json::Value {
    let mut position = serde_json::json!({
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
        "treeHost": math_tree_host_json(&stop.position.host),
        "treePath": stop.position.path.steps().iter().map(math_tree_path_step_json).collect::<Vec<_>>(),
    });
    if let Some(revision) = composition_revision {
        position["compositionRevision"] = serde_json::json!(revision.to_string());
    }
    serde_json::json!({
        "domainId": "body",
        "cellId": target_cell_id(snapshot, page_index, &stop.target).unwrap_or_default(),
        "pageIndex": page_index,
        "xPt": stop.caret.x_pt,
        "y0Pt": stop.caret.y0_pt,
        "y1Pt": stop.caret.y1_pt,
        "position": position,
        "visualAffinity": visual_affinity_name(stop.visual_affinity),
    })
}

fn math_tree_host_json(host: &caret_engine::MathPositionHost) -> serde_json::Value {
    match host {
        caret_engine::MathPositionHost::InlineMath { owner_id } => serde_json::json!({
            "kind": "inlineMath",
            "ownerId": owner_id,
        }),
        caret_engine::MathPositionHost::Equation { owner_id } => serde_json::json!({
            "kind": "equation",
            "ownerId": owner_id,
        }),
        caret_engine::MathPositionHost::AlignCell { row_id, cell_id } => serde_json::json!({
            "kind": "alignCell",
            "rowId": row_id,
            "cellId": cell_id,
        }),
    }
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
