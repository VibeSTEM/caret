//! 删除结构选区覆盖的块节点与文字，并作为单一事务提交。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::document_position::visual_affinity_for;
use super::super::formatting::PendingInlineStyle;
use super::super::math_edit::find_block_path;
use super::super::{DocumentRangeEndpoint, MathEdit, StructureSelection};
use super::*;
use crate::editing::paragraph_ops::{
    ParagraphIndex, ParagraphLocation, build_paragraph_index, merge_paragraph_range,
    paragraph_at_location_mut,
};
use crate::editing::session::history::{self, ParagraphState};
use caret_doc::Document;
use caret_structure::{DocumentNodeKind, find_document_node};
use std::collections::VecDeque;

/// 结构范围删除计划：先完整校验端点、覆盖节点与文字切口，再一次性提交。
///
/// 节点移除 patch 按同级下标降序排列，顺序应用即可保持下标有效；段落 `before`
/// 快照取自节点移除后的文档，`after` 快照取自文字合并/截断后的文档，因此两端各自
/// 对应一侧的块布局，不依赖被删除节点的原始下标。范围两端都是普通文本段落时按
/// 纯文本跨段删除语义合并，`before` 快照还包含合并时删除的终点与中间段落。
struct StructureRangeDeletion {
    node_patches: Vec<DocumentPatch>,
    /// 仅移除覆盖节点（及补位空段落）后的文档，范围文字尚未截断。
    document_after_nodes: Document,
    /// 最终文档：节点移除后再截断范围文字。
    document: Document,
    before_paragraphs: Vec<ParagraphState>,
    after_paragraphs: Vec<ParagraphState>,
    caret: Pos,
}

/// 范围文字处理结果：最终文档与 `Paragraphs` 补丁两侧快照所需的段落 ID。
struct StructureRangeTextPlan {
    document: Document,
    /// 节点移除后文档中需要 `before` 快照的段落，包含因合并或中间段移除而删除的段落。
    before_ids: Vec<NodeId>,
    /// 最终文档中保留并需要 `after` 快照的段落。
    surviving_ids: Vec<NodeId>,
}

/// 对象删除语义下的文字切口与需整段移除的中间段落。
struct StructureRangeTextEdits {
    text_edits: Vec<(NodeId, usize, usize)>,
    removed_ids: Vec<NodeId>,
}

impl Session {
    /// 删除结构选区覆盖的文字与块节点，并作为一个历史操作提交。
    ///
    /// 覆盖的 Section/Custom 以整棵环境删除，Equation/Align 作为原子块删除；范围
    /// 中间与终点侧的普通文本段落按纯文本跨段删除语义并入起点段落。端点不是文本
    /// （含块外侧边界）或两端结构上不可合并（不同容器、标题段落、中间夹有不可合并
    /// 块等）时退回对象删除语义，仅截断两端文字并移除中间段落。校验失败时文档、
    /// 选区、代际与历史均不改变。
    pub(in crate::editing::session) fn delete_selected_structure_range(
        &mut self,
    ) -> Result<Option<Effect>, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(None);
        }
        let Some(plan) = self.plan_structure_range_deletion()? else {
            return Ok(None);
        };
        let mut patches = plan.node_patches;
        // 合并或中间段删除可能只登记 before 快照（after 为空即删除段落），因此不能
        // 只在 after 非空时补丁；否则重做无法删除段落、撤销也会与文档状态不一致。
        if !plan.before_paragraphs.is_empty() || !plan.after_paragraphs.is_empty() {
            patches.push(DocumentPatch::Paragraphs {
                before: plan.before_paragraphs,
                after: plan.after_paragraphs,
                merge_group: None,
                merge_epoch: self.history_merge_epoch,
            });
        }
        let before = self.capture_structure_edit_before_state();
        self.park_structure_range_caret(&plan.caret);
        self.document = plan.document;
        self.install_structure_range_edit(patches, before);
        Ok(Some(Effect {
            document_changed: true,
            selection_changed: true,
            ..Effect::default()
        }))
    }

    /// 用一次可编辑操作替换整个结构选区，并合并为一个历史操作。
    ///
    /// 覆盖节点与范围文字先由同一删除计划移除，随后把落点交给既有编辑入口执行，
    /// 因此不需要在替换路径复制文本插入逻辑。返回 `None` 表示没有可删除范围，
    /// 调用方继续按普通编辑处理。
    pub(in crate::editing::session) fn replace_structure_selection(
        &mut self,
        insert: impl FnOnce(&mut Session) -> Result<Effect, EditError>,
    ) -> Result<Option<Effect>, EditError> {
        self.replace_structure_selection_inner(insert, false)
    }

    /// 用 IME 提交结果替换结构选区；组合输入在提交时仍处于活动状态，因此允许它参与事务。
    pub(in crate::editing::session) fn commit_composition_replacing_structure(
        &mut self,
        id: u64,
        text: &str,
    ) -> Result<Option<Effect>, EditError> {
        self.replace_structure_selection_inner(|session| session.commit_composition(id, text), true)
    }

    fn replace_structure_selection_inner(
        &mut self,
        insert: impl FnOnce(&mut Session) -> Result<Effect, EditError>,
        allow_composition: bool,
    ) -> Result<Option<Effect>, EditError> {
        if self.command_line.is_some() || (!allow_composition && self.composition.is_some()) {
            return Ok(None);
        }
        let Some(plan) = self.plan_structure_range_deletion()? else {
            return Ok(None);
        };
        // 替换事务还要把写入落点段落纳入段落快照，范围截断与后续插入共享一条记录。
        let mut before_ids = plan
            .before_paragraphs
            .iter()
            .map(|state| state.id.clone())
            .collect::<Vec<_>>();
        if !before_ids.contains(&plan.caret.paragraph_id) {
            before_ids.push(plan.caret.paragraph_id.clone());
        }
        let after_nodes_index = build_paragraph_index(&plan.document_after_nodes.body);
        let before_paragraphs = history::paragraph_states_for_ids(
            &plan.document_after_nodes,
            &after_nodes_index,
            before_ids.clone(),
        );
        if before_paragraphs.len() != before_ids.len() {
            return Err(EditError::InvalidPosition);
        }

        let before = self.capture_structure_edit_before_state();
        let restore = self.capture_structure_edit_restore_point();
        self.document = plan.document.clone();
        self.paragraph_index = build_paragraph_index(&self.document.body);
        self.structure_index = Arc::new(StructureIndex::build(&self.document));
        self.park_structure_range_caret(&plan.caret);

        let inserted = match insert(self) {
            Ok(effect) => effect,
            Err(error) => {
                self.restore_structure_edit_restore_point(restore);
                return Err(error);
            }
        };
        let mut after_ids = before_ids;
        if let Some(map) = &inserted.position_map {
            history::state_ids_from_position_map(map, &mut after_ids);
        }
        // 插入入口可能重建段落索引；这里按最终文档重新取一遍快照。
        self.paragraph_index = build_paragraph_index(&self.document.body);
        let after_paragraphs =
            history::paragraph_states_for_ids(&self.document, &self.paragraph_index, after_ids);
        let mut patches = plan.node_patches;
        // 插入没有改变任何段落内容时不必登记段落补丁，否则复合撤销会出现空步。
        if before_paragraphs != after_paragraphs {
            patches.push(DocumentPatch::Paragraphs {
                before: before_paragraphs,
                after: after_paragraphs,
                merge_group: None,
                merge_epoch: self.history_merge_epoch,
            });
        }
        if inserted.selection_changed {
            self.selection_visual_affinity = visual_affinity_for(self.selection.focus.affinity);
            if self.selection.anchor == self.selection.focus {
                self.selection_anchor_visual_affinity = self.selection_visual_affinity;
            }
        }
        self.install_structure_range_edit(patches, before);
        Ok(Some(Effect {
            document_changed: true,
            selection_changed: true,
            position_map: inserted.position_map,
            ..Effect::default()
        }))
    }

    /// 记录结构编辑前的选区、视觉侧与文档边界停靠，供历史回退精确恢复。
    fn capture_structure_edit_before_state(&self) -> StructureEditBeforeState {
        StructureEditBeforeState {
            selection: self.selection.clone(),
            anchor_visual_affinity: self.selection_anchor_visual_affinity,
            visual_affinity: self.selection_visual_affinity,
            state_id: self.history_state_id,
            host_caret_position: self.host_caret_position.clone(),
        }
    }

    /// 替换事务失败时用于原子回滚的文档、交互与历史记账快照。
    fn capture_structure_edit_restore_point(&self) -> StructureEditRestorePoint {
        StructureEditRestorePoint {
            document: self.document.clone(),
            paragraph_index: self.paragraph_index.clone(),
            structure_index: self.structure_index.clone(),
            text_map_cache: self.text_map_cache.clone(),
            selection: self.selection.clone(),
            selection_anchor_visual_affinity: self.selection_anchor_visual_affinity,
            selection_visual_affinity: self.selection_visual_affinity,
            math_edit: self.math_edit.clone(),
            selected_block_environments: self.selected_block_environments.clone(),
            selected_math_blocks: self.selected_math_blocks.clone(),
            structure_selection: self.structure_selection.clone(),
            host_caret_position: self.host_caret_position.clone(),
            pending_inline_style: self.pending_inline_style.clone(),
            history_state_id: self.history_state_id,
            history_merge_epoch: self.history_merge_epoch,
            document_generation: self.document_generation,
        }
    }

    fn restore_structure_edit_restore_point(&mut self, restore: StructureEditRestorePoint) {
        self.document = restore.document;
        self.paragraph_index = restore.paragraph_index;
        self.structure_index = restore.structure_index;
        self.text_map_cache = restore.text_map_cache;
        self.selection = restore.selection;
        self.selection_anchor_visual_affinity = restore.selection_anchor_visual_affinity;
        self.selection_visual_affinity = restore.selection_visual_affinity;
        self.math_edit = restore.math_edit;
        self.selected_block_environments = restore.selected_block_environments;
        self.selected_math_blocks = restore.selected_math_blocks;
        self.structure_selection = restore.structure_selection;
        self.host_caret_position = restore.host_caret_position;
        self.pending_inline_style = restore.pending_inline_style;
        self.history_state_id = restore.history_state_id;
        self.history_merge_epoch = restore.history_merge_epoch;
        self.document_generation = restore.document_generation;
    }

    /// 提交已校验的结构范围结果，并只登记一条历史记录。
    fn install_structure_range_edit(
        &mut self,
        patches: Vec<DocumentPatch>,
        before: StructureEditBeforeState,
    ) {
        let updated_index = build_paragraph_index(&self.document.body);
        self.finalize_structure_edit_with_boundary(
            updated_index,
            DocumentPatch::Compound(patches),
            before,
        );
    }

    /// 把结构范围落点安装为折叠选区，并清除结构选区与块外停靠。
    fn park_structure_range_caret(&mut self, caret: &Pos) {
        self.selection = Selection {
            anchor: caret.clone(),
            focus: caret.clone(),
        };
        self.selection_visual_affinity = visual_affinity_for(caret.affinity);
        self.selection_anchor_visual_affinity = self.selection_visual_affinity;
        self.math_edit = None;
        self.clear_structure_selection_state();
        self.host_caret_position = None;
        self.text_map_cache.clear();
    }

    /// 规划结构范围删除；没有结构选区时返回 `None`。
    fn plan_structure_range_deletion(&self) -> Result<Option<StructureRangeDeletion>, EditError> {
        let Some((start, end)) = self.ordered_range_endpoints()? else {
            return Ok(None);
        };
        let (environments, math_blocks) = self.structure_range_block_nodes()?;
        let text_bounds = self.usable_structure_text_bounds()?;

        let mut node_patches = self.structure_node_deletion_patches(&environments, &math_blocks)?;
        let mut document_after_nodes = self.document.clone();
        for patch in &node_patches {
            if !patch.redo(&mut document_after_nodes) {
                return Err(EditError::InvalidPosition);
            }
        }
        let fallback_paragraph_id = build_paragraph_index(&document_after_nodes.body)
            .is_empty()
            .then(|| {
                let paragraph = Box::new(Paragraph::empty(NodeId::generate()));
                let sibling_index = document_after_nodes.body.len();
                document_after_nodes
                    .body
                    .push(Block::Paragraph((*paragraph).clone()));
                node_patches.push(DocumentPatch::ParagraphInsertion {
                    parent_environment_ids: Vec::new(),
                    sibling_index,
                    paragraph: paragraph.clone(),
                });
                paragraph.id.clone()
            });
        document_after_nodes
            .validate()
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;

        let after_nodes_index = build_paragraph_index(&document_after_nodes.body);
        let StructureRangeTextPlan {
            mut document,
            before_ids,
            surviving_ids,
        } = self.plan_structure_range_text(
            &start,
            &end,
            &text_bounds,
            &environments,
            &document_after_nodes,
        )?;
        // 移除范围中间的普通段落也可能再次清空文档；与节点删除一样补一个可编辑空段落。
        let fallback_paragraph_id = if build_paragraph_index(&document.body).is_empty() {
            let paragraph = Box::new(Paragraph::empty(NodeId::generate()));
            // 补位段落追加在节点删除后文档的末尾，重放顺序才与 before 快照一致。
            let sibling_index = document_after_nodes.body.len();
            document.body.push(Block::Paragraph((*paragraph).clone()));
            document_after_nodes
                .body
                .push(Block::Paragraph((*paragraph).clone()));
            node_patches.push(DocumentPatch::ParagraphInsertion {
                parent_environment_ids: Vec::new(),
                sibling_index,
                paragraph: paragraph.clone(),
            });
            Some(paragraph.id.clone())
        } else {
            fallback_paragraph_id
        };
        if node_patches.is_empty() && before_ids.is_empty() {
            return Ok(None);
        }
        document
            .validate()
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;

        let before_paragraphs = history::paragraph_states_for_ids(
            &document_after_nodes,
            &after_nodes_index,
            before_ids.clone(),
        );
        if before_paragraphs.len() != before_ids.len() {
            return Err(EditError::InvalidPosition);
        }
        let after_index = build_paragraph_index(&document.body);
        let after_paragraphs =
            history::paragraph_states_for_ids(&document, &after_index, surviving_ids.clone());
        if after_paragraphs.len() != surviving_ids.len() {
            return Err(EditError::InvalidPosition);
        }
        let caret = self.structure_range_caret(
            &start,
            &end,
            &text_bounds,
            &document,
            &after_index,
            &fallback_paragraph_id,
        )?;
        Ok(Some(StructureRangeDeletion {
            node_patches,
            document_after_nodes,
            document,
            before_paragraphs,
            after_paragraphs,
            caret,
        }))
    }

    /// 在节点移除后的文档上规划范围文字处理，返回结果文档与段落补丁所需 ID。
    ///
    /// 有序范围两端都是同一正文容器内的普通文本段落时，按纯文本跨段删除语义合并：
    /// 保留起点段落，删除起点段落尾部与终点段落头部，把终点段落剩余 inlines 拼到
    /// 起点段落之后，并移除终点与全部中间段落。其余情况保持对象删除语义：只截断
    /// 两端文字，并把范围中间的普通段落整段移除。`before_ids` 取自节点移除后的
    /// 文档（包含被删除段落），`surviving_ids` 只包含最终文档中保留的段落，两者共同
    /// 让 `Paragraphs` 补丁能够删除与恢复段落。
    fn plan_structure_range_text(
        &self,
        start: &DocumentRangeEndpoint,
        end: &DocumentRangeEndpoint,
        text_bounds: &Option<(Pos, Pos, usize, usize)>,
        environments: &[NodeId],
        document_after_nodes: &Document,
    ) -> Result<StructureRangeTextPlan, EditError> {
        let after_nodes_index = build_paragraph_index(&document_after_nodes.body);
        let mut document = document_after_nodes.clone();
        let merge_locations = structure_range_merge_locations(start, end, &after_nodes_index);
        if let Some((start_location, end_location)) = merge_locations {
            let (start_position, end_position) = match text_bounds {
                Some((start_position, end_position, _, _)) => (start_position, end_position),
                None => return Err(EditError::InvalidPosition),
            };
            match merge_paragraph_range(
                &mut document.body,
                &start_location,
                start_position.offset,
                &end_location,
                end_position.offset,
            ) {
                Ok(result) => {
                    let mut before_ids = Vec::with_capacity(result.removed_ids.len() + 1);
                    before_ids.push(result.paragraph_id.clone());
                    before_ids.extend(result.removed_ids.iter().cloned());
                    return Ok(StructureRangeTextPlan {
                        document,
                        before_ids,
                        surviving_ids: vec![result.paragraph_id],
                    });
                }
                Err(EditError::CrossParagraphSelection) => {
                    // 结构上不能合并（例如两端不在同一容器、端点含标题，或中间夹有
                    // 不可合并块）时退回对象删除语义；合并函数在写入前拒绝，文档未变。
                }
                Err(error) => return Err(error),
            }
        }
        let StructureRangeTextEdits {
            text_edits,
            removed_ids,
        } = self.structure_range_text_plan(start, end, text_bounds, environments)?;
        for (paragraph_id, from, to) in &text_edits {
            let location = after_nodes_index
                .get(paragraph_id)
                .cloned()
                .ok_or(EditError::InvalidPosition)?;
            let current = paragraph_at_location(&document.body, &location)
                .ok_or(EditError::InvalidPosition)?
                .clone();
            let map = TextMap::new(&current)?;
            let mut updated = current;
            map.delete_range(&mut updated, *from, *to)?;
            let slot = paragraph_at_location_mut(&mut document.body, &location)
                .ok_or(EditError::InvalidPosition)?;
            *slot = updated;
        }
        remove_range_paragraphs(&mut document, &after_nodes_index, &removed_ids)?;
        let mut before_ids = text_edits
            .iter()
            .map(|(paragraph_id, _, _)| paragraph_id.clone())
            .collect::<Vec<_>>();
        before_ids.extend(removed_ids);
        let surviving_ids = text_edits
            .iter()
            .map(|(paragraph_id, _, _)| paragraph_id.clone())
            .collect();
        Ok(StructureRangeTextPlan {
            document,
            before_ids,
            surviving_ids,
        })
    }

    // 合并可行性由 `paragraph_ops::merge_paragraph_range` 单一判定：这里只把两端
    // 文本端点解析成节点移除后的文档位置，不同容器、标题段落、样式不兼容、未知
    // opaque 内容等条件全部交给合并函数返回 `CrossParagraphSelection` 或具体错误。

    /// 当前结构选区覆盖的块节点，按文档顺序返回最外层环境与未被环境包含的公式块。
    ///
    /// 选区安装时已从范围派生这两个列表；这里再按物理树校验节点身份并裁掉被选中
    /// 环境的后代，避免同一祖先子树被删除两次。
    pub(in crate::editing::session) fn structure_range_block_nodes(
        &self,
    ) -> Result<(Vec<NodeId>, Vec<NodeId>), EditError> {
        let mut environments = Vec::with_capacity(self.selected_block_environments.len());
        for id in &self.selected_block_environments {
            let location =
                find_document_node(&self.document, id).ok_or(EditError::InvalidPosition)?;
            if location.kind != DocumentNodeKind::Environment {
                return Err(EditError::InvalidPosition);
            }
            environments.push(id.clone());
        }
        // 只保留最外层环境：删除父环境已包含全部后代。
        let outermost = environments
            .iter()
            .filter(|id| {
                find_document_node(&self.document, id).is_some_and(|location| {
                    !location
                        .parent_environment_ids
                        .iter()
                        .any(|parent| environments.contains(parent))
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut math_blocks = Vec::with_capacity(self.selected_math_blocks.len());
        for id in &self.selected_math_blocks {
            let location =
                find_document_node(&self.document, id).ok_or(EditError::InvalidPosition)?;
            if !location.kind.is_math_block() {
                return Err(EditError::InvalidPosition);
            }
            if location
                .parent_environment_ids
                .iter()
                .any(|parent| outermost.contains(parent))
            {
                continue;
            }
            math_blocks.push(id.clone());
        }
        Ok((outermost, math_blocks))
    }

    /// 按文档顺序返回结构选区两端；没有结构选区时返回 `None`。
    fn ordered_range_endpoints(
        &self,
    ) -> Result<Option<(DocumentRangeEndpoint, DocumentRangeEndpoint)>, EditError> {
        if self.structure_selection.is_none() {
            return Ok(None);
        }
        let (start, end) = self.ordered_structure_selection()?;
        Ok(Some((start.clone(), end.clone())))
    }

    /// 返回可按文档顺序使用的文本切口与端点段落序号。
    ///
    /// 仅覆盖一个块公式或空环境的范围没有可截断文字，此时两端的最近段落会互相
    /// 交错；这种情况返回 `None`，由覆盖节点单独表达范围。
    pub(in crate::editing::session) fn usable_structure_text_bounds(
        &self,
    ) -> Result<Option<(Pos, Pos, usize, usize)>, EditError> {
        let Some((start_position, end_position)) = self.structure_selection_text_bounds() else {
            return Ok(None);
        };
        let Some(start_ordinal) = self
            .structure_index
            .paragraph_ordinal(&start_position.paragraph_id)
        else {
            return Err(EditError::InvalidPosition);
        };
        let Some(end_ordinal) = self
            .structure_index
            .paragraph_ordinal(&end_position.paragraph_id)
        else {
            return Err(EditError::InvalidPosition);
        };
        Ok((start_ordinal <= end_ordinal).then_some((
            start_position,
            end_position,
            start_ordinal,
            end_ordinal,
        )))
    }

    /// 计算对象删除语义下的文字切口与需整段移除的中间段落。
    ///
    /// 文本端点所在的普通段落只截断保留一侧文字（起点保留前缀、终点保留后缀）；
    /// 完全落在范围内部的普通段落记录为待删除 ID，交给 `Paragraphs` 补丁整段移除。
    /// 端点被规范到相邻段落时（例如块外侧边界投影）该段落只是切口载体，不能仅凭
    /// 序号把它当作端点段落。被覆盖环境内部的段落随环境整体删除，不单独处理。
    fn structure_range_text_plan(
        &self,
        start: &DocumentRangeEndpoint,
        end: &DocumentRangeEndpoint,
        text_bounds: &Option<(Pos, Pos, usize, usize)>,
        environments: &[NodeId],
    ) -> Result<StructureRangeTextEdits, EditError> {
        let Some((_, _, start_ordinal, end_ordinal)) = text_bounds else {
            return Ok(StructureRangeTextEdits {
                text_edits: Vec::new(),
                removed_ids: Vec::new(),
            });
        };
        let start_paragraph = match start {
            DocumentRangeEndpoint::Text(position) => {
                Some((&position.paragraph_id, position.offset))
            }
            DocumentRangeEndpoint::BlockEdge { .. } => None,
        };
        let end_paragraph = match end {
            DocumentRangeEndpoint::Text(position) => {
                Some((&position.paragraph_id, position.offset))
            }
            DocumentRangeEndpoint::BlockEdge { .. } => None,
        };
        let mut text_edits = Vec::new();
        let mut removed_ids = Vec::new();
        for ordinal in *start_ordinal..=*end_ordinal {
            let Some(paragraph_id) = self.structure_index.paragraph_id_at(ordinal).cloned() else {
                continue;
            };
            if self.paragraph_inside_environments(&paragraph_id, environments) {
                continue;
            }
            let text_len = self.paragraph_text_len(&paragraph_id)?;
            let is_start = start_paragraph.is_some_and(|(id, _)| id == &paragraph_id);
            let is_end = end_paragraph.is_some_and(|(id, _)| id == &paragraph_id);
            if is_start || is_end {
                let from = start_paragraph
                    .filter(|(id, _)| *id == &paragraph_id)
                    .map_or(0, |(_, offset)| offset);
                let to = end_paragraph
                    .filter(|(id, _)| *id == &paragraph_id)
                    .map_or(text_len, |(_, offset)| offset);
                let to = to.min(text_len);
                let from = from.min(to);
                if from < to {
                    text_edits.push((paragraph_id, from, to));
                }
                continue;
            }
            let story = self
                .story_for_paragraph(&paragraph_id)
                .ok_or(EditError::InvalidPosition)?;
            let paragraph_start = Pos {
                story: story.clone(),
                paragraph_id: paragraph_id.clone(),
                offset: 0,
                affinity: Affinity::Downstream,
            };
            let paragraph_end = Pos {
                story,
                paragraph_id: paragraph_id.clone(),
                offset: text_len,
                affinity: Affinity::Upstream,
            };
            // 完全包含判定使用严格边界：段落必须至少有一个字素落在范围内部才算范围内
            // 段落。相邻节点的物理边界键相等，零宽空段落又与其相邻块对象的边界重合，
            // 非严格的“起点不早于范围起点”会把紧邻被选对象、实际位于范围之外的空
            // 段落误判为范围内而删除。
            let starts_inside = self
                .compare_range_endpoints(&DocumentRangeEndpoint::Text(paragraph_end), start)?
                .is_gt();
            let ends_inside = self
                .compare_range_endpoints(&DocumentRangeEndpoint::Text(paragraph_start), end)?
                .is_lt();
            if starts_inside && ends_inside {
                removed_ids.push(paragraph_id);
            }
        }
        Ok(StructureRangeTextEdits {
            text_edits,
            removed_ids,
        })
    }

    /// 段落是否位于任一被覆盖环境内部；这类段落不单独截断文字。
    fn paragraph_inside_environments(
        &self,
        paragraph_id: &NodeId,
        environments: &[NodeId],
    ) -> bool {
        let Some(location) = find_document_node(&self.document, paragraph_id) else {
            return true;
        };
        location
            .parent_environment_ids
            .iter()
            .any(|id| environments.contains(id))
    }

    /// 为覆盖的块节点生成删除 patch，按同级下标降序排列以保持下标有效。
    fn structure_node_deletion_patches(
        &self,
        environments: &[NodeId],
        math_blocks: &[NodeId],
    ) -> Result<Vec<DocumentPatch>, EditError> {
        let mut locations: Vec<(Vec<NodeId>, usize, NodeId)> =
            Vec::with_capacity(environments.len() + math_blocks.len());
        for block_id in environments.iter().chain(math_blocks) {
            let (parent_environment_ids, sibling_index) =
                find_block_path(&self.document.body, block_id).ok_or(EditError::InvalidPosition)?;
            locations.push((parent_environment_ids, sibling_index, block_id.clone()));
        }
        locations.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)));
        let mut patches = Vec::with_capacity(locations.len());
        for (parent_environment_ids, sibling_index, block_id) in locations {
            let siblings = block_list_by_parent_ids(&self.document.body, &parent_environment_ids)
                .ok_or(EditError::InvalidPosition)?;
            match siblings.get(sibling_index) {
                Some(Block::Environment(environment))
                    if environments.contains(&block_id) && environment.id == block_id =>
                {
                    patches.push(DocumentPatch::EnvironmentDeletion {
                        parent_environment_ids,
                        sibling_index,
                        environment_estimated_bytes: history::environment_estimated_bytes(
                            environment,
                        ),
                        environment: Box::new(environment.clone()),
                        fallback_paragraph: None,
                    });
                }
                Some(block)
                    if math_blocks.contains(&block_id)
                        && matches!(block, Block::Equation(_) | Block::Align(_)) =>
                {
                    patches.push(DocumentPatch::BlockRange {
                        parent_environment_ids,
                        sibling_index,
                        before: vec![block.clone()],
                        after: Vec::new(),
                    });
                }
                _ => return Err(EditError::InvalidPosition),
            }
        }
        Ok(patches)
    }

    /// 结构范围删除后的折叠落点：优先范围起点一侧，其次范围终点，再按文档顺序取
    /// 最近的幸存段落，最后才使用补位空段落。
    fn structure_range_caret(
        &self,
        start: &DocumentRangeEndpoint,
        end: &DocumentRangeEndpoint,
        text_bounds: &Option<(Pos, Pos, usize, usize)>,
        document: &Document,
        index: &ParagraphIndex,
        fallback_paragraph_id: &Option<NodeId>,
    ) -> Result<Pos, EditError> {
        let mut candidates = Vec::new();
        match start {
            DocumentRangeEndpoint::Text(position) => candidates.push(position.clone()),
            DocumentRangeEndpoint::BlockEdge { .. } => {
                if let Some(position) = self.text_endpoint_clamped(start) {
                    candidates.push(position);
                }
            }
        }
        if let Some(position) = self.text_endpoint_clamped(end) {
            candidates.push(position);
        }
        for candidate in candidates {
            if let Some(position) = clamped_caret(document, index, &candidate) {
                return Ok(position);
            }
        }
        // 端点段落可能随被覆盖环境一起删除；按段落顺序退到最近的幸存段落。
        if let Some((_, _, start_ordinal, _)) = text_bounds {
            let length = self.structure_index.navigation_paragraphs().len();
            for ordinal in *start_ordinal..length {
                let Some(paragraph_id) = self.structure_index.paragraph_id_at(ordinal) else {
                    continue;
                };
                if let Some(location) = index.get(paragraph_id) {
                    return Ok(Pos {
                        story: location.story.clone(),
                        paragraph_id: paragraph_id.clone(),
                        offset: 0,
                        affinity: Affinity::Downstream,
                    });
                }
            }
            for ordinal in (0..*start_ordinal).rev() {
                let Some(paragraph_id) = self.structure_index.paragraph_id_at(ordinal) else {
                    continue;
                };
                if let Some(location) = index.get(paragraph_id) {
                    let paragraph = paragraph_at_location(&document.body, location)
                        .ok_or(EditError::InvalidPosition)?;
                    let text_len = TextMap::new(paragraph)?.text.len();
                    return Ok(Pos {
                        story: location.story.clone(),
                        paragraph_id: paragraph_id.clone(),
                        offset: text_len,
                        affinity: Affinity::Upstream,
                    });
                }
            }
        }
        let fallback_id = fallback_paragraph_id
            .as_ref()
            .ok_or(EditError::InvalidPosition)?;
        let location = index.get(fallback_id).ok_or(EditError::InvalidPosition)?;
        Ok(Pos {
            story: location.story.clone(),
            paragraph_id: fallback_id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        })
    }

    fn paragraph_text_len(&self, paragraph_id: &NodeId) -> Result<usize, EditError> {
        let location = self
            .paragraph_index
            .get(paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
        let paragraph = paragraph_at_location(&self.document.body, location)
            .ok_or_else(|| EditError::UnknownParagraph(paragraph_id.to_string()))?;
        Ok(TextMap::new(paragraph)?.text.len())
    }
}

/// 替换事务失败时恢复的文档与交互快照。
struct StructureEditRestorePoint {
    document: Document,
    paragraph_index: ParagraphIndex,
    structure_index: Arc<StructureIndex>,
    text_map_cache: VecDeque<(NodeId, StoryId, TextMap)>,
    selection: Selection,
    selection_anchor_visual_affinity: VisualAffinity,
    selection_visual_affinity: VisualAffinity,
    math_edit: Option<MathEdit>,
    selected_block_environments: Vec<NodeId>,
    selected_math_blocks: Vec<NodeId>,
    structure_selection: Option<StructureSelection>,
    host_caret_position: Option<HostCaretPosition<Pos>>,
    pending_inline_style: PendingInlineStyle,
    history_state_id: u64,
    history_merge_epoch: u64,
    document_generation: u64,
}

/// 把有序范围两端的文本端点解析为节点移除后的段落位置。
///
/// 只接受两端都是文本端点且位于不同段落；可合并性（容器、标题、样式、opaque 内容）
/// 由 `merge_paragraph_range` 统一判定，避免在 Session 复制同一组前置条件。
fn structure_range_merge_locations(
    start: &DocumentRangeEndpoint,
    end: &DocumentRangeEndpoint,
    after_nodes_index: &ParagraphIndex,
) -> Option<(ParagraphLocation, ParagraphLocation)> {
    let (DocumentRangeEndpoint::Text(start_position), DocumentRangeEndpoint::Text(end_position)) =
        (start, end)
    else {
        return None;
    };
    if start_position.paragraph_id == end_position.paragraph_id {
        return None;
    }
    let start_location = after_nodes_index.get(&start_position.paragraph_id)?.clone();
    let end_location = after_nodes_index.get(&end_position.paragraph_id)?.clone();
    Some((start_location, end_location))
}

/// 从结果文档中移除范围中间的普通段落。
///
/// `index` 是移除节点后的段落索引；同一父容器内按同级下标降序移除，避免前面的
/// 删除影响后续下标。记录身份与文档不一致时返回错误，交由调用方保持失败原子性。
fn remove_range_paragraphs(
    document: &mut Document,
    index: &ParagraphIndex,
    paragraph_ids: &[NodeId],
) -> Result<(), EditError> {
    let mut locations = paragraph_ids
        .iter()
        .map(|paragraph_id| {
            index
                .get(paragraph_id)
                .cloned()
                .ok_or(EditError::InvalidPosition)
        })
        .collect::<Result<Vec<_>, _>>()?;
    locations.sort_by(|left, right| {
        left.parent_environment_ids
            .cmp(&right.parent_environment_ids)
            .then_with(|| right.block_path.cmp(&left.block_path))
    });
    for location in locations {
        let Some((&sibling_index, _)) = location.block_path.split_last() else {
            return Err(EditError::InvalidPosition);
        };
        let siblings =
            block_list_by_parent_ids_mut(&mut document.body, &location.parent_environment_ids)
                .ok_or(EditError::InvalidPosition)?;
        if !matches!(
            siblings.get(sibling_index),
            Some(Block::Paragraph(paragraph)) if paragraph.id == location.paragraph_id
        ) {
            return Err(EditError::InvalidPosition);
        }
        siblings.remove(sibling_index);
    }
    Ok(())
}

/// 在结果文档中校验并收紧落点偏移；段落已被删除时返回 `None`。
fn clamped_caret(document: &Document, index: &ParagraphIndex, position: &Pos) -> Option<Pos> {
    let location = index.get(&position.paragraph_id)?;
    let paragraph = paragraph_at_location(&document.body, location)?;
    let map = TextMap::new(paragraph).ok()?;
    let offset = position.offset.min(map.text.len());
    Some(Pos {
        story: location.story.clone(),
        paragraph_id: position.paragraph_id.clone(),
        offset: map.snap_to_grapheme_boundary(offset, position.affinity),
        affinity: position.affinity,
    })
}
