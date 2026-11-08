//! 声明视觉导航提交时使用的方向意图。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

/// 由宿主布局提供几何候选前交给 Session 仲裁的导航意图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationIntent {
    Horizontal,
    Vertical,
    VisualLineEdge,
}
