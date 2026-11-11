//! 表达 Block 与 Inline 宿主共享的外前、内部和外后焦点位置。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::NodeId;

use super::{
    MathEditTarget, MathPositionAffinity, MathPositionHost, MathSequencePath, MathTextFlowPath, Pos,
};

/// 活动宿主内部由对应领域 adapter 持有的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCaretLocalPosition {
    /// Paragraph TextMap 或 InlineEnvironment 中的真实逻辑位置。
    Paragraph(Pos),
    /// 数学树、TextFlow 或源码分支中的窄局部坐标。
    Math(MathCaretLocalPosition),
}

/// 数学宿主当前分支中的编辑位置；只在当前有效会话和文档代际内有效。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathCaretLocalPosition {
    /// 公式宿主 ID；Align 的行列身份由局部坐标中的 MathPositionHost 保留。
    pub target: MathEditTarget,
    /// 当前数学编辑载体中的局部位置。
    pub coordinate: MathCaretCoordinate,
}

/// 数学宿主的具体局部坐标表示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathCaretCoordinate {
    /// LaTeX 源码中的 UTF-8 字节偏移。
    SourceUtf8 {
        offset: usize,
        /// 未解析草稿身份及内容修订；已提交源码投影没有独立草稿修订。
        draft_revision: Option<(NodeId, u64)>,
        /// 已提交源码投影身份；未解析草稿位置由草稿身份绑定。
        projection_id: Option<NodeId>,
    },
    /// 数学 TextFlow 中的 UTF-8 偏移及其流路径。
    TextFlowUtf8 {
        path: MathTextFlowPath,
        offset: usize,
        affinity: MathPositionAffinity,
    },
    /// MathTree 内容序列中的边界索引及其结构路径。
    TreeBoundary {
        /// InlineMath、Equation 或 Align 的具体公式宿主；AlignCell 保留行列身份。
        host: MathPositionHost,
        path: MathSequencePath,
        index: usize,
        affinity: MathPositionAffinity,
    },
}

/// 宿主内由领域适配器提供的局部位置，不限定其坐标表示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCaretPosition<P> {
    /// 宿主内容之前的可停靠位置。
    Before {
        host_id: NodeId,
        kind: HostCaretKind,
    },
    /// 宿主内部的有效局部位置。
    Inside {
        host_id: NodeId,
        kind: HostCaretKind,
        local: P,
    },
    /// 宿主内容之后的可停靠位置。
    After {
        host_id: NodeId,
        kind: HostCaretKind,
    },
}

/// 区分共用三态的 Inline 与 Block 宿主。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCaretKind {
    /// Paragraph 内的显式行内宿主。
    Inline,
    /// 独立的块级宿主。
    Block,
}

/// 水平移动在宿主边缘产生的共享状态迁移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCaretTransition {
    /// 从外前进入内部起点。
    EnterStart,
    /// 从外后进入内部终点。
    EnterEnd,
    /// 从内部起点退到外前。
    LeaveBefore,
    /// 从内部终点退到外后。
    LeaveAfter,
    /// 当前宿主内部继续移动。
    MoveInside,
    /// 当前宿主边缘之外继续移动。
    MoveOutside,
}

/// 当前局部位置相对宿主边缘的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCaretEdge {
    /// 宿主内容之前。
    Before,
    /// 宿主内部。
    Inside,
    /// 宿主内容之后。
    After,
}

impl HostCaretTransition {
    /// 解析 TextMap 的格式环境边界；边界亲和性只决定从哪一侧转换。
    pub(crate) fn for_text_boundary(
        forward: bool,
        inside_nonempty_host: bool,
        at_start: bool,
        at_end: bool,
    ) -> Self {
        let edge = match (forward, inside_nonempty_host) {
            (true, true) => HostCaretEdge::Inside,
            (true, false) => HostCaretEdge::Before,
            (false, true) => HostCaretEdge::Inside,
            (false, false) => HostCaretEdge::After,
        };
        Self::classify_horizontal(edge, forward, at_start, at_end)
    }
}

impl HostCaretTransition {
    /// 按宿主三态和局部首尾位置统一分类水平移动。
    pub fn classify_horizontal(
        edge: HostCaretEdge,
        forward: bool,
        at_start: bool,
        at_end: bool,
    ) -> Self {
        match (edge, forward, at_start, at_end) {
            (HostCaretEdge::Before, true, _, _) => Self::EnterStart,
            (HostCaretEdge::After, false, _, _) => Self::EnterEnd,
            (HostCaretEdge::Inside, false, true, _) => Self::LeaveBefore,
            (HostCaretEdge::Inside, true, _, true) => Self::LeaveAfter,
            (HostCaretEdge::Inside, _, _, _) => Self::MoveInside,
            (HostCaretEdge::Before | HostCaretEdge::After, _, _, _) => Self::MoveOutside,
        }
    }
}

impl<P> HostCaretPosition<P> {
    /// 从既有退出方向构造等价的外侧宿主停靠点。
    pub fn outside(host_id: NodeId, direction: crate::editing::Direction) -> Self {
        Self::outside_with_kind(host_id, direction, HostCaretKind::Block)
    }

    /// 按方向和宿主层级构造外侧停靠点。
    pub fn outside_with_kind(
        host_id: NodeId,
        direction: crate::editing::Direction,
        kind: HostCaretKind,
    ) -> Self {
        match direction {
            crate::editing::Direction::Backward => Self::Before { host_id, kind },
            crate::editing::Direction::Forward => Self::After { host_id, kind },
        }
    }

    /// 返回稳定宿主所属的层级。
    pub fn kind(&self) -> HostCaretKind {
        match self {
            Self::Before { kind, .. } | Self::Inside { kind, .. } | Self::After { kind, .. } => {
                *kind
            }
        }
    }

    /// 按宿主局部位置是否位于首尾，统一判定左右边界转换。
    pub fn horizontal_transition(
        &self,
        forward: bool,
        at_start: bool,
        at_end: bool,
    ) -> HostCaretTransition {
        match self {
            Self::Before { .. } => HostCaretTransition::classify_horizontal(
                HostCaretEdge::Before,
                forward,
                at_start,
                at_end,
            ),
            Self::Inside { .. } => HostCaretTransition::classify_horizontal(
                HostCaretEdge::Inside,
                forward,
                at_start,
                at_end,
            ),
            Self::After { .. } => HostCaretTransition::classify_horizontal(
                HostCaretEdge::After,
                forward,
                at_start,
                at_end,
            ),
        }
    }

    /// 返回位置所属的稳定宿主 ID。
    pub fn host_id(&self) -> &NodeId {
        match self {
            Self::Before { host_id, .. } | Self::After { host_id, .. } => host_id,
            Self::Inside { host_id, .. } => host_id,
        }
    }

    /// 将外前/外后状态投影为与旧接口一致的外侧停靠方向。
    pub fn outside_direction(&self) -> Option<crate::editing::Direction> {
        match self {
            Self::Before { .. } => Some(crate::editing::Direction::Backward),
            Self::After { .. } => Some(crate::editing::Direction::Forward),
            Self::Inside { .. } => None,
        }
    }

    /// 将外侧停靠点投影为旧边界接口使用的宿主 ID 与停靠方向。
    pub fn as_outside(&self) -> Option<(&NodeId, crate::editing::Direction)> {
        self.outside_direction()
            .map(|direction| (self.host_id(), direction))
    }
}

#[cfg(test)]
#[path = "host_caret/tests.rs"]
mod tests;
