//! 表格块：行、单元格与单元格内的通用块序列。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{Block, NodeId};

/// 表格块；行与单元格持有稳定身份，单元格内容是普通块序列。
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub id: NodeId,
    pub rows: Vec<TableRow>,
}

/// 表格行；单元格顺序即列顺序。
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub id: NodeId,
    pub cells: Vec<TableCell>,
}

/// 表格单元格；内容与文档正文块流同构，可包含多段、公式与环境。
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    pub id: NodeId,
    pub blocks: Vec<Block>,
}
