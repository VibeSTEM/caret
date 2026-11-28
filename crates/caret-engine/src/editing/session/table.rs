//! 表格上下文：插入表格、行列结构命令与单元格间导航。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::document_position::visual_affinity_for;
use super::*;
use crate::editing::paragraph_ops::{
    BlockContainerPath, TableCellPath, block_list_in_container, block_list_in_container_mut,
    build_paragraph_index,
};
use caret_doc::{Block, NodeId, Paragraph, Table, TableCell, TableRow};

/// 方向键在单元格边界的移动轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TableArrowAxis {
    Horizontal,
    Vertical,
}

/// 当前表格单元格及其所属表格的块序列位置。
#[derive(Clone)]
pub(super) struct TableContext {
    /// 表格块所在容器（单元格之前的容器路径）。
    pub(super) table_container: BlockContainerPath,
    pub(super) table_sibling_index: usize,
    pub(super) cell: TableCellPath,
}

impl Session {
    /// 返回交互焦点所属单元格。块外停靠以宿主的容器为准，忽略借用的文本位置。
    pub fn focused_table_cell(&self) -> Option<TableCellPath> {
        if self.has_structure_selection() {
            return None;
        }
        if let Some(target) = self.current_math_edit_target() {
            return match target {
                MathEditTarget::Block { block_id, .. } => {
                    caret_structure::find_block_node(&self.document.body, block_id)?
                        .container
                        .innermost_cell()
                        .cloned()
                }
                MathEditTarget::Inline { paragraph_id, .. } => self
                    .paragraph_index
                    .get(paragraph_id)?
                    .container
                    .innermost_cell()
                    .cloned(),
            };
        }
        if let Some((block_id, _)) = self.block_boundary_exit() {
            return caret_structure::find_block_node(&self.document.body, &block_id)?
                .container
                .innermost_cell()
                .cloned();
        }
        self.table_cell_for_position(&self.selection.focus).cloned()
    }

    pub(super) fn table_cell_for_position(&self, position: &Pos) -> Option<&TableCellPath> {
        self.paragraph_index
            .get(&position.paragraph_id)?
            .container
            .innermost_cell()
    }

    /// 垂直视觉边缘可先退出单元格内环境，再由共享块边界导航处理单元格迁移。
    pub(super) fn focus_is_inside_table_cell_environment(&self) -> bool {
        let Some(location) = self.paragraph_index.get(&self.selection.focus.paragraph_id) else {
            return false;
        };
        let Some(cell_container) = location.container.container_before_innermost_cell() else {
            return false;
        };
        location.container.environment_depth() > cell_container.environment_depth()
    }

    /// 解析当前焦点所在的表格单元格；焦点必须落在单元格块流内的段落上。
    pub(super) fn table_context(&self) -> Option<TableContext> {
        if self.selection.anchor != self.selection.focus
            || self.block_boundary_exit().is_some()
            || self.has_structure_selection()
        {
            return None;
        }
        let location = self
            .paragraph_index
            .get(&self.selection.focus.paragraph_id)?;
        self.table_context_for_container(&location.container)
    }

    fn table_context_for_container(&self, container: &BlockContainerPath) -> Option<TableContext> {
        let cell = container.innermost_cell()?.clone();
        let table_container = container.container_before_innermost_cell()?;
        let blocks = block_list_in_container(&self.document.body, &table_container)?;
        let table_sibling_index = blocks
            .iter()
            .position(|block| matches!(block, Block::Table(table) if table.id == cell.table_id))?;
        Some(TableContext {
            table_container,
            table_sibling_index,
            cell,
        })
    }

    /// Tab 可用单元格内块的真实容器；行列与方向键仍要求文本光标上下文。
    fn table_tab_context(&self) -> Option<TableContext> {
        let Some((block_id, _)) = self.block_boundary_exit() else {
            return self.table_context();
        };
        if self.has_structure_selection() {
            return None;
        }
        let location = caret_structure::find_block_node(&self.document.body, &block_id)?;
        self.table_context_for_container(&location.container)
    }

    pub(super) fn run_table_command(&mut self, command: Command) -> Result<Effect, EditError> {
        match command {
            Command::InsertTable => self.insert_table(),
            Command::TableInsertRow(direction) => self.insert_table_row(direction),
            Command::TableDeleteRow => self.delete_table_row(),
            Command::TableInsertColumn(direction) => self.insert_table_column(direction),
            Command::TableDeleteColumn => self.delete_table_column(),
            _ => Err(EditError::UnknownCommand),
        }
    }

    /// 表格只能插入正文/环境正文块序列；标题与单元格内当前明确拒绝。
    pub(super) fn can_insert_table(&self) -> bool {
        let Some(location) = self.paragraph_index.get(&self.selection.focus.paragraph_id) else {
            return false;
        };
        !location.is_title && location.container.innermost_cell().is_none()
    }

    pub(super) fn insert_table(&mut self) -> Result<Effect, EditError> {
        if !self.can_insert_table() {
            return Err(EditError::TableInsertionUnsupported);
        }
        let location = self
            .paragraph_index
            .get(&self.selection.focus.paragraph_id)
            .cloned()
            .ok_or_else(|| {
                EditError::UnknownParagraph(self.selection.focus.paragraph_id.to_string())
            })?;
        if location.is_title {
            return Err(EditError::InvalidPosition);
        }
        let Some(&sibling_index) = location.block_path.last() else {
            return Err(EditError::InvalidPosition);
        };
        let table = empty_table(2, 2);
        let target = table_target(&table, 0, 0, true);
        self.apply_block_range(
            location.container.clone(),
            sibling_index + 1,
            Vec::new(),
            vec![Block::Table(table)],
            target,
        )
    }

    pub(super) fn insert_table_row(&mut self, direction: Direction) -> Result<Effect, EditError> {
        let context = self.table_context().ok_or(EditError::InvalidPosition)?;
        let before = self.current_table(&context)?.clone();
        let columns = before
            .rows
            .get(context.cell.row_index)
            .ok_or(EditError::InvalidPosition)?
            .cells
            .len();
        let mut table = before.clone();
        let insert_at = match direction {
            Direction::Backward => context.cell.row_index,
            Direction::Forward => context.cell.row_index + 1,
        };
        table.rows.insert(insert_at, empty_row(columns));
        let target = table_target(&table, insert_at, context.cell.cell_index, true);
        self.replace_table(context, before, table, target)
    }

    pub(super) fn delete_table_row(&mut self) -> Result<Effect, EditError> {
        let context = self.table_context().ok_or(EditError::InvalidPosition)?;
        let before = self.current_table(&context)?.clone();
        if before.rows.len() <= 1 {
            return Ok(Effect::default());
        }
        let mut table = before.clone();
        table.rows.remove(context.cell.row_index);
        let target_row = context.cell.row_index.min(table.rows.len() - 1);
        let target = table_target(&table, target_row, context.cell.cell_index, true);
        self.replace_table(context, before, table, target)
    }

    pub(super) fn insert_table_column(
        &mut self,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        let context = self.table_context().ok_or(EditError::InvalidPosition)?;
        let before = self.current_table(&context)?.clone();
        let mut table = before.clone();
        let insert_at = match direction {
            Direction::Backward => context.cell.cell_index,
            Direction::Forward => context.cell.cell_index + 1,
        };
        for row in &mut table.rows {
            row.cells.insert(insert_at, empty_cell());
        }
        let target = table_target(&table, context.cell.row_index, insert_at, true);
        self.replace_table(context, before, table, target)
    }

    pub(super) fn delete_table_column(&mut self) -> Result<Effect, EditError> {
        let context = self.table_context().ok_or(EditError::InvalidPosition)?;
        let before = self.current_table(&context)?.clone();
        if before.rows.first().is_none_or(|row| row.cells.len() <= 1) {
            return Ok(Effect::default());
        }
        let mut table = before.clone();
        for row in &mut table.rows {
            if context.cell.cell_index < row.cells.len() {
                row.cells.remove(context.cell.cell_index);
            }
        }
        let target_cell = context.cell.cell_index.min(table.rows[0].cells.len() - 1);
        let target = table_target(&table, context.cell.row_index, target_cell, true);
        self.replace_table(context, before, table, target)
    }

    /// Tab 到下一格；最后一格再按 Tab 增加一行并落到新行首格。
    pub(super) fn table_tab_forward(&mut self) -> Result<Option<Effect>, EditError> {
        let Some(context) = self.table_tab_context() else {
            return Ok(None);
        };
        let before = self.current_table(&context)?;
        let columns = before.rows[0].cells.len();
        let index = context.cell.row_index * columns + context.cell.cell_index + 1;
        if index < before.rows.len() * columns {
            return Ok(Some(self.focus_table_cell(
                &context.cell.table_id,
                index / columns,
                index % columns,
                true,
            )?));
        }
        let before = before.clone();
        let mut table = before.clone();
        table.rows.push(empty_row(columns));
        let last_row = table.rows.len() - 1;
        let target = table_target(&table, last_row, 0, true);
        Ok(Some(self.replace_table(context, before, table, target)?))
    }

    /// Shift+Tab 到上一格；首格保持原位。
    pub(super) fn table_tab_backward(&mut self) -> Result<Option<Effect>, EditError> {
        let Some(context) = self.table_tab_context() else {
            return Ok(None);
        };
        if context.cell.row_index == 0 && context.cell.cell_index == 0 {
            return Ok(Some(Effect::default()));
        }
        let before = self.current_table(&context)?;
        let columns = before.rows[0].cells.len();
        let previous = context.cell.row_index * columns + context.cell.cell_index - 1;
        Ok(Some(self.focus_table_cell(
            &context.cell.table_id,
            previous / columns,
            previous % columns,
            false,
        )?))
    }

    /// 横向结构键在单元格间移动；到达行边缘后交给外侧移动。
    pub(super) fn move_table_cell(
        &mut self,
        direction: Direction,
    ) -> Result<Option<Effect>, EditError> {
        let Some(context) = self.table_context() else {
            return Ok(None);
        };
        let table = self.current_table(&context)?;
        let columns = table.rows[0].cells.len();
        let cell_index = match direction {
            Direction::Backward => {
                if context.cell.cell_index == 0 {
                    return self.leave_table(context, false).map(Some);
                }
                context.cell.cell_index - 1
            }
            Direction::Forward => {
                if context.cell.cell_index + 1 >= columns {
                    return self.leave_table(context, true).map(Some);
                }
                context.cell.cell_index + 1
            }
        };
        Ok(Some(self.focus_table_cell(
            &context.cell.table_id,
            context.cell.row_index,
            cell_index,
            true,
        )?))
    }

    /// 纵向结构键按行移动；到达首/末行后离开表格。
    pub(super) fn move_table_row(
        &mut self,
        direction: Direction,
    ) -> Result<Option<Effect>, EditError> {
        let Some(context) = self.table_context() else {
            return Ok(None);
        };
        self.move_across_table_cell_boundary(&context.cell, TableArrowAxis::Vertical, direction)
            .map(Some)
    }

    /// 提供表格首末格的局部入口，外侧状态由共享块边界处理。
    pub(super) fn enter_table_at_edge(
        &mut self,
        table_id: &NodeId,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        let (row_index, cell_index) = if direction == Direction::Forward {
            (0, 0)
        } else {
            let Some(Block::Table(table)) =
                super::block_navigation::find_block_by_id(&self.document.body, table_id)
            else {
                return Err(EditError::InvalidPosition);
            };
            let row_index = table.rows.len() - 1;
            (row_index, table.rows[row_index].cells.len() - 1)
        };
        self.focus_table_cell(
            table_id,
            row_index,
            cell_index,
            direction == Direction::Forward,
        )
    }

    /// Escape 从当前单元格停靠到表格外后。
    pub(super) fn exit_table_for_escape(&mut self) -> Result<Option<Effect>, EditError> {
        let Some(context) = self.table_context() else {
            return Ok(None);
        };
        self.leave_table(context, true).map(Some)
    }

    /// 判断方向键是否落在单元格首/末内容边界；返回上下文供导航与 app 查询共用。
    /// 边界按 cell 块和逻辑文本偏移判定，不感知布局视觉行。
    fn table_arrow_boundary_state(
        &self,
        axis: TableArrowAxis,
        direction: Direction,
    ) -> Result<Option<TableContext>, EditError> {
        let Some(context) = self.table_context() else {
            return Ok(None);
        };
        let Some(location) = self
            .paragraph_index
            .get(&self.selection.focus.paragraph_id)
            .cloned()
        else {
            return Ok(None);
        };
        if location.container.innermost_cell().is_none() {
            return Ok(None);
        }
        // 允许表格本身位于块环境内；只拒绝单元格内再包一层环境造成的嵌套落点。
        let outside_cell_environment_depth = location
            .container
            .container_before_innermost_cell()
            .map_or(0, |container| container.environment_depth());
        if location.container.environment_depth() != outside_cell_environment_depth {
            return Ok(None);
        }
        let Some(block_index) = location.block_path.last().copied() else {
            return Ok(None);
        };
        let table = self.current_table(&context)?;
        let Some(row) = table.rows.get(context.cell.row_index) else {
            return Ok(None);
        };
        let Some(cell) = row.cells.get(context.cell.cell_index) else {
            return Ok(None);
        };
        let Some(Block::Paragraph(paragraph)) = cell.blocks.get(block_index) else {
            return Ok(None);
        };
        if paragraph.id != self.selection.focus.paragraph_id {
            return Ok(None);
        }
        let adjacent_index = match direction {
            Direction::Backward => block_index.checked_sub(1),
            Direction::Forward => block_index.checked_add(1),
        };
        let has_adjacent_block = adjacent_index.is_some_and(|index| {
            self.find_caret_sibling(&cell.blocks, index, direction)
                .is_some()
        });
        if has_adjacent_block {
            return Ok(None);
        }
        let at_boundary = match axis {
            TableArrowAxis::Horizontal => match direction {
                Direction::Backward => self.selection.focus.offset == 0,
                Direction::Forward => {
                    let text_len = crate::editing::text_map::TextMap::new(paragraph)
                        .map_err(|error| EditError::InvalidDocument(error.to_string()))?
                        .text
                        .len();
                    self.selection.focus.offset == text_len
                }
            },
            TableArrowAxis::Vertical => true,
        };
        Ok(at_boundary.then_some(context))
    }

    /// 方向键在单元格首/末内容边界进入相邻单元格；返回 `None` 表示不在边界、交给文本/外层导航。
    /// 表格边缘停靠到共享块外侧。
    pub(super) fn table_arrow_step(
        &mut self,
        axis: TableArrowAxis,
        direction: Direction,
    ) -> Result<Option<Effect>, EditError> {
        let Some(context) = self.table_arrow_boundary_state(axis, direction)? else {
            return Ok(None);
        };
        self.move_across_table_cell_boundary(&context.cell, axis, direction)
            .map(Some)
    }

    /// 单元格内容耗尽后按行列顺序选择下一个入口，整表边缘停在 BlockEdge。
    pub(super) fn move_across_table_cell_boundary(
        &mut self,
        cell: &TableCellPath,
        axis: TableArrowAxis,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        let Some(Block::Table(table)) =
            super::block_navigation::find_block_by_id(&self.document.body, &cell.table_id)
        else {
            return Err(EditError::InvalidPosition);
        };
        let columns = table
            .rows
            .first()
            .ok_or(EditError::InvalidPosition)?
            .cells
            .len();
        let rows = table.rows.len();
        let forward = direction == Direction::Forward;
        let next_index = |index: usize, limit: usize| {
            if forward {
                (index + 1 < limit).then_some(index + 1)
            } else {
                index.checked_sub(1)
            }
        };
        let target = match axis {
            TableArrowAxis::Horizontal => {
                next_index(cell.row_index * columns + cell.cell_index, rows * columns)
                    .map(|index| (index / columns, index % columns))
            }
            TableArrowAxis::Vertical => {
                next_index(cell.row_index, rows).map(|row| (row, cell.cell_index))
            }
        };
        if let Some((row, column)) = target {
            self.focus_table_cell(&cell.table_id, row, column, forward)
        } else {
            self.park_at_block_edge(cell.table_id.clone(), direction)
        }
    }

    /// 纯单元格移动只改变选区，不产生文档补丁、generation 或历史步。
    fn focus_table_cell(
        &mut self,
        table_id: &NodeId,
        row_index: usize,
        cell_index: usize,
        forward: bool,
    ) -> Result<Effect, EditError> {
        let Some(Block::Table(table)) =
            super::block_navigation::find_block_by_id(&self.document.body, table_id)
        else {
            return Err(EditError::InvalidPosition);
        };
        let cell = table
            .rows
            .get(row_index)
            .and_then(|row| row.cells.get(cell_index))
            .ok_or(EditError::InvalidPosition)?;
        let direction = if forward {
            Direction::Forward
        } else {
            Direction::Backward
        };
        let Some(target) = self.block_sequence_entry(&cell.blocks, direction) else {
            return Err(EditError::UnsupportedTableContent);
        };
        self.commit_block_entry(target)
    }

    fn current_table(&self, context: &TableContext) -> Result<&Table, EditError> {
        let blocks = block_list_in_container(&self.document.body, &context.table_container)
            .ok_or(EditError::InvalidPosition)?;
        blocks
            .get(context.table_sibling_index)
            .and_then(|block| match block {
                Block::Table(table) if table.id == context.cell.table_id => Some(table),
                _ => None,
            })
            .ok_or(EditError::InvalidPosition)
    }

    /// 整表替换一次提交：文档、索引、焦点与历史都走结构编辑入口。
    fn replace_table(
        &mut self,
        context: TableContext,
        before: Table,
        after: Table,
        target: Option<Pos>,
    ) -> Result<Effect, EditError> {
        self.apply_block_range(
            context.table_container,
            context.table_sibling_index,
            vec![Block::Table(before)],
            vec![Block::Table(after)],
            target,
        )
    }

    fn apply_block_range(
        &mut self,
        container: BlockContainerPath,
        sibling_index: usize,
        before: Vec<Block>,
        after: Vec<Block>,
        target_position: Option<Pos>,
    ) -> Result<Effect, EditError> {
        let Some(target_position) = target_position else {
            return Err(EditError::UnsupportedTableContent);
        };
        let mut candidate = self.document.clone();
        {
            let Some(blocks) = block_list_in_container_mut(&mut candidate.body, &container) else {
                return Err(EditError::InvalidPosition);
            };
            let Some(end) = sibling_index.checked_add(before.len()) else {
                return Err(EditError::InvalidPosition);
            };
            if blocks.get(sibling_index..end) != Some(before.as_slice()) {
                return Err(EditError::InvalidPosition);
            }
            blocks.splice(sibling_index..end, after.iter().cloned());
        }
        candidate
            .validate()
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
        let index = build_paragraph_index(&candidate.body);
        let location = index
            .get(&target_position.paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        if location.story != target_position.story {
            return Err(EditError::WrongStory);
        }
        let before_state = self.capture_structure_edit_before_state();
        self.document = candidate;
        self.clear_host_caret_position();
        let visual_affinity = visual_affinity_for(target_position.affinity);
        self.selection_visual_affinity = visual_affinity;
        self.selection_anchor_visual_affinity = visual_affinity;
        self.selection = Selection {
            anchor: target_position.clone(),
            focus: target_position,
        };
        self.finalize_structure_edit_with_boundary(
            index,
            DocumentPatch::BlockRange {
                container,
                sibling_index,
                before,
                after,
            },
            before_state,
        );
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn leave_table(&mut self, context: TableContext, forward: bool) -> Result<Effect, EditError> {
        self.park_at_block_edge(
            context.cell.table_id,
            if forward {
                Direction::Forward
            } else {
                Direction::Backward
            },
        )
    }
}

fn empty_table(rows: usize, columns: usize) -> Table {
    Table {
        id: NodeId::generate(),
        rows: (0..rows).map(|_| empty_row(columns)).collect(),
    }
}

fn empty_row(columns: usize) -> TableRow {
    TableRow {
        id: NodeId::generate(),
        cells: (0..columns).map(|_| empty_cell()).collect(),
    }
}

fn empty_cell() -> TableCell {
    TableCell {
        id: NodeId::generate(),
        blocks: vec![Block::Paragraph(Paragraph::empty(NodeId::generate()))],
    }
}

/// 表格单元格内可聚焦段落的首/末落点：forward 取首段起点，backward 取末段终点。
fn table_target(table: &Table, row_index: usize, cell_index: usize, forward: bool) -> Option<Pos> {
    let row = table.rows.get(row_index)?;
    let cell = row.cells.get(cell_index)?;
    let paragraph = if forward {
        cell.blocks.iter().find_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
    } else {
        cell.blocks.iter().rev().find_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
    }?;
    let text_len = crate::editing::text_map::TextMap::new(paragraph)
        .ok()?
        .text
        .len();
    Some(Pos {
        story: StoryId::TableCell {
            table_id: table.id.clone(),
            row_id: row.id.clone(),
            cell_id: cell.id.clone(),
        },
        paragraph_id: paragraph.id.clone(),
        offset: if forward { 0 } else { text_len },
        affinity: if forward {
            Affinity::Downstream
        } else {
            Affinity::Upstream
        },
    })
}
