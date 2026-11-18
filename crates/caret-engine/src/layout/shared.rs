//! 生产与快照查询共用的路径解析与轻量几何工具。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

pub(super) fn find_inline_math_by_id<'a>(
    nodes: &'a [ParagraphContent],
    id: &NodeId,
) -> Option<&'a caret_doc::InlineMath> {
    nodes.iter().find_map(|node| match node {
        FlowContent::Math(math) if &math.id == id => Some(math),
        FlowContent::Environment(environment) => find_inline_math_by_id(&environment.children, id),
        FlowContent::Text(_)
        | FlowContent::Math(_)
        | FlowContent::SoftBreak(_)
        | FlowContent::Opaque(_) => None,
    })
}

pub(super) fn math_flow_at_layout_path<'a>(
    root: &'a MathFlow,
    steps: &[MathPathStep],
) -> Option<&'a MathFlow> {
    match math_layout_target_at_path(root, steps)? {
        MathLayoutTarget::Flow(flow) => Some(flow),
        MathLayoutTarget::Content(_) => None,
    }
}

pub(super) fn math_text_flow_at_layout_path<'a>(
    root: &'a MathFlow,
    steps: &[MathPathStep],
) -> Option<&'a caret_doc::TextFlow> {
    match math_layout_target_at_path(root, steps)? {
        MathLayoutTarget::Content(MathContent::Text(text)) => Some(text),
        MathLayoutTarget::Flow(_) | MathLayoutTarget::Content(_) => None,
    }
}

pub(super) fn math_sequence_edge_child_indices(
    offset: usize,
    length: usize,
    affinity: MathPositionAffinity,
) -> [Option<usize>; 2] {
    match affinity {
        MathPositionAffinity::Upstream => {
            [offset.checked_sub(1), (offset < length).then_some(offset)]
        }
        MathPositionAffinity::Downstream => {
            [(offset < length).then_some(offset), offset.checked_sub(1)]
        }
    }
}

pub(super) fn math_environment_at_layout_path<'a>(
    root: &'a MathFlow,
    steps: &[MathPathStep],
) -> Option<&'a caret_doc::InlineEnvironment<MathContent>> {
    match math_layout_target_at_path(root, steps)? {
        MathLayoutTarget::Content(MathContent::Environment(environment)) => Some(environment),
        MathLayoutTarget::Flow(_) | MathLayoutTarget::Content(_) => None,
    }
}

pub(super) enum MathLayoutTarget<'a> {
    Flow(&'a MathFlow),
    Content(&'a MathContent),
}

pub(super) fn math_layout_target_at_path<'a>(
    root: &'a MathFlow,
    steps: &[MathPathStep],
) -> Option<MathLayoutTarget<'a>> {
    let mut target = MathLayoutTarget::Flow(root);
    for step in steps {
        target = match (target, step) {
            (MathLayoutTarget::Flow(flow), MathPathStep::Content(index)) => {
                MathLayoutTarget::Content(flow.content.get(*index)?)
            }
            (MathLayoutTarget::Content(MathContent::Math(node)), MathPathStep::Operand(index)) => {
                MathLayoutTarget::Flow(node.children.get(*index)?)
            }
            (
                MathLayoutTarget::Content(MathContent::Environment(environment)),
                MathPathStep::EnvironmentChild(index),
            ) => MathLayoutTarget::Content(environment.children.get(*index)?),
            _ => return None,
        };
    }
    Some(target)
}

/// 序列路径末尾的显式补充步类型。
#[derive(Clone, Copy)]
pub(super) enum MathSequenceEnd {
    /// 路径止于当前 Content，要求所有步骤恰好成对消费。
    Content,
    /// 节点自身是环境子序列，在末尾追加环境子节点步。
    EnvironmentChildren,
}

/// 把布局路径折算为公式结构序列；`end` 显式给出末尾补充步。
pub(super) fn math_sequence_path(
    steps: &[MathPathStep],
    end: MathSequenceEnd,
) -> Option<MathSequencePath> {
    let has_environment_end = matches!(end, MathSequenceEnd::EnvironmentChildren);
    let mut sequence = Vec::with_capacity(steps.len() / 2 + usize::from(has_environment_end));
    let mut content_index = None;
    for step in steps {
        match *step {
            MathPathStep::Content(index) => {
                if content_index.replace(index).is_some() {
                    return None;
                }
            }
            MathPathStep::Operand(operand_index) => {
                sequence.push(MathSequenceStep::Operand {
                    content_index: content_index.take()?,
                    operand_index,
                });
            }
            MathPathStep::EnvironmentChild(index) => {
                sequence.push(MathSequenceStep::EnvironmentChildren {
                    content_index: content_index.take()?,
                });
                content_index = Some(index);
            }
        }
    }
    if has_environment_end {
        sequence.push(MathSequenceStep::EnvironmentChildren {
            content_index: content_index.take()?,
        });
    }
    content_index
        .is_none()
        .then(|| MathSequencePath::new(sequence))
}

pub(super) fn math_row_id_for_host(host: &MathPositionHost) -> Option<&NodeId> {
    match host {
        MathPositionHost::AlignCell { row_id, .. } => Some(row_id),
        MathPositionHost::InlineMath { .. } | MathPositionHost::Equation { .. } => None,
    }
}

pub(super) fn font_size_centipoints(font_size_pt: f32) -> Option<u16> {
    DocumentUnits::points_to_font_size_centipoints(font_size_pt)
}

pub(super) fn visible_font_size_centipoints(font_size_pt: f32) -> Option<u16> {
    if !font_size_pt.is_finite() || font_size_pt <= 0.0 {
        return None;
    }
    let centipoints = (f64::from(font_size_pt) * 100.0)
        .round()
        .clamp(1.0, f64::from(u16::MAX));
    #[expect(
        clippy::cast_possible_truncation,
        reason = "显示投影先将有限字号夹取到 u16 百分之一 pt 范围"
    )]
    Some(centipoints as u16)
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum MathBlockSource {
    Equation(caret_doc::Equation),
    Align(caret_doc::Align),
}

#[derive(Debug, Clone)]
pub(crate) struct MathTextScopeHost {
    pub(crate) host: MathPositionHost,
    pub(crate) align_cell: Option<(usize, usize)>,
}

#[derive(Debug, Clone)]
pub(crate) struct MathTextLayoutHost {
    pub(crate) target: MathEditTarget,
    pub(crate) scopes: HashMap<NodeId, MathTextScopeHost>,
}

impl MathTextLayoutHost {
    pub(super) fn inline(paragraph_id: NodeId, math_id: NodeId, scope_id: NodeId) -> Self {
        Self {
            target: MathEditTarget::Inline {
                paragraph_id,
                math_id: math_id.clone(),
            },
            scopes: HashMap::from([(
                scope_id,
                MathTextScopeHost {
                    host: MathPositionHost::InlineMath { owner_id: math_id },
                    align_cell: None,
                },
            )]),
        }
    }

    pub(super) fn equation(block_id: NodeId, scope_id: NodeId) -> Self {
        Self {
            target: MathEditTarget::Block {
                block_id: block_id.clone(),
                align: false,
            },
            scopes: HashMap::from([(
                scope_id,
                MathTextScopeHost {
                    host: MathPositionHost::Equation { owner_id: block_id },
                    align_cell: None,
                },
            )]),
        }
    }

    pub(super) fn align(block_id: NodeId, align: &caret_doc::Align) -> Self {
        let mut scopes = HashMap::new();
        for (row_index, row) in align.rows.iter().enumerate() {
            for (cell_index, cell) in row.cells.iter().enumerate() {
                scopes.insert(
                    cell.root.id.clone(),
                    MathTextScopeHost {
                        host: MathPositionHost::AlignCell {
                            row_id: row.id.clone(),
                            cell_id: cell.root.id.clone(),
                        },
                        align_cell: Some((row_index, cell_index)),
                    },
                );
            }
        }
        Self {
            target: MathEditTarget::Block {
                block_id,
                align: true,
            },
            scopes,
        }
    }

    pub(super) fn for_scope(&self, scope_id: &NodeId) -> Option<MathTextScopeHost> {
        self.scopes.get(scope_id).cloned()
    }
}
