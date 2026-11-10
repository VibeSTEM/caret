//! 按文档树物理结构解析节点父链、同级位置与全树顺序。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{Block, Document, FlowContent, NodeId, ParagraphContent};
use std::cmp::Ordering;

/// 文档物理树中可由稳定 ID 定位的节点种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentNodeKind {
    Paragraph,
    EnvironmentTitle,
    Environment,
    Equation,
    Align,
    InlineMath,
    InlineEnvironment,
}

/// 仅供同一文档顺序查询内部比较的路径键。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentOrderKey(Vec<usize>);

/// 稳定 ID 节点的物理父链、同级 slot 与文档顺序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentNodeLocation {
    pub id: NodeId,
    pub kind: DocumentNodeKind,
    /// 所属段落宿主；段落节点及其标题中的 Inline 子节点使用自身段落 ID。
    pub paragraph_id: Option<NodeId>,
    /// 所属 Paragraph 是环境标题时为真。
    pub is_title: bool,
    /// 从根到直接父环境的 ID；标题包含其所属环境。
    pub parent_environment_ids: Vec<NodeId>,
    /// 从外到内的行内格式环境 ID；行内数学自身不列入父链。
    pub parent_inline_environment_ids: Vec<NodeId>,
    /// 所属块序列中的真实下标；环境标题没有块序列 slot。
    pub sibling_index: Option<usize>,
    order: DocumentOrderKey,
}

/// 在完整文档树中查找节点；ExplicitBreak 与 Opaque 保留真实 slot，但不产生身份。
pub fn find_document_node(document: &Document, id: &NodeId) -> Option<DocumentNodeLocation> {
    find_block_node(&document.body, id)
}

/// 在块序列中查找节点；ExplicitBreak 与 Opaque 保留真实 slot，但不产生身份。
pub fn find_block_node(blocks: &[Block], id: &NodeId) -> Option<DocumentNodeLocation> {
    find_in_blocks(blocks, id, &mut Vec::new(), &mut Vec::new())
}

/// 比较同一文档快照中的两个稳定节点 ID；缺失任一节点时不提供次序。
pub fn compare_document_nodes(
    document: &Document,
    left_id: &NodeId,
    right_id: &NodeId,
) -> Option<Ordering> {
    let left = find_document_node(document, left_id)?;
    let right = find_document_node(document, right_id)?;
    Some(left.order.0.cmp(&right.order.0))
}

fn find_in_blocks(
    blocks: &[Block],
    id: &NodeId,
    parents: &mut Vec<NodeId>,
    prefix: &mut Vec<usize>,
) -> Option<DocumentNodeLocation> {
    for (slot, block) in blocks.iter().enumerate() {
        prefix.push(slot * 2 + 1);
        match block {
            Block::Paragraph(paragraph) if &paragraph.id == id => {
                let location = location(
                    paragraph.id.clone(),
                    DocumentNodeKind::Paragraph,
                    Some(paragraph.id.clone()),
                    false,
                    parents,
                    Some(slot),
                    prefix,
                );
                prefix.pop();
                return Some(location);
            }
            Block::Paragraph(paragraph) => {
                prefix.push(1);
                let found = find_inline_nodes(
                    &paragraph.inlines,
                    id,
                    parents,
                    &mut Vec::new(),
                    prefix,
                    &paragraph.id,
                    false,
                );
                prefix.pop();
                if found.is_some() {
                    prefix.pop();
                    return found;
                }
            }
            Block::Equation(equation) if &equation.id == id => {
                let location = location(
                    equation.id.clone(),
                    DocumentNodeKind::Equation,
                    None,
                    false,
                    parents,
                    Some(slot),
                    prefix,
                );
                prefix.pop();
                return Some(location);
            }
            Block::Align(align) if &align.id == id => {
                let location = location(
                    align.id.clone(),
                    DocumentNodeKind::Align,
                    None,
                    false,
                    parents,
                    Some(slot),
                    prefix,
                );
                prefix.pop();
                return Some(location);
            }
            Block::Environment(environment) => {
                if &environment.id == id {
                    let location = location(
                        environment.id.clone(),
                        DocumentNodeKind::Environment,
                        None,
                        false,
                        parents,
                        Some(slot),
                        prefix,
                    );
                    prefix.pop();
                    return Some(location);
                }
                parents.push(environment.id.clone());
                if let Some(title) = &environment.title {
                    if &title.id == id {
                        prefix.push(0);
                        let location = location(
                            title.id.clone(),
                            DocumentNodeKind::EnvironmentTitle,
                            Some(title.id.clone()),
                            true,
                            parents,
                            None,
                            prefix,
                        );
                        prefix.pop();
                        parents.pop();
                        prefix.pop();
                        return Some(location);
                    }
                    prefix.push(0);
                    prefix.push(1);
                    let found = find_inline_nodes(
                        &title.inlines,
                        id,
                        parents,
                        &mut Vec::new(),
                        prefix,
                        &title.id,
                        true,
                    );
                    prefix.pop();
                    prefix.pop();
                    if found.is_some() {
                        parents.pop();
                        prefix.pop();
                        return found;
                    }
                }
                prefix.push(1);
                let found = find_in_blocks(&environment.children, id, parents, prefix);
                prefix.pop();
                parents.pop();
                if found.is_some() {
                    prefix.pop();
                    return found;
                }
                prefix.pop();
                continue;
            }
            Block::Equation(_) | Block::Align(_) | Block::ExplicitBreak(_) | Block::Opaque(_) => {}
        }
        prefix.pop();
    }
    None
}

fn location(
    id: NodeId,
    kind: DocumentNodeKind,
    paragraph_id: Option<NodeId>,
    is_title: bool,
    parents: &[NodeId],
    sibling_index: Option<usize>,
    prefix: &[usize],
) -> DocumentNodeLocation {
    DocumentNodeLocation {
        id,
        kind,
        paragraph_id,
        is_title,
        parent_environment_ids: parents.to_vec(),
        parent_inline_environment_ids: Vec::new(),
        sibling_index,
        order: DocumentOrderKey(prefix.to_vec()),
    }
}

fn find_inline_nodes(
    nodes: &[ParagraphContent],
    id: &NodeId,
    block_parents: &[NodeId],
    inline_parents: &mut Vec<NodeId>,
    prefix: &mut Vec<usize>,
    paragraph_id: &NodeId,
    is_title: bool,
) -> Option<DocumentNodeLocation> {
    for (slot, node) in nodes.iter().enumerate() {
        prefix.push(slot * 2 + 1);
        match node {
            FlowContent::Math(math) if &math.id == id => {
                let mut location = location(
                    math.id.clone(),
                    DocumentNodeKind::InlineMath,
                    Some(paragraph_id.clone()),
                    is_title,
                    block_parents,
                    Some(slot),
                    prefix,
                );
                location.parent_inline_environment_ids = inline_parents.clone();
                prefix.pop();
                return Some(location);
            }
            FlowContent::Environment(environment) => {
                if &environment.id == id {
                    let mut location = location(
                        environment.id.clone(),
                        DocumentNodeKind::InlineEnvironment,
                        Some(paragraph_id.clone()),
                        is_title,
                        block_parents,
                        Some(slot),
                        prefix,
                    );
                    location.parent_inline_environment_ids = inline_parents.clone();
                    prefix.pop();
                    return Some(location);
                }
                inline_parents.push(environment.id.clone());
                prefix.push(1);
                let found = find_inline_nodes(
                    &environment.children,
                    id,
                    block_parents,
                    inline_parents,
                    prefix,
                    paragraph_id,
                    is_title,
                );
                prefix.pop();
                inline_parents.pop();
                if found.is_some() {
                    prefix.pop();
                    return found;
                }
            }
            FlowContent::Text(_)
            | FlowContent::Math(_)
            | FlowContent::SoftBreak(_)
            | FlowContent::Opaque(_) => {}
        }
        prefix.pop();
    }
    None
}
