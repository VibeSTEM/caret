//! 管理编辑 Session 状态并执行输入、选区和 IME 命令。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod block_rows;
mod checkpoint;
mod clipboard;
mod command_line;
mod composition;
pub use checkpoint::SessionCheckpoint;
pub use composition::{CompositionRange, CompositionUpdateCandidate};
mod current_environment;
mod document_position;
mod formatting;
mod history;
mod inline_content;
mod keyboards;
mod math_edit;
pub use math_edit::{
    MathEdit, MathEditBranch, MathEditTarget, MathPositionAffinity, MathPositionHost,
    MathSequencePath, MathSequenceStep, MathTextFlowPath, MathTextFlowPathStep, MathTextPosition,
    MathTreePosition, MathTreePositionError,
};
mod navigation;
mod paragraph_edit;
mod paragraph_formatting;
mod range_edit;
mod range_extension;
mod selection;
mod structure;
mod text_edit;

use super::paragraph_ops::{
    ParagraphIndex, build_paragraph_index, first_paragraph, paragraph_at_location,
    paragraph_at_location_mut,
};
use super::text_map::TextMap;
use super::{
    Affinity, Command, CommandLine, Direction, DocumentCaretPosition, DocumentRangeEndpoint,
    EditError, Effect, HostCaretKind, HostCaretLocalPosition, HostCaretPosition, InputComposition,
    MathCaretCoordinate, MathCaretLocalPosition, Pos, PositionMap, Selection, SessionCaretSnapshot,
    StoryId, StructureSelection,
};
use caret_doc::{Document, NodeId, PageGeometry, Paragraph};
use caret_layout::VisualAffinity;
use caret_structure::StructureIndex;
use std::{collections::VecDeque, sync::Arc};
use undo::History;

use self::formatting::PendingInlineStyle;
use self::history::{DocumentPatch, SessionHistoryEdit, SessionHistoryState};

const TEXT_MAP_CACHE_CAPACITY: usize = 2;

/// 数学 TextFlow 提供给输入法查询的文本与宿主端点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathTextInputContext {
    /// 当前可编辑的数学文本岛。
    pub surrounding_text: String,
    /// 文本岛中的选区锚点，以 UTF-16 code unit 计数。
    pub anchor_utf16: usize,
    /// 文本岛中的输入焦点，以 UTF-16 code unit 计数。
    pub focus_utf16: usize,
    /// 选区锚点的 Parley 视觉侧，可能与焦点侧不同。
    pub anchor_visual_affinity: VisualAffinity,
    /// 数学文本光标的 Parley 视觉侧，用于恢复双向文本中的同偏移插入点。
    pub visual_affinity: VisualAffinity,
    /// 当前公式目标及两端在树中的实际位置。
    pub target: MathEditTarget,
    pub anchor: MathTextPosition,
    pub focus: MathTextPosition,
    /// 整个连续数学文本岛的首尾端点，用于绘制文本岛范围而不推导第二份树状态。
    pub island_start: MathTextPosition,
    pub island_end: MathTextPosition,
}

#[derive(Debug)]
pub struct Session {
    pub(super) document: Document,
    pub(super) selection: Selection,
    selection_anchor_visual_affinity: VisualAffinity,
    selection_visual_affinity: VisualAffinity,
    session_id: NodeId,
    document_generation: u64,
    composition: Option<InputComposition>,
    pending_inline_style: PendingInlineStyle,
    command_line: Option<CommandLine>,
    math_edit: Option<MathEdit>,
    host_caret_position: Option<HostCaretPosition<Pos>>,
    selected_block_environments: Vec<NodeId>,
    selected_math_blocks: Vec<NodeId>,
    structure_selection: Option<StructureSelection>,
    last_committed_composition: Option<u64>,
    text_map_cache: VecDeque<(NodeId, StoryId, TextMap)>,
    paragraph_index: ParagraphIndex,
    structure_index: Arc<StructureIndex>,
    edit_history: History<SessionHistoryEdit>,
    history_state_id: u64,
    next_history_state_id: u64,
    history_estimated_bytes: usize,
    history_merge_epoch: u64,
}

impl Session {
    fn clone_preserving_identity(&self) -> Self {
        Self {
            document: self.document.clone(),
            selection: self.selection.clone(),
            selection_anchor_visual_affinity: self.selection_anchor_visual_affinity,
            selection_visual_affinity: self.selection_visual_affinity,
            session_id: self.session_id.clone(),
            document_generation: self.document_generation,
            composition: self.composition.clone(),
            pending_inline_style: self.pending_inline_style.clone(),
            command_line: self.command_line.clone(),
            math_edit: self.math_edit.clone(),
            host_caret_position: self.host_caret_position.clone(),
            selected_block_environments: self.selected_block_environments.clone(),
            selected_math_blocks: self.selected_math_blocks.clone(),
            structure_selection: self.structure_selection.clone(),
            last_committed_composition: self.last_committed_composition,
            text_map_cache: self.text_map_cache.clone(),
            paragraph_index: self.paragraph_index.clone(),
            structure_index: self.structure_index.clone(),
            edit_history: self.edit_history.clone(),
            history_state_id: self.history_state_id,
            next_history_state_id: self.next_history_state_id,
            history_estimated_bytes: self.history_estimated_bytes,
            history_merge_epoch: self.history_merge_epoch,
        }
    }

    pub fn new(document: Document) -> Result<Self, EditError> {
        document
            .validate()
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
        let (paragraph, story) = first_paragraph(&document.body)
            .ok_or_else(|| EditError::InvalidDocument("document has no paragraph".to_owned()))?;
        let pos = Pos {
            story,
            paragraph_id: paragraph.id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        let structure_index = Arc::new(StructureIndex::build(&document));
        Ok(Self {
            paragraph_index: build_paragraph_index(&document.body),
            structure_index,
            document,
            selection: Selection {
                anchor: pos.clone(),
                focus: pos,
            },
            selection_anchor_visual_affinity: VisualAffinity::Downstream,
            selection_visual_affinity: VisualAffinity::Downstream,
            session_id: NodeId::generate(),
            document_generation: 0,
            composition: None,
            pending_inline_style: PendingInlineStyle::default(),
            command_line: None,
            math_edit: None,
            host_caret_position: None,
            selected_block_environments: Vec::new(),
            selected_math_blocks: Vec::new(),
            structure_selection: None,
            last_committed_composition: None,
            text_map_cache: VecDeque::new(),
            edit_history: history::history(),
            history_state_id: 0,
            next_history_state_id: 1,
            history_estimated_bytes: 0,
            history_merge_epoch: 0,
        })
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    /// 当前打开会话的唯一标识，用于区分被新 Session 替换的同代文档快照。
    pub fn session_id(&self) -> &NodeId {
        &self.session_id
    }

    /// 文档内容或页面几何每成功变更一次便递增；异步布局结果用它拒绝过时代际。
    pub fn document_generation(&self) -> u64 {
        self.document_generation
    }

    /// 返回当前数学 TextFlow 的输入法文本、UTF-16 选区和树端点。
    ///
    /// 源码/结构分支、过期公式焦点或无法映射到同一文本岛的端点返回 `None`；
    /// 查询不改变文档、选择、composition 或历史。
    pub fn math_text_input_context(&self) -> Option<MathTextInputContext> {
        self.ensure_math_edit_revision_current().ok()?;
        let edit = self.math_edit.as_ref()?;
        let MathEditBranch::Text {
            path,
            anchor,
            focus_affinity,
            anchor_visual_affinity,
            visual_affinity,
            ..
        } = &edit.branch
        else {
            return None;
        };
        let (island, anchor_utf8, focus_utf8) = self.math_text_selection_island_offsets()?;
        let surrounding_text = island.display().to_owned();
        let island_start = island.position_at_offset(0, MathPositionAffinity::Downstream)?;
        let island_end =
            island.position_at_offset(surrounding_text.len(), MathPositionAffinity::Upstream)?;
        let anchor_utf16 =
            super::text_map::utf8_offset_to_utf16(&surrounding_text, anchor_utf8).ok()?;
        let focus_utf16 =
            super::text_map::utf8_offset_to_utf16(&surrounding_text, focus_utf8).ok()?;
        Some(MathTextInputContext {
            surrounding_text,
            anchor_utf16,
            focus_utf16,
            anchor_visual_affinity: *anchor_visual_affinity,
            visual_affinity: *visual_affinity,
            target: edit.target.clone(),
            anchor: anchor.clone(),
            focus: MathTextPosition::new(path.clone(), edit.cursor, *focus_affinity),
            island_start,
            island_end,
        })
    }

    /// 返回经文档代际校验的当前数学编辑目标；过期公式焦点返回 `None`。
    pub fn current_math_edit_target(&self) -> Option<&MathEditTarget> {
        self.ensure_math_edit_revision_current().ok()?;
        self.math_edit.as_ref().map(|edit| &edit.target)
    }

    /// 返回当前数学结构序列插入点；源码/TextFlow 分支或过期焦点返回 `None`。
    pub fn math_tree_input_context(&self) -> Option<(MathEditTarget, MathTreePosition)> {
        self.ensure_math_edit_revision_current().ok()?;
        let edit = self.math_edit.as_ref()?;
        let MathEditBranch::Tree { path, anchor } = &edit.branch else {
            return None;
        };
        let mut focus = anchor.clone();
        focus.path = path.clone();
        focus.offset = edit.cursor;
        Some((edit.target.clone(), focus))
    }

    fn current_math_edit_revision(&self) -> math_edit::MathEditRevision {
        let projection_id = self
            .math_edit
            .as_ref()
            .filter(|edit| {
                edit.revision.session_id == self.session_id
                    && edit.revision.document_generation == self.document_generation
            })
            .map(|edit| edit.revision.projection_id.clone())
            .unwrap_or_else(NodeId::generate);
        math_edit::MathEditRevision {
            session_id: self.session_id.clone(),
            document_generation: self.document_generation,
            projection_id,
        }
    }

    fn new_math_edit_revision(&self) -> math_edit::MathEditRevision {
        math_edit::MathEditRevision {
            session_id: self.session_id.clone(),
            document_generation: self.document_generation,
            projection_id: NodeId::generate(),
        }
    }

    pub(super) fn ensure_math_edit_revision_current(&self) -> Result<(), EditError> {
        if self.math_edit.as_ref().is_some_and(|edit| {
            edit.revision.session_id != self.session_id
                || edit.revision.document_generation != self.document_generation
        }) {
            return Err(EditError::InvalidPosition);
        }
        Ok(())
    }

    fn sync_math_edit_revision(&mut self) {
        if let Some(edit) = &mut self.math_edit {
            if edit.source_draft_is_pending() {
                return;
            }
            edit.revision = math_edit::MathEditRevision {
                session_id: self.session_id.clone(),
                document_generation: self.document_generation,
                projection_id: edit.revision.projection_id.clone(),
            };
        }
    }

    pub(super) fn refresh_math_edit_revision(&self, edit: &mut math_edit::MathEdit) {
        edit.revision.session_id = self.session_id.clone();
        edit.revision.document_generation = self.document_generation;
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// 为当前焦点创建绑定到本 Session 与文档代际的位置快照。
    pub fn current_caret_snapshot(&mut self) -> Result<SessionCaretSnapshot, EditError> {
        let has_scoped_position = self.host_caret_position.is_some() || self.math_edit.is_some();
        let position = match self.active_host_caret_position() {
            Some(position) => DocumentCaretPosition::Host(position),
            None if !has_scoped_position => {
                let focus = self.selection.focus.clone();
                self.text_map_for(&focus)?.validate_pos(&focus)?;
                DocumentCaretPosition::Text(focus)
            }
            None => return Err(EditError::InvalidPosition),
        };
        Ok(SessionCaretSnapshot {
            session_id: self.session_id.clone(),
            document_generation: self.document_generation,
            position,
        })
    }

    /// 校验快照的 Session、文档代际和源码草稿修订身份。
    ///
    /// 宿主路径、Story 与局部坐标仍须由具体 adapter 解析和校验。
    pub fn validate_caret_snapshot_revision(
        &self,
        snapshot: &SessionCaretSnapshot,
    ) -> Result<(), EditError> {
        if snapshot.session_id != self.session_id
            || snapshot.document_generation != self.document_generation
        {
            return Err(EditError::InvalidPosition);
        }
        if let DocumentCaretPosition::Host(HostCaretPosition::Inside {
            local:
                HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target,
                    coordinate:
                        MathCaretCoordinate::SourceUtf8 {
                            draft_revision,
                            projection_id,
                            ..
                        },
                }),
            ..
        }) = &snapshot.position
        {
            self.resolve_math_source_revision(
                target,
                draft_revision.as_ref(),
                projection_id.as_ref(),
            )?;
        }
        Ok(())
    }

    /// 按稳定段落 ID 返回 Rust 索引中的正文或环境标题归属。
    pub fn story_for_paragraph(&self, paragraph_id: &NodeId) -> Option<StoryId> {
        self.paragraph_index
            .get(paragraph_id)
            .map(|location| location.story.clone())
    }

    /// 当前文档派生的结构索引；不写入文档，不触发布局。
    pub fn structure_index(&self) -> &StructureIndex {
        &self.structure_index
    }

    /// 当前焦点的视觉侧；不属于 UTF-8 逻辑位置，也不改变文档 generation。
    pub fn selection_visual_affinity(&self) -> VisualAffinity {
        self.selection_visual_affinity
    }

    /// 当前空光标处是否有等待输入的行内格式。
    pub fn has_pending_inline_style(&self) -> bool {
        self.pending_inline_style.shows_insertion_slot()
    }

    /// 当前选区锚点的视觉侧；撤销后用于精确恢复换行边界上的光标端点。
    pub fn selection_anchor_visual_affinity(&self) -> VisualAffinity {
        self.selection_anchor_visual_affinity
    }

    pub fn composition(&self) -> Option<&InputComposition> {
        self.composition.as_ref()
    }

    /// 当前临时命令查询；此状态不属于文档修订或布局缓存。
    pub fn command_line(&self) -> Option<&CommandLine> {
        self.command_line.as_ref()
    }

    /// 当前光标是否停在可编辑块的外侧，而不为导航插入可见空段落。
    pub fn block_boundary_exit(&self) -> Option<(NodeId, Direction)> {
        self.host_caret_position
            .as_ref()
            .filter(|position| position.kind() == HostCaretKind::Block)
            .and_then(HostCaretPosition::as_outside)
            .map(|(host_id, direction)| (host_id.clone(), direction))
    }

    /// 投影当前有效宿主的位置；返回 `None` 表示没有可验证的宿主焦点。
    ///
    /// 数学局部坐标仅对当前 Session 与文档代际有效，不能作为持久或跨代 locator。
    pub fn active_host_caret_position(
        &mut self,
    ) -> Option<HostCaretPosition<HostCaretLocalPosition>> {
        if let Some(position) = self.host_caret_position.clone() {
            match &position {
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Block,
                }
                | HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Block,
                } if math_edit::find_block_path(&self.document.body, host_id).is_some() => {
                    return Some(match &position {
                        HostCaretPosition::Before { .. } => HostCaretPosition::Before {
                            host_id: host_id.clone(),
                            kind: HostCaretKind::Block,
                        },
                        _ => HostCaretPosition::After {
                            host_id: host_id.clone(),
                            kind: HostCaretKind::Block,
                        },
                    });
                }
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
                | HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
                | HostCaretPosition::Inside {
                    host_id,
                    kind: HostCaretKind::Inline,
                    ..
                } => {
                    let focus = self.selection.focus.clone();
                    let map = self.text_map_for(&focus).ok()?;
                    let local_matches = match &position {
                        HostCaretPosition::Inside { local, .. } => local == &focus,
                        _ => true,
                    };
                    if local_matches && map.is_empty_inline_environment(focus.offset, host_id) {
                        return Some(match &position {
                            HostCaretPosition::Before { .. } => HostCaretPosition::Before {
                                host_id: host_id.clone(),
                                kind: HostCaretKind::Inline,
                            },
                            HostCaretPosition::After { .. } => HostCaretPosition::After {
                                host_id: host_id.clone(),
                                kind: HostCaretKind::Inline,
                            },
                            HostCaretPosition::Inside { local, .. } => HostCaretPosition::Inside {
                                host_id: host_id.clone(),
                                kind: HostCaretKind::Inline,
                                local: HostCaretLocalPosition::Paragraph(local.clone()),
                            },
                        });
                    }
                    return None;
                }
                _ => return None,
            }
        }
        if self.math_edit.is_some() {
            self.ensure_math_edit_revision_current().ok()?;
            let edit = self.math_edit.as_ref()?;
            let host_id = match &edit.target {
                MathEditTarget::Inline { math_id, .. } => math_id.clone(),
                MathEditTarget::Block { block_id, .. } => block_id.clone(),
            };
            let kind = match edit.target {
                MathEditTarget::Inline { .. } => HostCaretKind::Inline,
                MathEditTarget::Block { .. } => HostCaretKind::Block,
            };
            return Some(HostCaretPosition::Inside {
                host_id,
                kind,
                local: HostCaretLocalPosition::Math(MathCaretLocalPosition {
                    target: edit.target.clone(),
                    coordinate: match &edit.branch {
                        MathEditBranch::Source => MathCaretCoordinate::SourceUtf8 {
                            offset: edit.cursor,
                            draft_revision: edit
                                .source_draft
                                .as_ref()
                                .map(|draft| (draft.id().clone(), draft.revision())),
                            projection_id: edit
                                .source_draft
                                .is_none()
                                .then(|| edit.revision.projection_id.clone()),
                        },
                        MathEditBranch::Text {
                            path,
                            focus_affinity,
                            ..
                        } => MathCaretCoordinate::TextFlowUtf8 {
                            path: path.clone(),
                            offset: edit.cursor,
                            affinity: *focus_affinity,
                        },
                        MathEditBranch::Tree { path, anchor } => {
                            MathCaretCoordinate::TreeBoundary {
                                host: anchor.host.clone(),
                                path: path.clone(),
                                index: edit.cursor,
                                affinity: anchor.affinity,
                            }
                        }
                    },
                }),
            });
        }

        let focus = self.selection.focus.clone();
        let map = self.text_map_for(&focus).ok()?;
        if let Some(span) = map.object_spans.iter().find(|span| {
            (span.start == focus.offset && focus.affinity == Affinity::Upstream)
                || (span.end == focus.offset && focus.affinity == Affinity::Downstream)
        }) && let Some(host_id) = span.id.clone()
        {
            return Some(if span.start == focus.offset {
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            } else {
                HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            });
        }
        if let Some((host_id, _, _)) = map.inline_environment_at(focus.offset, focus.affinity) {
            return Some(HostCaretPosition::Inside {
                host_id,
                kind: HostCaretKind::Inline,
                local: HostCaretLocalPosition::Paragraph(focus.clone()),
            });
        }
        if let Some(host_id) = map
            .empty_inline_environment_stack_at(focus.offset, focus.affinity)
            .last()
            .and_then(|span| span.id.clone())
        {
            return Some(if focus.affinity == Affinity::Upstream {
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            } else {
                HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            });
        }
        if let Some(span) = map
            .environment_spans
            .iter()
            .filter(|span| {
                span.start < span.end
                    && ((span.start == focus.offset && focus.affinity == Affinity::Upstream)
                        || (span.end == focus.offset && focus.affinity == Affinity::Downstream))
            })
            .max_by_key(|span| span.depth)
            && let Some(host_id) = span.id.clone()
        {
            return Some(if focus.affinity == Affinity::Upstream {
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            } else {
                HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
            });
        }
        self.structure_index
            .at(&caret_structure::StructurePosition {
                paragraph_id: focus.paragraph_id.clone(),
                offset_utf8: focus.offset,
            })
            .into_iter()
            .rfind(|entry| {
                matches!(
                    entry.kind,
                    caret_structure::StructureKind::Section
                        | caret_structure::StructureKind::Custom
                )
            })
            .map(|entry| HostCaretPosition::Inside {
                host_id: entry.id.clone(),
                kind: HostCaretKind::Block,
                local: HostCaretLocalPosition::Paragraph(focus),
            })
    }

    /// 返回当前结构选区覆盖的首个块环境；没有选中块环境时为 `None`。
    pub fn selected_block_environment(&self) -> Option<&NodeId> {
        self.selected_block_environments.first()
    }

    /// 返回当前结构选区覆盖的块环境，按文档顺序排列。
    pub fn selected_block_environments(&self) -> &[NodeId] {
        &self.selected_block_environments
    }

    /// 返回当前结构选区覆盖的 Equation/Align 块，按文档顺序排列。
    pub fn selected_math_blocks(&self) -> &[NodeId] {
        &self.selected_math_blocks
    }

    /// 当前是否存在结构选区；包含两端为块外侧边界的范围。
    pub fn has_structure_selection(&self) -> bool {
        self.structure_selection.is_some()
    }

    /// 返回结构选区中的原始文本锚点；任一端是块外侧边界时返回 `None`。
    pub fn structure_selection_anchor(&self) -> Option<&Pos> {
        match &self.structure_selection.as_ref()?.anchor {
            DocumentRangeEndpoint::Text(anchor) => Some(anchor),
            DocumentRangeEndpoint::BlockEdge { .. } => None,
        }
    }

    /// 返回全部由文本位置表达的结构选区两端；含块外侧边界时返回 `None`。
    pub fn structure_selection(&self) -> Option<(&Pos, &Pos)> {
        let selection = self.structure_selection.as_ref()?;
        match (&selection.anchor, &selection.focus) {
            (DocumentRangeEndpoint::Text(anchor), DocumentRangeEndpoint::Text(focus)) => {
                Some((anchor, focus))
            }
            _ => None,
        }
    }

    /// 返回结构选区的完整文档端点；包含 BlockEdge 端点。
    pub fn structure_selection_range(
        &self,
    ) -> Option<(&DocumentRangeEndpoint, &DocumentRangeEndpoint)> {
        self.structure_selection
            .as_ref()
            .map(|selection| (&selection.anchor, &selection.focus))
    }

    /// 清除结构选区及其派生列表；单值选择从多值派生，不单独维护。
    pub(super) fn clear_structure_selection_state(&mut self) {
        self.selected_block_environments.clear();
        self.selected_math_blocks.clear();
        self.structure_selection = None;
    }

    /// 安装结构选区，并从范围覆盖结果派生块环境与公式块列表。
    ///
    /// 这是结构选区派生状态的唯一写入入口：端点必须能在当前文档中排序，
    /// 派生失败时不改变任何选区状态。
    pub(super) fn install_structure_selection(
        &mut self,
        selection: StructureSelection,
    ) -> Result<(), EditError> {
        let (start, end) =
            match self.compare_range_endpoints(&selection.anchor, &selection.focus)? {
                std::cmp::Ordering::Greater => (&selection.focus, &selection.anchor),
                _ => (&selection.anchor, &selection.focus),
            };
        let (environments, math_blocks) = self.covered_block_nodes(start, end)?;
        self.assign_structure_selection_state(selection, environments, math_blocks);
        Ok(())
    }

    /// 结构选区派生状态的唯一赋值点；调用方须先完成端点校验与列表派生。
    fn assign_structure_selection_state(
        &mut self,
        selection: StructureSelection,
        environments: Vec<NodeId>,
        math_blocks: Vec<NodeId>,
    ) {
        self.selected_block_environments = environments;
        self.selected_math_blocks = math_blocks;
        self.structure_selection = Some(selection);
    }

    /// 将普通文本选区无法跨越的环境边界表示为结构选区投影。
    pub fn select_block_environment(
        &mut self,
        environment_id: NodeId,
    ) -> Result<Effect, EditError> {
        if self.has_uncommitted_math_source_draft() {
            return Err(EditError::InvalidMath(
                "resolve or cancel the incomplete math source draft before changing selection"
                    .to_owned(),
            ));
        }
        self.ensure_math_edit_revision_current()?;
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let exists = self
            .structure_index
            .structures()
            .iter()
            .any(|entry| entry.id == environment_id && entry.kind.is_block_environment());
        if !exists {
            return Err(EditError::InvalidPosition);
        }
        let selection = StructureSelection {
            anchor: DocumentRangeEndpoint::BlockEdge {
                block_id: environment_id.clone(),
                side: Direction::Backward,
            },
            focus: DocumentRangeEndpoint::BlockEdge {
                block_id: environment_id,
                side: Direction::Forward,
            },
        };
        let previous_selection = self.structure_selection.clone();
        let previous_environments = self.selected_block_environments.clone();
        let previous_math_blocks = self.selected_math_blocks.clone();
        // 目标与范围均有效后，才切换离开公式编辑状态。
        self.install_structure_selection(selection)?;
        let changed = self.structure_selection != previous_selection
            || self.selected_block_environments != previous_environments
            || self.selected_math_blocks != previous_math_blocks;
        self.math_edit = None;
        self.host_caret_position = None;
        Ok(Effect {
            selection_changed: changed,
            ..Effect::default()
        })
    }

    /// 当前文档历史状态标识，用于把异步保存结果关联到实际保存点。
    pub fn history_state_id(&self) -> u64 {
        self.history_state_id
    }

    /// 当前是否有尚未解析提交的公式源码；保存和离开文档必须先处理该草稿。
    pub fn has_uncommitted_math_source_draft(&self) -> bool {
        self.math_edit
            .as_ref()
            .is_some_and(MathEdit::source_draft_is_pending)
    }

    pub fn can_undo(&self) -> bool {
        self.math_edit.as_ref().map_or_else(
            || self.edit_history.can_undo(),
            |edit| {
                edit.source_draft_can_undo()
                    || (!edit.source_draft_is_pending() && self.edit_history.can_undo())
            },
        )
    }

    pub fn can_redo(&self) -> bool {
        self.math_edit.as_ref().map_or_else(
            || self.edit_history.can_redo(),
            |edit| {
                edit.source_draft_can_redo()
                    || (!edit.source_draft_is_pending() && self.edit_history.can_redo())
            },
        )
    }

    /// 在异步保存捕获修订前切断输入合并，保留可精确返回的保存位置。
    pub fn begin_save_checkpoint(&mut self) -> Result<(), EditError> {
        if self.has_uncommitted_math_source_draft() {
            return Err(EditError::InvalidMath(
                "resolve or cancel the incomplete math source draft before saving".to_owned(),
            ));
        }
        self.advance_history_merge_epoch();
        Ok(())
    }

    /// 执行唯一命令入口；可恢复校验在替换文档数据前完成。
    pub fn run(&mut self, command: Command) -> Result<Effect, EditError> {
        self.run_with_inline_math_focus(command, false)
    }

    pub(super) fn run_with_inline_math_focus(
        &mut self,
        command: Command,
        focus_inserted_inline_math: bool,
    ) -> Result<Effect, EditError> {
        self.run_session_operation(command, move |session, command| {
            session.run_inner(command, focus_inserted_inline_math)
        })
    }

    /// 包住所有公开输入适配入口，统一执行过时代际检查和成功后的 Session 同步。
    fn run_session_operation(
        &mut self,
        command: Command,
        operation: impl FnOnce(&mut Self, Command) -> Result<Effect, EditError>,
    ) -> Result<Effect, EditError> {
        let preserve_inline_outer_stop = matches!(
            &command,
            Command::MoveCaret(_)
                | Command::EditorKey(super::EditorKey::LeftArrow { .. })
                | Command::EditorKey(super::EditorKey::RightArrow { .. })
        );
        let stale_math_edit = self.ensure_math_edit_revision_current().is_err();
        if stale_math_edit {
            if self.command_line.is_some()
                && matches!(
                    &command,
                    Command::CancelCommandLine | Command::EditorKey(super::EditorKey::Escape)
                )
            {
                let had_preedit = self
                    .command_line
                    .as_ref()
                    .is_some_and(|line| !line.preedit.is_empty());
                self.command_line = None;
                return Ok(Effect {
                    composition_changed: had_preedit,
                    ..Effect::default()
                });
            }
            if self.command_line.is_none()
                && matches!(&command, Command::EditorKey(super::EditorKey::Escape))
            {
                // 仅显式放弃过期的交互投影；已提交数学树和当前宿主选区保持权威。
                self.math_edit = None;
                self.pending_inline_style.clear();
                self.advance_history_merge_epoch();
                return Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                });
            }
            return Err(EditError::InvalidPosition);
        }
        // 未完成草稿的失败命令不会清除编辑状态，避免逐键复制其局部撤销历史。
        let previous_math_edit = self
            .math_edit
            .as_ref()
            .filter(|edit| !edit.source_draft_is_pending())
            .cloned();
        let result = operation(self, command);
        if result.is_err() && self.math_edit.is_none() {
            self.math_edit = previous_math_edit;
        }
        if result.is_ok() {
            self.clear_stale_empty_inline_focus(preserve_inline_outer_stop);
            if self.selection.anchor == self.selection.focus && self.structure_selection.is_none() {
                self.selection_anchor_visual_affinity = self.selection_visual_affinity;
            }
            self.sync_math_edit_revision();
        }
        result
    }

    fn clear_stale_empty_inline_focus(&mut self, preserve_inline_outer_stop: bool) {
        let Some(position) = self.host_caret_position.clone() else {
            return;
        };
        let HostCaretPosition::Inside {
            host_id,
            kind: HostCaretKind::Inline,
            local,
        } = position
        else {
            if position.kind() == HostCaretKind::Inline {
                let focus = self.selection.focus.clone();
                let is_current_inline_boundary = preserve_inline_outer_stop
                    && self.text_map_for(&focus).ok().is_some_and(|map| {
                        map.is_empty_inline_environment(focus.offset, position.host_id())
                    });
                if !is_current_inline_boundary {
                    self.host_caret_position = None;
                }
            }
            return;
        };
        let is_still_active = local == self.selection.focus
            && self
                .text_map_for(&local)
                .ok()
                .is_some_and(|map| map.is_empty_inline_environment(local.offset, &host_id));
        if !is_still_active {
            self.host_caret_position = None;
        }
    }

    fn run_inner(
        &mut self,
        command: Command,
        focus_inserted_inline_math: bool,
    ) -> Result<Effect, EditError> {
        let command = match command {
            Command::TextInput(text) => return self.run_text_input(text),
            command => command,
        };
        if let Command::EditorKey(key) = command {
            return self.run_editor_key(key);
        }
        if matches!(command, Command::Undo | Command::Redo) {
            return self.handle_history_command(command == Command::Redo);
        }
        // 结构选区必须先按范围事务整体消费；相邻空段落的公式邻接特例只在没有结构范围时成立。
        if matches!(&command, Command::Delete(Direction::Backward))
            && self.math_edit.is_none()
            && self.block_boundary_exit().is_none()
            && self.composition.is_none()
            && self.command_line.is_none()
            && !self.has_structure_selection()
            && self.selected_math_blocks.is_empty()
            && self.selection.anchor == self.selection.focus
            && self.selection.focus.offset == 0
        {
            let position = self.selection.focus.clone();
            if let Some(effect) = self.delete_empty_paragraph_after_math(&position)? {
                return Ok(effect);
            }
        }
        if let Some(effect) = self.run_math_edit(&command)? {
            return Ok(effect);
        }
        if let Command::EnterMath(direction) = command {
            return self.enter_math(direction);
        }
        if let Command::SetSelection(selection) = &command {
            let next = selection.clone().unwrap_or_else(|| Selection {
                anchor: self.selection.focus.clone(),
                focus: self.selection.focus.clone(),
            });
            self.validate_selection(&next)?;
        }
        let inserted_text = match &command {
            Command::InsertText(text) | Command::PasteText(text) => Some(text.as_str()),
            Command::InsertOneCharacterWidthSpace => Some("\u{2003}"),
            Command::InsertTwoCharacterWidthSpace => Some("\u{2003}\u{2003}"),
            _ => None,
        };
        // 结构选区由同一文档范围消费：删除、替换与块边界输入都不得落在被删除的宿主内。
        if self.has_structure_selection() {
            if matches!(&command, Command::Delete(_)) {
                if let Some(effect) = self.delete_selected_structure_range()? {
                    return Ok(effect);
                }
                // 范围计划为空时清除隐藏状态，继续普通删除分派而不是永久拒绝按键。
                self.clear_structure_selection_state();
            }
            if let Command::CommitComposition { id, text } = &command
                && let Some(mut effect) = self.commit_composition_replacing_structure(*id, text)?
            {
                effect.composition_changed = true;
                return Ok(effect);
            }
            if let Some(text) = inserted_text {
                // 空文本不构成替换：不消费结构范围，也不产生文档或历史变化。
                if text.is_empty() {
                    return Ok(Effect::default());
                }
                let replaced = if text.contains(['\n', '\r']) {
                    self.replace_structure_selection(|session| session.paste_text(text))?
                } else {
                    self.replace_structure_selection(|session| session.insert_text(text))?
                };
                if let Some(effect) = replaced {
                    return Ok(effect);
                }
            }
            if matches!(&command, Command::InsertParagraphBreak)
                && let Some(effect) = self.replace_structure_selection(Session::split_paragraph)?
            {
                return Ok(effect);
            }
            if matches!(&command, Command::InsertSoftBreak)
                && let Some(effect) =
                    self.replace_structure_selection(Session::insert_soft_break)?
            {
                return Ok(effect);
            }
        }
        // 结构范围已由范围事务消费或无法映射到该范围；块边界输入前清理隐藏状态，
        // 否则范围会跨文档修改存活并吞掉后续按键。
        let block_boundary_mutation = inserted_text.is_some()
            || matches!(&command, Command::CommitComposition { .. })
            || matches!(
                &command,
                Command::InsertParagraphBreak | Command::InsertSoftBreak
            );
        if self.has_structure_selection()
            && self.block_boundary_exit().is_some()
            && block_boundary_mutation
        {
            self.clear_structure_selection_state();
        }
        if let Some(text) = inserted_text
            && self.block_boundary_exit().is_some()
        {
            return self.insert_text_at_block_boundary(text);
        }
        if let Command::CommitComposition { id, text } = &command
            && self.block_boundary_exit().is_some()
        {
            return self.commit_composition(*id, text);
        }
        if self.block_boundary_exit().is_some() {
            match command {
                Command::InsertParagraphBreak => {
                    return self.insert_break_at_block_boundary(false);
                }
                Command::InsertSoftBreak => return self.insert_break_at_block_boundary(true),
                _ => {}
            }
        }
        // IME 预览不改变文档，必须保留结构范围供最终 CommitComposition 作为一次替换消费。
        if !matches!(
            &command,
            Command::SetSelection(_) | Command::UpdateComposition { .. }
        ) {
            self.clear_structure_selection_state();
        }
        if self.command_line.is_some() && matches!(&command, Command::Delete(Direction::Backward)) {
            return self.run_command_line(Command::BackspaceCommandLine);
        }
        if self.block_boundary_exit().is_some() && matches!(&command, Command::Delete(_)) {
            let (block_id, exit_direction) = self
                .host_caret_position
                .as_ref()
                .and_then(HostCaretPosition::as_outside)
                .map(|(block_id, direction)| (block_id.clone(), direction))
                .ok_or(EditError::InvalidPosition)?;
            let deletes_adjacent_environment = matches!(
                (exit_direction, &command),
                (Direction::Forward, Command::Delete(Direction::Backward))
                    | (Direction::Backward, Command::Delete(Direction::Forward))
            );
            if deletes_adjacent_environment
                && !structure::environment_has_content_at(&self.document.body, &block_id)?
            {
                self.install_structure_selection(StructureSelection {
                    anchor: DocumentRangeEndpoint::BlockEdge {
                        block_id: block_id.clone(),
                        side: Direction::Backward,
                    },
                    focus: DocumentRangeEndpoint::BlockEdge {
                        block_id,
                        side: Direction::Forward,
                    },
                })?;
                return self.delete_selected_block_environment();
            }
            return Ok(Effect::default());
        }
        if matches!(&command, Command::Delete(Direction::Backward))
            && self.selection.anchor == self.selection.focus
            && self.selection.focus.offset == 0
            && self.composition.is_none()
            && self.command_line.is_none()
        {
            let position = self.selection.focus.clone();
            if let Some(effect) = self.unwrap_focused_block_environment(true)? {
                return Ok(effect);
            }
            if let Some(effect) = self.delete_empty_environment_before(&position)? {
                return Ok(effect);
            }
        }
        if matches!(
            &command,
            Command::MoveStructure(_)
                | Command::ExitBlockEnvironment(_)
                | Command::RemoveBlockEnvironmentPreservingContent
                | Command::ExpandStructureSelection
                | Command::ExitStructure
        ) {
            return self.run_structure(command);
        }
        if let Command::CreateBlockEnvironment(kind) = command {
            return self.create_block_environment(kind);
        }
        if matches!(
            &command,
            Command::BeginCommandLine
                | Command::TypeCommandLine(_)
                | Command::BackspaceCommandLine
                | Command::MoveCommandLineCursor(_)
                | Command::MoveCommandLineSelection(_)
                | Command::SelectCommandLineCandidate(_)
                | Command::TabCommandLine
                | Command::AcceptCommandLine
                | Command::CancelCommandLine
        ) {
            return self.run_command_line(command);
        }
        let before_selection = self.selection.clone();
        let cancels_command_line_preedit = self
            .command_line
            .as_ref()
            .is_some_and(|line| !line.preedit.is_empty());
        let before_visual_affinity = self.selection_visual_affinity;
        let before_anchor_visual_affinity = self.selection_anchor_visual_affinity;
        let before_pending_style = self.pending_inline_style.clone();
        let before_state_id = self.history_state_id;
        let before_math_edit = self.math_edit.as_ref().map(math_edit::history_position);
        let before_host_caret_position = self.host_caret_position.clone();
        let adjacent_delete_paragraph = match &command {
            Command::Delete(direction) if before_selection.anchor == before_selection.focus => {
                self.adjacent_paragraph_for_delete(*direction)?
            }
            _ => None,
        };
        let edit_group = history::edit_group(&command);
        let breaks_history_merge = edit_group.is_none();
        let is_move_caret = matches!(&command, Command::MoveCaret(_));
        let resets_pending_style =
            matches!(&command, Command::MoveCaret(_) | Command::SetSelection(_));
        let mut before_paragraph_states = if is_paragraph_edit(&command) {
            let mut ids = history::before_selection_ids(&self.paragraph_index, &self.selection);
            if let Command::Delete(_) = &command
                && self.selection.anchor == self.selection.focus
                && let Some(adjacent_id) = adjacent_delete_paragraph.clone()
            {
                ids.push(adjacent_id);
            }
            history::paragraph_states_for_ids(&self.document, &self.paragraph_index, ids)
        } else {
            Vec::new()
        };
        let before_page_geometry = matches!(&command, Command::SetPageGeometry(_))
            .then(|| {
                self.document
                    .page_sections
                    .first()
                    .and_then(|section| section.geometry().ok())
            })
            .flatten();
        let mut before_latin_font_family = None;
        let before_document_font_family = match &command {
            Command::SetDefaultFontFamily(_) => {
                before_latin_font_family = Some(history::LatinFontFamilyState::capture(
                    &self.document.global_settings,
                ));
                None
            }
            Command::SetDocumentFontFamily { target, .. } => {
                let target = target
                    .normalized()
                    .map_err(|_| EditError::InvalidFontFamily)?;
                if target == caret_doc::DocumentFontTarget::Script("Latn".to_owned()) {
                    before_latin_font_family = Some(history::LatinFontFamilyState::capture(
                        &self.document.global_settings,
                    ));
                    None
                } else {
                    Some((
                        target.clone(),
                        self.document
                            .global_settings
                            .font_family(&target)
                            .map(str::to_owned),
                    ))
                }
            }
            _ => None,
        };
        let effect = match command {
            Command::TextInput(_) => unreachable!("text input is resolved before command dispatch"),
            Command::InsertText(text) => self.insert_text(&text),
            Command::InsertOneCharacterWidthSpace => self.insert_text("\u{2003}"),
            Command::InsertTwoCharacterWidthSpace => self.insert_text("\u{2003}\u{2003}"),
            Command::PasteText(text) => self.paste_text(&text),
            Command::InsertSoftBreak => self.insert_soft_break(),
            Command::InsertInlineMath(source) => {
                self.insert_inline_math_with_edit_target(&source, focus_inserted_inline_math)
            }
            Command::EnterMath(direction) => self.enter_math(direction),
            Command::SetMathTextSelection { .. } => Err(EditError::InvalidPosition),
            Command::InsertParagraphBreak => self.split_paragraph(),
            Command::Delete(direction) => self.delete(direction),
            Command::MoveCaret(direction) => self.move_caret(direction),
            Command::MoveStructure(_)
            | Command::ExitBlockEnvironment(_)
            | Command::RemoveBlockEnvironmentPreservingContent
            | Command::ExpandStructureSelection
            | Command::ExitStructure => {
                unreachable!("structure commands returned above")
            }
            Command::CreateBlockEnvironment(_) => {
                unreachable!("structure edit commands returned above")
            }
            Command::ToggleInlineFormat(kind) => self.toggle_inline_format(kind),
            Command::ToggleRunUnderline => self.toggle_run_underline(),
            Command::RemoveInlineEnvironmentPreservingContent(environment_id) => {
                self.remove_inline_environment_preserving_content(&environment_id)
            }
            Command::RemoveRunPropertyPreservingContent(kind) => {
                self.remove_run_property_from_current_range(kind)
            }
            Command::SetInlineFontFamily(family) => self.set_inline_font_family(&family),
            Command::SetParagraphFirstLineIndentChars(chars) => {
                self.set_paragraph_first_line_indent_chars(chars)
            }
            Command::SetDefaultFontFamily(family) => self.set_default_font_family(family),
            Command::SetDocumentFontFamily { target, family } => {
                self.set_document_font_family(&target, family)
            }
            Command::EditorKey(_) => unreachable!("editor keys returned before editing dispatch"),
            Command::SetSelection(selection) => self.set_selection(selection),
            Command::UpdateComposition { id, preedit } => self.update_composition(id, preedit),
            Command::CommitComposition { id, text } => self.commit_composition(id, &text),
            Command::CancelComposition { id } => self.cancel_composition(id),
            Command::SetPageGeometry(geometry) => self.set_page_geometry(geometry),
            Command::BeginCommandLine
            | Command::TypeCommandLine(_)
            | Command::BackspaceCommandLine
            | Command::MoveCommandLineCursor(_)
            | Command::MoveCommandLineSelection(_)
            | Command::SelectCommandLineCandidate(_)
            | Command::TabCommandLine
            | Command::AcceptCommandLine
            | Command::CancelCommandLine => unreachable!("command-line commands returned above"),
            Command::Undo | Command::Redo => unreachable!("history commands returned above"),
        };
        if effect.is_ok() {
            self.clear_stale_empty_inline_focus(is_move_caret);
        }
        let mut effect = effect?;
        effect.composition_changed |= cancels_command_line_preedit;
        self.command_line = None;
        if effect.selection_changed && resets_pending_style {
            self.pending_inline_style.clear();
        }
        if breaks_history_merge
            && (!is_move_caret || effect.selection_changed || effect.document_changed)
        {
            self.advance_history_merge_epoch();
        }
        if effect.selection_changed {
            self.selection_visual_affinity = match self.selection.focus.affinity {
                Affinity::Upstream => VisualAffinity::Upstream,
                Affinity::Downstream => VisualAffinity::Downstream,
            };
        }
        if effect.document_changed {
            if effect
                .position_map
                .as_ref()
                .is_some_and(position_map_changes_paragraph_structure)
            {
                self.structure_index = Arc::new(StructureIndex::build(&self.document));
            }
            let after_state_id = self.allocate_history_state_id();
            let patch = if let Some(before) = before_page_geometry {
                let after = self
                    .document
                    .page_sections
                    .first()
                    .and_then(|section| section.geometry().ok())
                    .ok_or_else(|| {
                        EditError::InvalidDocument("document has no PageSection".to_owned())
                    })?;
                DocumentPatch::PageGeometry { before, after }
            } else if let Some(before) = before_latin_font_family {
                DocumentPatch::LatinFontFamily {
                    before,
                    after: history::LatinFontFamilyState::capture(&self.document.global_settings),
                }
            } else if let Some((target, before)) = before_document_font_family {
                DocumentPatch::DocumentFontFamily {
                    before,
                    after: self
                        .document
                        .global_settings
                        .font_family(&target)
                        .map(str::to_owned),
                    target,
                }
            } else {
                let mut ids = before_paragraph_states
                    .iter()
                    .map(|state| state.id.clone())
                    .collect::<Vec<_>>();
                if let Some(map) = &effect.position_map {
                    history::state_ids_from_position_map(map, &mut ids);
                }
                let after_paragraph_states =
                    history::paragraph_states_for_ids(&self.document, &self.paragraph_index, ids);
                if after_paragraph_states.is_empty() {
                    return Err(EditError::InvalidDocument(
                        "changed document command did not produce a history patch".to_owned(),
                    ));
                }
                DocumentPatch::Paragraphs {
                    before: std::mem::take(&mut before_paragraph_states),
                    after: after_paragraph_states,
                    merge_group: edit_group,
                    merge_epoch: self.history_merge_epoch,
                }
            };
            effect.changed_paragraph_id = patch.single_changed_paragraph_id();
            let edit = SessionHistoryEdit::new(
                patch,
                SessionHistoryState::new(
                    before_selection,
                    before_visual_affinity,
                    before_pending_style,
                    before_state_id,
                )
                .with_anchor_visual_affinity(before_anchor_visual_affinity)
                .with_math_edit(before_math_edit)
                .with_host_caret_position(before_host_caret_position),
                SessionHistoryState::new(
                    self.selection.clone(),
                    self.selection_visual_affinity,
                    self.pending_inline_style.clone(),
                    after_state_id,
                )
                .with_anchor_visual_affinity(if self.selection.anchor == self.selection.focus {
                    self.selection_visual_affinity
                } else {
                    self.selection_anchor_visual_affinity
                })
                .with_math_edit(self.math_edit.as_ref().map(math_edit::history_position))
                .with_host_caret_position(self.host_caret_position.clone()),
            );
            self.record_history_edit(edit);
            self.history_state_id = after_state_id;
            if let Some(next) = self.document_generation.checked_add(1) {
                self.document_generation = next;
            } else {
                self.session_id = NodeId::generate();
                self.document_generation = 0;
            }
        }
        Ok(effect)
    }

    fn record_history_edit(&mut self, edit: SessionHistoryEdit) {
        let latest_edit_bytes = edit.estimated_bytes();
        let transition = self.edit_history.edit(&mut self.document, edit.clone());
        self.history_state_id = transition.state_id;
        self.history_estimated_bytes = history::estimated_history_bytes(&self.edit_history);
        if self.history_estimated_bytes > history::history_memory_limit() {
            self.edit_history.clear();
            self.history_estimated_bytes = 0;
            if latest_edit_bytes <= history::history_memory_limit() {
                let transition = self.edit_history.edit(&mut self.document, edit);
                self.history_state_id = transition.state_id;
                self.history_estimated_bytes = history::estimated_history_bytes(&self.edit_history);
                if self.history_estimated_bytes > history::history_memory_limit() {
                    self.edit_history.clear();
                    self.history_estimated_bytes = 0;
                }
            }
        }
    }

    pub(super) fn advance_document_generation(&mut self) {
        if let Some(next) = self.document_generation.checked_add(1) {
            self.document_generation = next;
        } else {
            self.session_id = NodeId::generate();
            self.document_generation = 0;
        }
        self.sync_math_edit_revision();
    }

    fn advance_history_merge_epoch(&mut self) {
        if let Some(next) = self.history_merge_epoch.checked_add(1) {
            self.history_merge_epoch = next;
        } else {
            self.edit_history.clear();
            self.history_estimated_bytes = 0;
            self.history_merge_epoch = 0;
        }
    }

    pub(super) fn allocate_history_state_id(&mut self) -> u64 {
        let state_id = self.next_history_state_id;
        if let Some(next) = state_id.checked_add(1) {
            self.next_history_state_id = next;
        } else {
            self.edit_history.clear();
            self.history_estimated_bytes = 0;
            self.next_history_state_id = 1;
        }
        state_id
    }

    pub(super) fn restore_history(&mut self, redo: bool) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if redo && !self.edit_history.can_redo() || !redo && !self.edit_history.can_undo() {
            return Ok(Effect::default());
        }
        let mut next_document = self.document.clone();
        let head = self.edit_history.head().index;
        let edit_index = if redo {
            head
        } else {
            head.checked_sub(1).ok_or(EditError::InvalidPosition)?
        };
        let mut candidate_edit = self
            .edit_history
            .get_entry(edit_index)
            .ok_or(EditError::InvalidPosition)?
            .as_ref()
            .clone();
        let transition = if redo {
            undo::Edit::redo(&mut candidate_edit, &mut next_document)
        } else {
            undo::Edit::undo(&mut candidate_edit, &mut next_document)
        };
        let previous_math_edit = self.math_edit.clone();
        let history::RestoredHistoryTransition {
            selection,
            anchor_visual_affinity,
            visual_affinity,
            pending_style,
            state_id,
            changed_paragraph_id,
            document_changed,
            math_edit: math_edit_position,
            host_caret_position,
            valid,
        } = history::restore_history_transition(transition);
        if !valid {
            return Err(EditError::InvalidPosition);
        }
        if document_changed && next_document == self.document {
            return Err(EditError::InvalidPosition);
        }
        let next_paragraph_index = build_paragraph_index(&next_document.body);
        for position in [&selection.anchor, &selection.focus] {
            let location = next_paragraph_index
                .get(&position.paragraph_id)
                .ok_or(EditError::InvalidPosition)?;
            if location.story != position.story {
                return Err(EditError::WrongStory);
            }
            let paragraph = paragraph_at_location(&next_document.body, location)
                .ok_or(EditError::InvalidPosition)?;
            TextMap::new(paragraph)?.validate_pos(position)?;
        }
        let restored_math_edit = match math_edit_position {
            Some(position) => {
                let revision = self.new_math_edit_revision();
                match math_edit::restore_math_edit(&next_document, position, revision) {
                    Some(edit) => Some(edit),
                    None => return Err(EditError::InvalidPosition),
                }
            }
            None => None,
        };
        if let Some(position) = &host_caret_position {
            match position {
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Block,
                }
                | HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Block,
                } => {
                    if math_edit::find_block_path(&next_document.body, host_id).is_none() {
                        return Err(EditError::InvalidPosition);
                    }
                }
                HostCaretPosition::Inside {
                    host_id,
                    kind: HostCaretKind::Block,
                    local,
                } => {
                    let location = next_paragraph_index
                        .get(&local.paragraph_id)
                        .ok_or(EditError::InvalidPosition)?;
                    if location.story != local.story
                        || !location.parent_environment_ids.contains(host_id)
                    {
                        return Err(EditError::InvalidPosition);
                    }
                    TextMap::new(
                        paragraph_at_location(&next_document.body, location)
                            .ok_or(EditError::InvalidPosition)?,
                    )?
                    .validate_pos(local)?;
                }
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Inline,
                }
                | HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Inline,
                } => {
                    let focus = &selection.focus;
                    let location = next_paragraph_index
                        .get(&focus.paragraph_id)
                        .ok_or(EditError::InvalidPosition)?;
                    let paragraph = paragraph_at_location(&next_document.body, location)
                        .ok_or(EditError::InvalidPosition)?;
                    let map = TextMap::new(paragraph)?;
                    map.validate_pos(focus)?;
                    if !map.is_empty_inline_environment(focus.offset, host_id) {
                        return Err(EditError::InvalidPosition);
                    }
                }
                HostCaretPosition::Inside {
                    host_id,
                    kind: HostCaretKind::Inline,
                    local,
                } => {
                    if local != &selection.focus {
                        return Err(EditError::InvalidPosition);
                    }
                    let location = next_paragraph_index
                        .get(&local.paragraph_id)
                        .ok_or(EditError::InvalidPosition)?;
                    let paragraph = paragraph_at_location(&next_document.body, location)
                        .ok_or(EditError::InvalidPosition)?;
                    let map = TextMap::new(paragraph)?;
                    map.validate_pos(local)?;
                    if !map.is_empty_inline_environment(local.offset, host_id) {
                        return Err(EditError::InvalidPosition);
                    }
                }
            }
        }
        let selection_changed = self.selection != selection
            || self.selection_anchor_visual_affinity != anchor_visual_affinity
            || self.selection_visual_affinity != visual_affinity
            || previous_math_edit != restored_math_edit
            || self.host_caret_position != host_caret_position;
        let applied_transition = if redo {
            self.edit_history.redo(&mut self.document)
        } else {
            self.edit_history.undo(&mut self.document)
        }
        .ok_or(EditError::InvalidPosition)?;
        if !applied_transition.valid {
            return Err(EditError::InvalidPosition);
        }
        self.document = next_document;
        self.selection = selection;
        self.selection_anchor_visual_affinity = anchor_visual_affinity;
        self.selection_visual_affinity = visual_affinity;
        self.pending_inline_style = pending_style;
        self.math_edit = restored_math_edit;
        self.clear_structure_selection_state();
        self.history_state_id = state_id;
        self.advance_history_merge_epoch();
        if document_changed {
            self.paragraph_index = build_paragraph_index(&self.document.body);
            self.structure_index = Arc::new(StructureIndex::build(&self.document));
            self.text_map_cache.clear();
            if let Some(next) = self.document_generation.checked_add(1) {
                self.document_generation = next;
            } else {
                self.session_id = NodeId::generate();
                self.document_generation = 0;
            }
        }
        self.host_caret_position = host_caret_position;
        self.sync_math_edit_revision();
        Ok(Effect {
            document_changed,
            selection_changed,
            changed_paragraph_id,
            ..Effect::default()
        })
    }

    fn adjacent_paragraph_for_delete(
        &mut self,
        direction: Direction,
    ) -> Result<Option<NodeId>, EditError> {
        let position = self.selection.focus.clone();
        let at_boundary = match direction {
            Direction::Backward => position.offset == 0,
            Direction::Forward => position.offset == self.text_map_for(&position)?.text.len(),
        };
        Ok(at_boundary
            .then(|| {
                history::include_adjacent_paragraph_id(
                    &self.document,
                    &self.paragraph_index,
                    &position.paragraph_id,
                    direction,
                )
            })
            .flatten())
    }

    fn set_page_geometry(&mut self, geometry: PageGeometry) -> Result<Effect, EditError> {
        geometry
            .validate()
            .map_err(|error| EditError::InvalidPageGeometry(error.to_string()))?;
        let section =
            self.document.page_sections.first_mut().ok_or_else(|| {
                EditError::InvalidDocument("document has no PageSection".to_owned())
            })?;
        let current = section
            .geometry()
            .map_err(|error| EditError::InvalidPageGeometry(error.to_string()))?;
        if current == geometry {
            return Ok(Effect::default());
        }
        section
            .set_geometry(geometry)
            .map_err(|error| EditError::InvalidPageGeometry(error.to_string()))?;
        Ok(Effect {
            document_changed: true,
            ..Effect::default()
        })
    }

    pub(super) fn paragraph_for(&self, pos: &Pos) -> Result<&Paragraph, EditError> {
        let location = self
            .paragraph_index
            .get(&pos.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if location.story != pos.story {
            return Err(EditError::WrongStory);
        }
        let paragraph = paragraph_at_location(&self.document.body, location)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if paragraph.id != pos.paragraph_id {
            return Err(EditError::UnknownParagraph(pos.paragraph_id.to_string()));
        }
        Ok(paragraph)
    }

    fn paragraph_for_mut(&mut self, pos: &Pos) -> Result<&mut Paragraph, EditError> {
        let location = self
            .paragraph_index
            .get(&pos.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if location.story != pos.story {
            return Err(EditError::WrongStory);
        }
        let paragraph = paragraph_at_location_mut(&mut self.document.body, location)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if paragraph.id != pos.paragraph_id {
            return Err(EditError::UnknownParagraph(pos.paragraph_id.to_string()));
        }
        Ok(paragraph)
    }

    pub(super) fn text_map_for(&mut self, pos: &Pos) -> Result<&TextMap, EditError> {
        if let Some(index) = self
            .text_map_cache
            .iter()
            .position(|(paragraph_id, story, _)| {
                paragraph_id == &pos.paragraph_id && story == &pos.story
            })
        {
            self.paragraph_for(pos)?;
            if index > 0 {
                let entry = self
                    .text_map_cache
                    .remove(index)
                    .ok_or(EditError::InvalidPosition)?;
                self.text_map_cache.push_front(entry);
            }
        } else {
            let paragraph = self.paragraph_for(pos)?;
            let map = TextMap::new(paragraph)?;
            self.text_map_cache
                .push_front((pos.paragraph_id.clone(), pos.story.clone(), map));
            if self.text_map_cache.len() > TEXT_MAP_CACHE_CAPACITY {
                self.text_map_cache.pop_back();
            }
        }
        self.text_map_cache
            .front()
            .map(|(_, _, map)| map)
            .ok_or(EditError::InvalidPosition)
    }

    pub(super) fn replace_paragraph(
        &mut self,
        pos: &Pos,
        paragraph: Paragraph,
    ) -> Result<(), EditError> {
        let location = self
            .paragraph_index
            .get(&pos.paragraph_id)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))?;
        if location.story != pos.story {
            return Err(EditError::WrongStory);
        }
        if location.paragraph_id != pos.paragraph_id || paragraph.id != pos.paragraph_id {
            return Err(EditError::UnknownParagraph(pos.paragraph_id.to_string()));
        }
        *paragraph_at_location_mut(&mut self.document.body, location)
            .ok_or_else(|| EditError::UnknownParagraph(pos.paragraph_id.to_string()))? = paragraph;
        Ok(())
    }
}

/// 克隆创建独立编辑会话，异步布局身份与文档代际从新会话重新开始。
impl Clone for Session {
    fn clone(&self) -> Self {
        let session_id = NodeId::generate();
        let mut clone = self.clone_preserving_identity();
        if let Some(edit) = clone.math_edit.as_mut() {
            // 仅迁移仍有效的公式焦点；过期投影不能借克隆恢复权限。
            if self.ensure_math_edit_revision_current().is_ok() {
                edit.revision = math_edit::MathEditRevision {
                    session_id: session_id.clone(),
                    document_generation: 0,
                    projection_id: edit.revision.projection_id.clone(),
                };
            }
        }
        clone.session_id = session_id;
        clone.document_generation = 0;
        clone
    }
}

fn is_paragraph_edit(command: &Command) -> bool {
    matches!(
        command,
        Command::InsertText(_)
            | Command::InsertOneCharacterWidthSpace
            | Command::InsertTwoCharacterWidthSpace
            | Command::PasteText(_)
            | Command::InsertSoftBreak
            | Command::InsertInlineMath(_)
            | Command::InsertParagraphBreak
            | Command::ToggleInlineFormat(_)
            | Command::ToggleRunUnderline
            | Command::RemoveInlineEnvironmentPreservingContent(_)
            | Command::RemoveRunPropertyPreservingContent(_)
            | Command::SetInlineFontFamily(_)
            | Command::SetParagraphFirstLineIndentChars(_)
            | Command::Delete(_)
            | Command::CommitComposition { .. }
    )
}

fn position_map_changes_paragraph_structure(position_map: &PositionMap) -> bool {
    match position_map {
        PositionMap::Composite(maps) => maps.iter().any(position_map_changes_paragraph_structure),
        PositionMap::ParagraphSplit { .. }
        | PositionMap::ParagraphMerge { .. }
        | PositionMap::ParagraphRangeMerge { .. }
        | PositionMap::StoryChange { .. } => true,
        PositionMap::TextEdit { .. } => false,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod math_text_input_context_tests {
    use super::*;

    fn session_in_math_text() -> Result<Session, EditError> {
        let mut session = Session::new(Document::blank())?;
        session.run(Command::InsertInlineMath(
            r"\frac{1}{\text{A😀B}}".to_owned(),
        ))?;
        session.run(Command::EnterMath(Direction::Backward))?;
        for _ in 0..64 {
            if session
                .math_edit()
                .is_some_and(|edit| matches!(edit.branch, MathEditBranch::Text { .. }))
            {
                return Ok(session);
            }
            session.run(Command::MoveCaret(Direction::Backward))?;
        }
        Err(EditError::InvalidPosition)
    }

    #[test]
    fn math_text_input_context_uses_utf16_offsets_for_reverse_selection() -> Result<(), EditError> {
        let mut session = session_in_math_text()?;
        session.run(Command::SetMathTextSelection {
            anchor: 6,
            focus: 1,
        })?;

        let context = session
            .math_text_input_context()
            .ok_or(EditError::InvalidPosition)?;
        assert_eq!(context.surrounding_text, "A😀B");
        assert_eq!(context.anchor_utf16, 4);
        assert_eq!(context.focus_utf16, 1);
        assert_eq!(context.anchor.offset, 6);
        assert_eq!(context.focus.offset, 1);
        assert!(matches!(context.target, MathEditTarget::Inline { .. }));
        Ok(())
    }

    #[test]
    fn math_text_input_context_is_unavailable_for_source_tree_and_stale_edits()
    -> Result<(), EditError> {
        let mut source = session_in_math_text()?;
        source
            .math_edit
            .as_mut()
            .ok_or(EditError::InvalidPosition)?
            .branch = MathEditBranch::Source;
        assert!(source.math_text_input_context().is_none());

        let mut tree = Session::new(Document::blank())?;
        tree.begin_inline_math()?;
        assert!(matches!(
            tree.math_edit().map(|edit| &edit.branch),
            Some(MathEditBranch::Tree { .. })
        ));
        assert!(tree.math_text_input_context().is_none());

        let mut stale = session_in_math_text()?;
        stale.document_generation += 1;
        assert!(stale.math_text_input_context().is_none());
        Ok(())
    }

    #[test]
    fn align_math_text_input_context_is_limited_to_the_active_cell() -> Result<(), EditError> {
        let source = r"\begin{aligned}\text{甲}&\text{乙}\end{aligned}";
        let projection = caret_math::parse_projection(source, caret_math::MathInputKind::Align)
            .map_err(|error| EditError::InvalidMath(error.to_string()))?;
        let (content, _) = projection.into_parts();
        let caret_math::ProjectedMath::Align(align) = content else {
            return Err(EditError::InvalidPosition);
        };
        let mut document = Document::blank();
        document.body = vec![
            caret_doc::Block::Paragraph(caret_doc::Paragraph::empty(NodeId::generate())),
            caret_doc::Block::Align(align),
        ];
        let mut session = Session::new(document)?;
        session.run(Command::EnterMath(Direction::Forward))?;
        let first = session
            .math_text_input_context()
            .ok_or(EditError::InvalidPosition)?;
        assert_eq!(first.surrounding_text, "甲");
        assert_eq!(first.focus.path.align_cell(), Some((0, 0)));

        session.run(Command::SetMathTextSelection {
            anchor: "甲".len(),
            focus: "甲".len(),
        })?;
        for _ in 0..8 {
            if session
                .math_text_input_context()
                .is_some_and(|context| context.focus.path.align_cell() == Some((0, 1)))
            {
                break;
            }
            session.run(Command::MoveCaret(Direction::Forward))?;
        }
        let second = session
            .math_text_input_context()
            .ok_or(EditError::InvalidPosition)?;
        assert_eq!(second.surrounding_text, "乙");
        assert_eq!(second.focus.path.align_cell(), Some((0, 1)));
        Ok(())
    }
}
