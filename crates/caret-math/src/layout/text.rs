//! 数学 TextFlow 与正文塑形器之间的中立文本岛契约。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{MathPathStep, NodeId, Run};

use super::MathColor;

/// 一个 Run 及其在 MathFlow 中的稳定来源位置。
#[derive(Debug, Clone, PartialEq)]
pub struct MathTextRun {
    /// 原始 Run；由排版语义生成的空格或空流锚点为 `None`。
    pub run: Option<Run>,
    /// 当前 TextFlow 显示文本；保留与原 Run 一致的 UTF-8 字节范围。
    pub text: String,
    pub scope_id: NodeId,
    pub path: Vec<MathPathStep>,
    pub run_index: Option<usize>,
    pub is_text_flow: bool,
    pub style: MathTextRunStyle,
}

/// 经过数学行内格式环境继承后的正文样式；`None` 表示继承相邻环境值。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MathTextRunStyle {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub font_family: Option<String>,
}

/// 整个连续文本岛作为一个请求提交，以保留跨 Run 的组合字符和双向排版。
#[derive(Debug, Clone, PartialEq)]
pub struct MathTextRequest {
    pub font_size_pt: f64,
    pub color: Option<MathColor>,
    pub runs: Vec<MathTextRun>,
    /// 塑形后仍须保留结构身份的文本/格式节点。
    pub containers: Vec<MathTextContainer>,
}

/// 数学格式容器及其在拼接文本岛中的 UTF-8 字节范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathTextContainer {
    pub id: Option<NodeId>,
    pub scope_id: NodeId,
    pub path: Vec<MathPathStep>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub is_flow: bool,
    pub is_container: bool,
}

/// 一个 Run 在文本岛基线坐标系中的塑形范围。
#[derive(Debug, Clone, PartialEq)]
pub struct MathTextRunBox {
    pub id: Option<NodeId>,
    pub scope_id: NodeId,
    pub path: Vec<MathPathStep>,
    pub run_index: Option<usize>,
    pub is_text_flow: bool,
    pub is_flow: bool,
    pub is_container: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub baseline: f64,
}

/// engine 持有的正文 Parley 布局 token 及数学需要的自然几何。
#[derive(Debug, Clone, PartialEq)]
pub struct MathTextShape {
    pub token: u64,
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
    pub requested_family_missing: bool,
    pub runs: Vec<MathTextRunBox>,
    pub containers: Vec<MathTextRunBox>,
}

/// 正文 shaping 回调。实现位于 caret-engine，由其持有 FontCatalog 和 Parley sidecar。
pub trait MathTextShaper {
    fn shape(&mut self, request: MathTextRequest) -> Result<MathTextShape, MathTextShapeError>;
}

/// 正文 shaping 失败或返回不可用布局时的稳定错误类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathTextShapeError {
    UnsupportedInput,
    InvalidMetrics,
    Failed,
}
