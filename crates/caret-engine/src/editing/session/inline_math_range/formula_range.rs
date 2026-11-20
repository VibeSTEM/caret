//! 将一个公式内的两个真实端点分解为互不重叠的结构项和文本字素范围。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use caret_doc::{MathContent, MathFlow, MathTree};

/// 树边界或位于该树内容项内部的字素。文本切口始终保留原始路径。
#[derive(Clone)]
pub(super) struct LocalCut {
    pub tree: MathTreePosition,
    pub text: Option<MathTextPosition>,
}

impl LocalCut {
    pub(super) fn boundary(tree: MathTreePosition) -> Self {
        Self { tree, text: None }
    }

    pub(super) fn text(
        host: MathPositionHost,
        position: &MathTextPosition,
    ) -> Result<Self, EditError> {
        if position.path.align_cell().is_some() {
            return Err(EditError::InvalidPosition);
        }
        let (path, offset) =
            MathTreePosition::sequence_boundary_from_text_path(position.path.steps())
                .map_err(|_| EditError::InvalidPosition)?;
        Ok(Self {
            tree: MathTreePosition {
                host,
                path,
                offset,
                affinity: position.affinity,
            },
            text: Some(position.clone()),
        })
    }

    fn key(&self, tree: &MathTree) -> Result<Vec<usize>, EditError> {
        self.tree
            .validate_reachable(tree)
            .map_err(|_| EditError::InvalidPosition)?;
        if let Some(text) = &self.text {
            text.compare_in_same_flow(text, tree)
                .ok_or(EditError::InvalidPosition)?;
            text.position_order_key(tree)
                .ok_or(EditError::InvalidPosition)
        } else {
            Ok(math_edit::math_tree_order_key(&self.tree))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Items {
        start: MathTreePosition,
        end: MathTreePosition,
    },
    Text {
        start: MathTextPosition,
        end: MathTextPosition,
    },
}

/// 从文档范围派生的一个宿主内切片。路径以编辑前的树为准，删除须逆序执行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::editing::session) struct FormulaRange {
    pub(super) target: MathEditTarget,
    pub(super) before_body: bool,
    pieces: Vec<Piece>,
}

impl FormulaRange {
    pub(in crate::editing::session) fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    pub(super) fn between(
        target: MathEditTarget,
        tree: &MathTree,
        a: &LocalCut,
        b: &LocalCut,
        before_body: bool,
    ) -> Result<Self, EditError> {
        let (lo, hi) = (a.key(tree)?, b.key(tree)?);
        let mut out = Self {
            target,
            before_body,
            pieces: Vec::new(),
        };
        if lo == hi {
            return Ok(out);
        }
        let (lo, hi, start, end) = if lo < hi {
            (lo, hi, a, b)
        } else {
            (hi, lo, b, a)
        };
        let bounds = Bounds { lo, hi, start, end };
        let root = MathTreePosition {
            path: MathSequencePath::root(),
            offset: 0,
            ..a.tree.clone()
        };
        bounds.visit(&root, &tree.root.content, &mut out.pieces)?;
        Ok(out)
    }

    pub(super) fn copy_into(&self, tree: &MathTree, output: &mut String) -> Result<(), EditError> {
        let mut selected = MathFlow {
            id: tree.root.id.clone(),
            content: Vec::new(),
            extra: Default::default(),
        };
        for piece in &self.pieces {
            match piece {
                Piece::Items { start, end } => {
                    let content = start
                        .resolve(tree)
                        .map_err(|_| EditError::InvalidPosition)?;
                    let items = content
                        .get(start.offset..end.offset)
                        .ok_or(EditError::InvalidPosition)?;
                    selected.content.extend_from_slice(items);
                }
                Piece::Text { start, end } => {
                    let flow = start
                        .path
                        .resolve_structurally(tree)
                        .ok_or(EditError::InvalidPosition)?;
                    let mut fragment = caret_doc::TextFlow::default();
                    let mut offset = 0;
                    for run in &flow.runs {
                        let lo = start.offset.saturating_sub(offset).min(run.text.len());
                        let hi = end.offset.saturating_sub(offset).min(run.text.len());
                        if lo < hi {
                            let text = run.text.get(lo..hi).ok_or(EditError::InvalidPosition)?;
                            fragment.runs.push(caret_doc::Run {
                                text: text.to_owned(),
                                ..run.clone()
                            });
                        }
                        offset += run.text.len();
                    }
                    selected.content.push(MathContent::Text(fragment));
                }
            }
        }
        copy_math_items(&selected.id, &selected.content, output);
        Ok(())
    }

    pub(in crate::editing::session) fn delete_from(
        &self,
        tree: &mut MathTree,
    ) -> Result<(), EditError> {
        for piece in self.pieces.iter().rev() {
            match piece {
                Piece::Items { start, end } => {
                    let items = math_edit::resolve_sequence_mut(tree, &start.path)
                        .ok_or(EditError::InvalidPosition)?;
                    if start.offset > end.offset || end.offset > items.len() {
                        return Err(EditError::InvalidPosition);
                    }
                    if items[start.offset..end.offset]
                        .iter()
                        .any(math_edit::math_content_has_unknown_metadata)
                    {
                        return Err(EditError::UnsupportedOpaqueContent);
                    }
                    items.drain(start.offset..end.offset);
                }
                Piece::Text { start, end } => {
                    let flow = start
                        .path
                        .resolve_structurally_mut(tree)
                        .ok_or(EditError::InvalidPosition)?;
                    crate::editing::flow_edit::replace_text_flow_range(
                        flow,
                        start.offset..end.offset,
                        Affinity::Downstream,
                        "",
                    )?;
                }
            }
        }
        Ok(())
    }
}

/// 保留纯文本岛的原文。结构片段与后续文本共享控制词边界，末端的颜色声明保持局部作用域。
fn copy_math_items(root_id: &caret_doc::NodeId, items: &[MathContent], output: &mut String) {
    let mut after_command = false;
    let mut chunks = items
        .chunk_by(|a, b| matches!(a, MathContent::Text(_)) == matches!(b, MathContent::Text(_)))
        .peekable();
    while let Some(chunk) = chunks.next() {
        if matches!(chunk.first(), Some(MathContent::Text(_))) {
            for item in chunk {
                let MathContent::Text(text) = item else {
                    continue;
                };
                for run in &text.runs {
                    if run.text.is_empty() {
                        continue;
                    }
                    if after_command && run.text.starts_with(|c: char| c.is_ascii_alphabetic()) {
                        output.push(' ');
                    }
                    output.push_str(&run.text);
                    after_command = false;
                }
            }
            continue;
        }
        let mut content = chunk.to_vec();
        if chunks.peek().is_some() {
            scope_trailing_color(&mut content);
        }
        let flow = MathFlow {
            id: root_id.clone(),
            content,
            extra: Default::default(),
        };
        let source = caret_math::serialize_node(&flow);
        if source.is_empty() {
            continue;
        }
        if after_command && source.starts_with(|c: char| c.is_ascii_alphabetic()) {
            output.push(' ');
        }
        let prefix = source.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        after_command = prefix.len() != source.len() && prefix.ends_with('\\');
        output.push_str(&source);
    }
}

fn scope_trailing_color(items: &mut [MathContent]) {
    use caret_doc::MathNodeKind;
    if let Some(MathContent::Math(node)) = items.last_mut() {
        match node.kind {
            MathNodeKind::Color => node.kind = MathNodeKind::TextColor,
            MathNodeKind::Row | MathNodeKind::Style => {
                if let Some(child) = node.children.last_mut() {
                    scope_trailing_color(&mut child.content);
                }
            }
            _ => {}
        }
    }
}

struct Bounds<'a> {
    lo: Vec<usize>,
    hi: Vec<usize>,
    start: &'a LocalCut,
    end: &'a LocalCut,
}

impl Bounds<'_> {
    /// 整项作为连续切片发布。只有包含某个端点的项才需要继续进入其子序列。
    fn visit(
        &self,
        basis: &MathTreePosition,
        items: &[MathContent],
        out: &mut Vec<Piece>,
    ) -> Result<(), EditError> {
        let boundary = |offset| MathTreePosition {
            offset,
            ..basis.clone()
        };
        // 同一序列的偶数键是边界，奇数键落在该项内部。只有首、末项可能需要继续递归。
        let mut prefix = math_edit::math_tree_order_key(basis);
        prefix.pop();
        let local_index = |key: &[usize], round_up: bool| {
            if let Some(rest) = key.strip_prefix(prefix.as_slice()) {
                let slot = rest.first().copied().unwrap_or(0);
                if round_up { slot.div_ceil(2) } else { slot / 2 }
            } else if key < prefix.as_slice() {
                0
            } else {
                items.len()
            }
        };
        let first = local_index(&self.lo, false);
        let end = local_index(&self.hi, true);
        if first >= end {
            return Ok(());
        }
        let partial = |index: usize| {
            let contains_text_end = |cut: &LocalCut| {
                cut.text.is_some() && cut.tree.path == basis.path && cut.tree.offset == index
            };
            contains_text_end(self.start)
                || contains_text_end(self.end)
                || math_edit::math_tree_order_key(&boundary(index)) < self.lo
                || math_edit::math_tree_order_key(&boundary(index + 1)) > self.hi
        };
        let mut whole_start = first;
        if partial(first) {
            self.visit_item(basis, first, &items[first], out)?;
            whole_start += 1;
        }
        let last_partial = whole_start < end && partial(end - 1);
        let whole_end = end - usize::from(last_partial);
        if whole_start < whole_end {
            out.push(Piece::Items {
                start: boundary(whole_start),
                end: boundary(whole_end),
            });
        }
        if last_partial {
            self.visit_item(basis, end - 1, &items[end - 1], out)?;
        }
        Ok(())
    }

    fn visit_item(
        &self,
        basis: &MathTreePosition,
        index: usize,
        item: &MathContent,
        out: &mut Vec<Piece>,
    ) -> Result<(), EditError> {
        match item {
            MathContent::Text(flow) => {
                let endpoint = |cut: &LocalCut| {
                    (cut.tree.path == basis.path && cut.tree.offset == index)
                        .then(|| cut.text.clone())
                        .flatten()
                };
                // 源端点是同一个 TextFlow 中的字素，另一侧在外面时补齐到流边界。
                let a = endpoint(self.start);
                let b = endpoint(self.end);
                let reference = a
                    .as_ref()
                    .or(b.as_ref())
                    .ok_or(EditError::InvalidPosition)?;
                let first = a.clone().unwrap_or_else(|| {
                    MathTextPosition::new(
                        reference.path.clone(),
                        0,
                        MathPositionAffinity::Downstream,
                    )
                });
                let last = b.clone().unwrap_or_else(|| {
                    MathTextPosition::new(
                        reference.path.clone(),
                        crate::editing::flow_edit::text_flow_len(&flow.runs),
                        MathPositionAffinity::Upstream,
                    )
                });
                if first.offset < last.offset {
                    out.push(Piece::Text {
                        start: first,
                        end: last,
                    });
                }
            }
            MathContent::Math(node) => {
                for (operand_index, child) in node.children.iter().enumerate() {
                    let step = MathSequenceStep::Operand {
                        content_index: index,
                        operand_index,
                    };
                    self.visit_child(basis, step, &child.content, out)?;
                }
            }
            MathContent::Environment(environment) => {
                self.visit_child(
                    basis,
                    MathSequenceStep::EnvironmentChildren {
                        content_index: index,
                    },
                    &environment.children,
                    out,
                )?;
            }
            _ => return Err(EditError::InvalidPosition),
        }
        Ok(())
    }

    fn visit_child(
        &self,
        basis: &MathTreePosition,
        step: MathSequenceStep,
        content: &[MathContent],
        out: &mut Vec<Piece>,
    ) -> Result<(), EditError> {
        let mut steps = basis.path.steps().to_vec();
        steps.push(step);
        let child = MathTreePosition {
            path: MathSequencePath::new(steps),
            offset: 0,
            ..basis.clone()
        };
        self.visit(&child, content, out)
    }
}

impl crate::DocumentLayoutSnapshot {
    /// 将跨宿主选区中的数学项和文本字素投影到当前布局，缺失任一片段时拒绝整份几何。
    pub fn inline_math_selection_geometry(
        &self,
        parts: &InlineMathRangeParts,
    ) -> Option<Vec<crate::PageSelectionRect>> {
        let mut rectangles = Vec::new();
        for range in &parts.formulae {
            for piece in &range.pieces {
                rectangles.extend(match piece {
                    Piece::Items { start, end } => {
                        self.math_tree_selection_geometry(&range.target, start, end)?
                    }
                    Piece::Text { start, end } => {
                        self.math_text_selection_geometry(&range.target, start, end)?
                    }
                });
            }
        }
        Some(rectangles)
    }
}
