//! 定义命令执行结果和输入法组字状态。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::position::PositionMap;
use caret_doc::NodeId;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Effect {
    pub document_changed: bool,
    pub selection_changed: bool,
    pub composition_changed: bool,
    pub position_map: Option<PositionMap>,
    /// 仅当历史补丁只影响同一段落时提供，可供应用层安全复用其余布局。
    pub changed_paragraph_id: Option<NodeId>,
    /// 由 Session 仲裁后交给 app 适配层执行的系统剪贴板动作。
    pub clipboard_action: Option<ClipboardAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardAction {
    Copy,
    Paste,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputComposition {
    pub id: u64,
    pub preedit: String,
}
