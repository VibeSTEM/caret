//! 提供不依赖源码投影的 MathTree 结构位置及只读解析。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::MathTextFlowPathStep;
use super::optional_placeholder_operand;
use super::path::MathTextFlowPath;
use caret_doc::{MathContent, MathTree, NodeId};

/// 位置在相邻结构边界间的归属方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathPositionAffinity {
    Upstream,
    Downstream,
}

/// 标识位置所处的公式宿主；Align cell 使用行与单元格的稳定节点标识。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathPositionHost {
    InlineMath { owner_id: NodeId },
    Equation { owner_id: NodeId },
    AlignCell { row_id: NodeId, cell_id: NodeId },
}

/// 从当前流进入一个嵌套子流的类型化路径步骤。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathSequenceStep {
    /// 进入指定数学节点的操作数 MathFlow。
    Operand {
        content_index: usize,
        operand_index: usize,
    },
    /// 进入指定行内环境的 children 序列。
    EnvironmentChildren { content_index: usize },
}

/// TextFlow 内侧边界对应的相邻数学内容序列位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathTextFlowBoundary {
    Start,
    End,
}

/// 从 MathTree 根流开始的结构路径；空路径指向根流。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MathSequencePath {
    steps: Vec<MathSequenceStep>,
}

impl MathSequencePath {
    /// 创建指向根流的位置路径。
    pub const fn root() -> Self {
        Self { steps: Vec::new() }
    }

    /// 创建包含给定嵌套步骤的路径。
    pub fn new(steps: Vec<MathSequenceStep>) -> Self {
        Self { steps }
    }

    /// 返回从根流开始的路径步骤。
    pub fn steps(&self) -> &[MathSequenceStep] {
        &self.steps
    }

    pub(in crate::editing::session) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.steps.capacity() * std::mem::size_of::<MathSequenceStep>()
    }
}

/// 指向 MathFlow 有序内容序列中一个边界的结构位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathTreePosition {
    pub host: MathPositionHost,
    pub path: MathSequencePath,
    pub offset: usize,
    pub affinity: MathPositionAffinity,
}

impl MathTreePosition {
    /// 将 TextFlow 起止边界转换为其父 MathFlow 中的结构边界。
    ///
    /// 输入路径必须是 `MathTextFlowPath::steps()` 的完整路径；Align 宿主必须与
    /// 路径携带的 cell 坐标成对传入。失败时不产生可部分安装的位置。
    pub fn from_text_flow_boundary(
        tree: &MathTree,
        host: MathPositionHost,
        text_path: &[MathTextFlowPathStep],
        align_cell: Option<(usize, usize)>,
        boundary: MathTextFlowBoundary,
        affinity: MathPositionAffinity,
    ) -> Result<Self, MathTreePositionError> {
        let host_matches_cell = match (&host, align_cell) {
            (MathPositionHost::AlignCell { cell_id, .. }, Some(_)) => *cell_id == tree.root.id,
            (MathPositionHost::InlineMath { .. } | MathPositionHost::Equation { .. }, None) => true,
            _ => false,
        };
        if !host_matches_cell {
            return Err(MathTreePositionError::HostMismatch);
        }

        let mut sequence_steps = Vec::with_capacity(text_path.len() / 2);
        let mut content_index = None;
        for (depth, step) in text_path.iter().enumerate() {
            match *step {
                MathTextFlowPathStep::Content(index) => {
                    if content_index.replace(index).is_some() {
                        return Err(MathTreePositionError::InvalidTextFlowPath { depth });
                    }
                }
                MathTextFlowPathStep::Operand(operand_index) => {
                    let Some(parent_index) = content_index.take() else {
                        return Err(MathTreePositionError::InvalidTextFlowPath { depth });
                    };
                    sequence_steps.push(MathSequenceStep::Operand {
                        content_index: parent_index,
                        operand_index,
                    });
                }
                MathTextFlowPathStep::EnvironmentChild(index) => {
                    let Some(parent_index) = content_index.take() else {
                        return Err(MathTreePositionError::InvalidTextFlowPath { depth });
                    };
                    sequence_steps.push(MathSequenceStep::EnvironmentChildren {
                        content_index: parent_index,
                    });
                    content_index = Some(index);
                }
            }
        }
        let Some(text_index) = content_index else {
            return Err(MathTreePositionError::InvalidTextFlowPath {
                depth: text_path.len(),
            });
        };
        let path = MathSequencePath::new(sequence_steps);
        let offset = match boundary {
            MathTextFlowBoundary::Start => text_index,
            MathTextFlowBoundary::End => {
                text_index
                    .checked_add(1)
                    .ok_or(MathTreePositionError::OffsetOutOfBounds {
                        offset: text_index,
                        content_len: usize::MAX,
                    })?
            }
        };
        let position = Self {
            host,
            path,
            offset,
            affinity,
        };
        let content = position.resolve(tree)?;
        if !matches!(content.get(text_index), Some(MathContent::Text(_))) {
            return Err(MathTreePositionError::InvalidTextFlowPath {
                depth: text_path.len(),
            });
        }
        Ok(position)
    }

    /// 校验路径与边界，并返回位置所属的有序数学内容序列。
    ///
    /// 错容器、越界路径和大于内容项数量的 offset 均返回可恢复错误。
    pub fn resolve<'a>(
        &self,
        tree: &'a MathTree,
    ) -> Result<&'a [MathContent], MathTreePositionError> {
        let mut content = tree.root.content.as_slice();
        for (depth, step) in self.path.steps.iter().enumerate() {
            content = match *step {
                MathSequenceStep::Operand {
                    content_index,
                    operand_index,
                } => match content.get(content_index) {
                    Some(MathContent::Math(node)) => node
                        .children
                        .get(operand_index)
                        .map(|flow| flow.content.as_slice()),
                    _ => None,
                },
                MathSequenceStep::EnvironmentChildren { content_index } => {
                    match content.get(content_index) {
                        Some(MathContent::Environment(environment)) => {
                            Some(environment.children.as_slice())
                        }
                        _ => None,
                    }
                }
            }
            .ok_or(MathTreePositionError::InvalidPath { depth })?;
        }
        if self.offset > content.len() {
            return Err(MathTreePositionError::OffsetOutOfBounds {
                offset: self.offset,
                content_len: content.len(),
            });
        }
        Ok(content)
    }

    pub(in crate::editing::session) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.path.estimated_bytes()
            + match &self.host {
                MathPositionHost::InlineMath { owner_id }
                | MathPositionHost::Equation { owner_id } => owner_id.as_str().len(),
                MathPositionHost::AlignCell { row_id, cell_id } => {
                    row_id.as_str().len() + cell_id.as_str().len()
                }
            }
    }
}

pub(super) enum MathTreeCaretMove {
    Position(MathTreePosition),
    TextFlow(MathTextFlowPath, usize),
    Boundary,
}

pub(super) fn advance_math_tree_position(
    tree: &MathTree,
    position: &MathTreePosition,
    forward: bool,
    align_cell: Option<(usize, usize)>,
) -> Result<MathTreeCaretMove, MathTreePositionError> {
    let contents = position.resolve(tree)?;
    let adjacent_index = if forward {
        (position.offset < contents.len()).then_some(position.offset)
    } else {
        position.offset.checked_sub(1)
    };
    if let Some(content_index) = adjacent_index {
        let content =
            contents
                .get(content_index)
                .ok_or(MathTreePositionError::OffsetOutOfBounds {
                    offset: content_index,
                    content_len: contents.len(),
                })?;
        return match content {
            MathContent::Text(text) => {
                let path = text_path_for_content(position, content_index, align_cell);
                let cursor = if forward {
                    0
                } else {
                    text.runs.iter().map(|run| run.text.len()).sum()
                };
                Ok(MathTreeCaretMove::TextFlow(path, cursor))
            }
            MathContent::Environment(environment) if !environment.children.is_empty() => {
                let path = child_sequence_path(
                    position,
                    MathSequenceStep::EnvironmentChildren { content_index },
                );
                let child_contents = environment.children.as_slice();
                enter_sequence_edge(position, path, child_contents, forward, align_cell)
            }
            MathContent::Math(node) if !node.children.is_empty() => {
                let operand_index = if forward {
                    (0..node.children.len())
                        .find(|&index| !optional_placeholder_operand(node, index))
                } else {
                    (0..node.children.len())
                        .rev()
                        .find(|&index| !optional_placeholder_operand(node, index))
                };
                if let Some(operand_index) = operand_index {
                    let path = child_sequence_path(
                        position,
                        MathSequenceStep::Operand {
                            content_index,
                            operand_index,
                        },
                    );
                    let child_contents = node.children[operand_index].content.as_slice();
                    enter_sequence_edge(position, path, child_contents, forward, align_cell)
                } else {
                    let offset = if forward {
                        content_index + 1
                    } else {
                        content_index
                    };
                    Ok(MathTreeCaretMove::Position(position_at(
                        position,
                        position.path.clone(),
                        offset,
                    )))
                }
            }
            MathContent::Math(_)
            | MathContent::Environment(_)
            | MathContent::SoftBreak(_)
            | MathContent::Opaque(_) => {
                let offset = if forward {
                    content_index + 1
                } else {
                    content_index
                };
                Ok(MathTreeCaretMove::Position(position_at(
                    position,
                    position.path.clone(),
                    offset,
                )))
            }
        };
    }

    climb_math_tree_position(tree, position, forward, align_cell)
}

pub(super) fn resolve_sequence_mut<'a>(
    tree: &'a mut MathTree,
    path: &MathSequencePath,
) -> Option<&'a mut Vec<MathContent>> {
    let mut contents = &mut tree.root.content;
    for step in path.steps() {
        contents = match *step {
            MathSequenceStep::Operand {
                content_index,
                operand_index,
            } => match contents.get_mut(content_index)? {
                MathContent::Math(node) => &mut node.children.get_mut(operand_index)?.content,
                _ => return None,
            },
            MathSequenceStep::EnvironmentChildren { content_index } => {
                match contents.get_mut(content_index)? {
                    MathContent::Environment(environment) => &mut environment.children,
                    _ => return None,
                }
            }
        };
    }
    Some(contents)
}

fn enter_sequence_edge(
    position: &MathTreePosition,
    path: MathSequencePath,
    contents: &[MathContent],
    forward: bool,
    align_cell: Option<(usize, usize)>,
) -> Result<MathTreeCaretMove, MathTreePositionError> {
    let offset = if forward { 0 } else { contents.len() };
    let child_position = position_at(position, path, offset);
    if let Some(content_index) = if forward {
        (offset < contents.len()).then_some(offset)
    } else {
        offset.checked_sub(1)
    } && let MathContent::Text(text) = &contents[content_index]
    {
        let path = text_path_for_content(&child_position, content_index, align_cell);
        let cursor = if forward {
            0
        } else {
            text.runs.iter().map(|run| run.text.len()).sum()
        };
        return Ok(MathTreeCaretMove::TextFlow(path, cursor));
    }
    Ok(MathTreeCaretMove::Position(child_position))
}

fn climb_math_tree_position(
    tree: &MathTree,
    position: &MathTreePosition,
    forward: bool,
    align_cell: Option<(usize, usize)>,
) -> Result<MathTreeCaretMove, MathTreePositionError> {
    let Some((last, parent_steps)) = position.path.steps().split_last() else {
        return Ok(MathTreeCaretMove::Boundary);
    };
    let parent_path = MathSequencePath::new(parent_steps.to_vec());
    match *last {
        MathSequenceStep::Operand {
            content_index,
            operand_index,
        } => {
            let parent = position_at(position, parent_path.clone(), content_index);
            let contents = parent.resolve(tree)?;
            let Some(MathContent::Math(node)) = contents.get(content_index) else {
                return Err(MathTreePositionError::InvalidPath {
                    depth: parent_steps.len(),
                });
            };
            let sibling = if forward {
                (operand_index.saturating_add(1)..node.children.len())
                    .find(|&next| !optional_placeholder_operand(node, next))
            } else {
                (0..operand_index)
                    .rev()
                    .find(|&previous| !optional_placeholder_operand(node, previous))
            };
            if let Some(operand_index) = sibling {
                let sibling_path = child_sequence_path(
                    &parent,
                    MathSequenceStep::Operand {
                        content_index,
                        operand_index,
                    },
                );
                let sibling_contents = &node.children[operand_index].content;
                return enter_sequence_edge(
                    position,
                    sibling_path,
                    sibling_contents,
                    forward,
                    align_cell,
                );
            }
            let offset = if forward {
                content_index
                    .checked_add(1)
                    .ok_or(MathTreePositionError::OffsetOutOfBounds {
                        offset: content_index,
                        content_len: usize::MAX,
                    })?
            } else {
                content_index
            };
            Ok(MathTreeCaretMove::Position(position_at(
                position,
                parent_path,
                offset,
            )))
        }
        MathSequenceStep::EnvironmentChildren { content_index } => {
            let offset = if forward {
                content_index
                    .checked_add(1)
                    .ok_or(MathTreePositionError::OffsetOutOfBounds {
                        offset: content_index,
                        content_len: usize::MAX,
                    })?
            } else {
                content_index
            };
            Ok(MathTreeCaretMove::Position(position_at(
                position,
                parent_path,
                offset,
            )))
        }
    }
}

pub(super) fn child_sequence_path(
    position: &MathTreePosition,
    step: MathSequenceStep,
) -> MathSequencePath {
    let mut steps = position.path.steps().to_vec();
    steps.push(step);
    MathSequencePath::new(steps)
}

fn position_at(
    basis: &MathTreePosition,
    path: MathSequencePath,
    offset: usize,
) -> MathTreePosition {
    MathTreePosition {
        host: basis.host.clone(),
        path,
        offset,
        affinity: basis.affinity,
    }
}

pub(super) fn text_path_for_content(
    position: &MathTreePosition,
    target_index: usize,
    align_cell: Option<(usize, usize)>,
) -> MathTextFlowPath {
    let sequence_steps = position.path.steps();
    let mut steps = Vec::with_capacity(sequence_steps.len() + 1);
    let mut content_already_selected = false;
    for (index, step) in sequence_steps.iter().enumerate() {
        match *step {
            MathSequenceStep::Operand {
                content_index,
                operand_index,
            } => {
                if !content_already_selected {
                    steps.push(MathTextFlowPathStep::Content(content_index));
                }
                steps.push(MathTextFlowPathStep::Operand(operand_index));
                content_already_selected = false;
            }
            MathSequenceStep::EnvironmentChildren { content_index } => {
                if !content_already_selected {
                    steps.push(MathTextFlowPathStep::Content(content_index));
                }
                let child_index = sequence_steps
                    .get(index + 1)
                    .map_or(target_index, |next| sequence_step_content_index(*next));
                steps.push(MathTextFlowPathStep::EnvironmentChild(child_index));
                content_already_selected = true;
            }
        }
    }
    if !content_already_selected {
        steps.push(MathTextFlowPathStep::Content(target_index));
    }
    MathTextFlowPath::new(steps, align_cell)
}

fn sequence_step_content_index(step: MathSequenceStep) -> usize {
    match step {
        MathSequenceStep::Operand { content_index, .. }
        | MathSequenceStep::EnvironmentChildren { content_index } => content_index,
    }
}

/// 结构路径或流边界不匹配当前 MathTree。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathTreePositionError {
    HostMismatch,
    InvalidTextFlowPath { depth: usize },
    InvalidPath { depth: usize },
    OffsetOutOfBounds { offset: usize, content_len: usize },
}

#[cfg(test)]
mod tests;
