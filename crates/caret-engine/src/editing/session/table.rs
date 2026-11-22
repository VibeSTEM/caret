//! 表格上下文：插入表格、行列结构命令与单元格间导航。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

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

/// 当前段落所在的表格单元格上下文。
pub(super) struct TableContext {
    /// 表格块所在容器（单元格之前的容器路径）。
    pub(super) table_container: BlockContainerPath,
    pub(super) table_sibling_index: usize,
    pub(super) cell: TableCellPath,
    pub(super) row_index: usize,
    pub(super) cell_index: usize,
}

impl Session {
    /// 解析当前焦点所在的表格单元格；焦点必须落在单元格块流内的段落上。
    pub(super) fn table_context(&self) -> Option<TableContext> {
        if self.selection.anchor != self.selection.focus {
            return None;
        }
        let location = self
            .paragraph_index
            .get(&self.selection.focus.paragraph_id)?;
        let cell = location.container.innermost_cell()?.clone();
        let table_container = location.container.container_before_innermost_cell()?;
        let blocks = block_list_in_container(&self.document.body, &table_container)?;
        let table_sibling_index = blocks
            .iter()
            .position(|block| matches!(block, Block::Table(table) if table.id == cell.table_id))?;
        Some(TableContext {
            table_container,
            table_sibling_index,
            row_index: cell.row_index,
            cell_index: cell.cell_index,
            cell,
        })
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
        let before = self.current_table(&context)?;
        let columns = before
            .rows
            .get(context.row_index)
            .ok_or(EditError::InvalidPosition)?
            .cells
            .len();
        let mut table = before.clone();
        let insert_at = match direction {
            Direction::Backward => context.row_index,
            Direction::Forward => context.row_index + 1,
        };
        table.rows.insert(insert_at, empty_row(columns));
        let target = table_target(&table, insert_at, context.cell_index, true);
        self.replace_table(context, before, table, target)
    }

    pub(super) fn delete_table_row(&mut self) -> Result<Effect, EditError> {
        let context = self.table_context().ok_or(EditError::InvalidPosition)?;
        let before = self.current_table(&context)?;
        if before.rows.len() <= 1 {
            return Ok(Effect::default());
        }
        let mut table = before.clone();
        table.rows.remove(context.row_index);
        let target_row = context.row_index.min(table.rows.len() - 1);
        let target = table_target(&table, target_row, context.cell_index, true);
        self.replace_table(context, before, table, target)
    }

    pub(super) fn insert_table_column(
        &mut self,
        direction: Direction,
    ) -> Result<Effect, EditError> {
        let context = self.table_context().ok_or(EditError::InvalidPosition)?;
        let before = self.current_table(&context)?;
        let mut table = before.clone();
        let insert_at = match direction {
            Direction::Backward => context.cell_index,
            Direction::Forward => context.cell_index + 1,
        };
        for row in &mut table.rows {
            row.cells.insert(insert_at, empty_cell());
        }
        let target = table_target(&table, context.row_index, insert_at, true);
        self.replace_table(context, before, table, target)
    }

    pub(super) fn delete_table_column(&mut self) -> Result<Effect, EditError> {
        let context = self.table_context().ok_or(EditError::InvalidPosition)?;
        let before = self.current_table(&context)?;
        if before.rows.first().is_none_or(|row| row.cells.len() <= 1) {
            return Ok(Effect::default());
        }
        let mut table = before.clone();
        for row in &mut table.rows {
            if context.cell_index < row.cells.len() {
                row.cells.remove(context.cell_index);
            }
        }
        let target_cell = context.cell_index.min(table.rows[0].cells.len() - 1);
        let target = table_target(&table, context.row_index, target_cell, true);
        self.replace_table(context, before, table, target)
    }

    /// Tab 到下一格；最后一格再按 Tab 增加一行并落到新行首格。
    pub(super) fn table_tab_forward(&mut self) -> Result<Option<Effect>, EditError> {
        let Some(context) = self.table_context() else {
            return Ok(None);
        };
        let before = self.current_table(&context)?;
        let columns = before.rows[0].cells.len();
        let index = context.row_index * columns + context.cell_index + 1;
        if index < before.rows.len() * columns {
            return Ok(Some(self.focus_table_cell(
                &before,
                index / columns,
                index % columns,
                true,
            )?));
        }
        let mut table = before.clone();
        table.rows.push(empty_row(columns));
        let last_row = table.rows.len() - 1;
        let target = table_target(&table, last_row, 0, true);
        Ok(Some(self.replace_table(context, before, table, target)?))
    }

    /// Shift+Tab 到上一格；首格保持原位。
    pub(super) fn table_tab_backward(&mut self) -> Result<Option<Effect>, EditError> {
        let Some(context) = self.table_context() else {
            return Ok(None);
        };
        if context.row_index == 0 && context.cell_index == 0 {
            return Ok(Some(Effect::default()));
        }
        let before = self.current_table(&context)?;
        let columns = before.rows[0].cells.len();
        let previous = context.row_index * columns + context.cell_index - 1;
        Ok(Some(self.focus_table_cell(
            &before,
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
                if context.cell_index == 0 {
                    return self.leave_table(context, false);
                }
                context.cell_index - 1
            }
            Direction::Forward => {
                if context.cell_index + 1 >= columns {
                    return self.leave_table(context, true);
                }
                context.cell_index + 1
            }
        };
        Ok(Some(self.focus_table_cell(
            &table,
            context.row_index,
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
        let table = self.current_table(&context)?;
        let rows = table.rows.len();
        let row_index = match direction {
            Direction::Backward => {
                if context.row_index == 0 {
                    return self.leave_table(context, false);
                }
                context.row_index - 1
            }
            Direction::Forward => {
                if context.row_index + 1 >= rows {
                    return self.leave_table(context, true);
                }
                context.row_index + 1
            }
        };
        Ok(Some(self.focus_table_cell(
            &table,
            row_index,
            context.cell_index,
            true,
        )?))
    }

    /// 方向键在单元格首/末内容边界进入相邻单元格；返回 `None` 表示不在边界、交给文本/外层导航。
    /// 表格边缘仍消费按键（`Some(default)`），不让普通方向键把光标带出表格。
    /// 判断方向键是否落在单元格首/末内容边界；返回上下文供导航与 app 查询共用。
    fn table_arrow_boundary_state(
        &self,
        axis: TableArrowAxis,
        direction: Direction,
    ) -> Result<Option<(TableContext, usize)>, EditError> {
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
        let Some(_cell) = location.container.innermost_cell() else {
            return Ok(None);
        };
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
        let Some(row) = table.rows.get(context.row_index) else {
            return Ok(None);
        };
        let Some(cell) = row.cells.get(context.cell_index) else {
            return Ok(None);
        };
        let Some(Block::Paragraph(paragraph)) = cell.blocks.get(block_index) else {
            return Ok(None);
        };
        if paragraph.id != self.selection.focus.paragraph_id {
            return Ok(None);
        }
        let text_len = crate::editing::text_map::TextMap::new(paragraph)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?
            .text
            .len();
        let at_boundary = match axis {
            TableArrowAxis::Horizontal => match direction {
                Direction::Backward => block_index == 0 && self.selection.focus.offset == 0,
                Direction::Forward => {
                    block_index + 1 == cell.blocks.len() && self.selection.focus.offset == text_len
                }
            },
            TableArrowAxis::Vertical => match direction {
                Direction::Backward => block_index == 0,
                Direction::Forward => block_index + 1 == cell.blocks.len(),
            },
        };
        Ok(at_boundary.then_some((context, block_index)))
    }

    /// app 视觉导航在候选为空时查询：当前纵向方向键是否应交给共享 cell 导航。
    pub fn table_vertical_arrow_boundary(&self, direction: Direction) -> bool {
        self.table_arrow_boundary_state(TableArrowAxis::Vertical, direction)
            .is_ok_and(|state| state.is_some())
    }

    /// 方向键在单元格首/末内容边界进入相邻单元格；返回 `None` 表示不在边界、交给文本/外层导航。
    /// 表格边缘仍消费按键（`Some(default)`），不让普通方向键把光标带出表格。
    pub(super) fn table_arrow_step(
        &mut self,
        axis: TableArrowAxis,
        direction: Direction,
    ) -> Result<Option<Effect>, EditError> {
        let Some((context, _block_index)) = self.table_arrow_boundary_state(axis, direction)?
        else {
            return Ok(None);
        };
        let table = self.current_table(&context)?;
        let columns = table.rows.first().map_or(0, |row| row.cells.len());
        let (target_row, target_cell) = match axis {
            TableArrowAxis::Horizontal => {
                let index = context.row_index * columns + context.cell_index;
                if direction == Direction::Backward {
                    if index == 0 {
                        return Ok(Some(Effect::default()));
                    }
                    let previous = index - 1;
                    (previous / columns, previous % columns)
                } else {
                    let next = index + 1;
                    if next >= table.rows.len() * columns {
                        return Ok(Some(Effect::default()));
                    }
                    (next / columns, next % columns)
                }
            }
            TableArrowAxis::Vertical => {
                let target_row = match direction {
                    Direction::Backward => {
                        let Some(row) = context.row_index.checked_sub(1) else {
                            return Ok(Some(Effect::default()));
                        };
                        row
                    }
                    Direction::Forward => {
                        let row = context.row_index + 1;
                        if row >= table.rows.len() {
                            return Ok(Some(Effect::default()));
                        }
                        row
                    }
                };
                (target_row, context.cell_index)
            }
        };
        let Some(target) = table_target(
            &table,
            target_row,
            target_cell,
            direction == Direction::Forward,
        ) else {
            return Ok(Some(Effect::default()));
        };
        if !self.paragraph_index.contains_key(&target.paragraph_id) {
            return Ok(Some(Effect::default()));
        }
        self.selection = Selection {
            anchor: target.clone(),
            focus: target,
        };
        Ok(Some(Effect {
            selection_changed: true,
            ..Effect::default()
        }))
    }

    /// 纯单元格移动只改变选区，不产生文档补丁、generation 或历史步。
    fn focus_table_cell(
        &mut self,
        table: &Table,
        row_index: usize,
        cell_index: usize,
        forward: bool,
    ) -> Result<Effect, EditError> {
        let Some(target) = table_target(table, row_index, cell_index, forward) else {
            return Err(EditError::UnsupportedTableContent);
        };
        if !self.paragraph_index.contains_key(&target.paragraph_id) {
            return Err(EditError::InvalidPosition);
        }
        self.selection = Selection {
            anchor: target.clone(),
            focus: target,
        };
        Ok(Effect {
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn current_table(&self, context: &TableContext) -> Result<Table, EditError> {
        let blocks = block_list_in_container(&self.document.body, &context.table_container)
            .ok_or(EditError::InvalidPosition)?;
        blocks
            .iter()
            .find_map(|block| match block {
                Block::Table(table) if table.id == context.cell.table_id => Some(table.clone()),
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
        let before_selection = self.selection.clone();
        let before_anchor_visual_affinity = self.selection_anchor_visual_affinity;
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
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
        self.document = candidate;
        self.selection = Selection {
            anchor: target_position.clone(),
            focus: target_position,
        };
        let index = build_paragraph_index(&self.document.body);
        self.finalize_structure_edit(
            index,
            DocumentPatch::BlockRange {
                container,
                sibling_index,
                before,
                after,
            },
            before_selection,
            before_anchor_visual_affinity,
            before_visual_affinity,
            before_state_id,
        );
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn leave_table(
        &mut self,
        context: TableContext,
        forward: bool,
    ) -> Result<Option<Effect>, EditError> {
        let Some(blocks) = block_list_in_container(&self.document.body, &context.table_container)
        else {
            return Ok(None);
        };
        let neighbour_index = if forward {
            context.table_sibling_index.checked_add(1)
        } else {
            context.table_sibling_index.checked_sub(1)
        };
        let Some(neighbour_index) = neighbour_index else {
            return Ok(None);
        };
        let Some(Block::Paragraph(paragraph)) = blocks.get(neighbour_index) else {
            return Ok(None);
        };
        let text_len = crate::editing::text_map::TextMap::new(paragraph)
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?
            .text
            .len();
        let location = self
            .paragraph_index
            .get(&paragraph.id)
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        let position = Pos {
            story: location.story.clone(),
            paragraph_id: paragraph.id.clone(),
            offset: if forward { 0 } else { text_len },
            affinity: if forward {
                Affinity::Downstream
            } else {
                Affinity::Upstream
            },
        };
        self.selection = Selection {
            anchor: position.clone(),
            focus: position,
        };
        Ok(Some(Effect {
            selection_changed: true,
            ..Effect::default()
        }))
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
