//! 定义命令执行结果和输入法组字状态。

use super::position::PositionMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Effect {
    pub document_changed: bool,
    pub selection_changed: bool,
    pub composition_changed: bool,
    pub position_map: Option<PositionMap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputComposition {
    pub id: u64,
    pub preedit: String,
}
