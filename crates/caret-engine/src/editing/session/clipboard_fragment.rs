//! 复制 Caret 结构片段、粘贴片段与外部纯文本数学识别。
//!
//! 本模块是本任务 Rust 侧剪贴板权威入口：复制是只读投影，粘贴在克隆文档上完整
//! 规划、校验后以单个历史补丁提交。片段裁切、节点身份重映射、合并首尾段落与
//! 失败原子性都在这里完成，Qt 只负责系统剪贴板 MIME 读写。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::{
    ParagraphLocation, build_paragraph_index, paragraph_at_location, paragraph_at_location_mut,
    paragraph_parent_path,
};
use super::super::text_map::{TextMap, inline_length, split_inline_nodes};
use super::super::{
    Affinity, Direction, DocumentRangeEndpoint, EditError, Effect, HostCaretKind,
    HostCaretPosition, Pos, PositionMap, Selection, StoryId,
};
use super::Session;
use super::history::{DocumentEditSnapshot, DocumentPatch, ParagraphState, SessionHistoryState};
use caret_doc::{
    Align, Block, ClipboardFragment, ClipboardFragmentShape, Equation, InlineMath, MathTree,
    NodeId, Paragraph, ParagraphContent, ParagraphLineSpacing, TextFlow,
};
use caret_math::{MathFragment, MathInputKind, ParsedMath};

/// 一次成功复制得到的双格式结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardCopy {
    /// Caret 专用结构片段；所有节点保留源文档身份，粘贴时重映射。
    pub fragment: ClipboardFragment,
    /// 供其他应用读取的纯文本回退；数学使用规范 TeX 定界符，不输出 U+FFFC。
    pub plain_text: String,
}

/// 外部纯文本粘贴的执行计划；不需要数学转换时回退到普通文本粘贴。
pub(super) enum ExternalClipboardPlan {
    /// 整段按普通文本粘贴（`Command::PasteText` 语义）。
    PlainText,
    /// 用行内内容替换当前段落选区（混合文本或整段行内公式）。
    ReplaceInline(Vec<ParagraphContent>),
    /// 在当前段落位置插入整块显示公式。
    InsertDisplay(ParsedMath),
}

/// 一个复制单元：已按选区裁切的规范块。
struct CopyUnit {
    block: Block,
    clipped_start: bool,
    clipped_end: bool,
}

/// 复制遍历上下文；`start`/`end` 与段落序号描述同一文本范围。
struct CopyContext<'a> {
    start: Option<&'a Pos>,
    end: Option<&'a Pos>,
    start_ordinal: Option<usize>,
    end_ordinal: Option<usize>,
    environments: Vec<NodeId>,
    math_blocks: Vec<NodeId>,
}

impl CopyContext<'_> {
    fn has_text_bounds(&self) -> bool {
        self.start.is_some() && self.end.is_some()
    }
}

/// 粘贴目标的单侧边界；块序列在 `parent_environment_ids` 的 `index` 处切开。
#[derive(Debug, Clone)]
enum PasteBoundary {
    AtIndex {
        parent_environment_ids: Vec<NodeId>,
        index: usize,
    },
    InParagraph {
        parent_environment_ids: Vec<NodeId>,
        child_index: usize,
        paragraph_id: NodeId,
        offset: usize,
    },
}

impl PasteBoundary {
    fn parent_ids(&self) -> &[NodeId] {
        match self {
            Self::AtIndex {
                parent_environment_ids,
                ..
            }
            | Self::InParagraph {
                parent_environment_ids,
                ..
            } => parent_environment_ids,
        }
    }
}

/// 粘贴完成后折叠选区的落点提示；正文段落优先，块对象退到块外侧停靠。
#[derive(Debug, Clone)]
enum PasteCaretHint {
    Paragraph { paragraph_id: NodeId, offset: usize },
    Block { block_id: NodeId },
}

/// 在某个容器的块序列里解析后的单侧边界。
enum LocalBoundary {
    AtIndex(usize),
    InParagraph {
        child_index: usize,
        offset: usize,
        paragraph: Paragraph,
    },
    Deeper {
        child_index: usize,
        boundary: PasteBoundary,
    },
}

/// 粘贴规划结果：新正文与折叠选区提示。
struct BodyPastePlan {
    body: Vec<Block>,
    caret: PasteCaretHint,
}

impl Session {
    /// 复制当前选区为 Caret 结构片段与纯文本回退。
    ///
    /// 空选区返回 `Ok(None)`（复制无效但不改变剪贴板）；选中范围包含公式内部切口
    /// 时 v1 没有对应片段形状，返回 [`EditError::ClipboardHostMismatch`] 而不是输出
    /// 降级文本。任何 checked 数学序列化损失返回 [`EditError::ClipboardMathLoss`]。
    pub fn copy_clipboard_fragment(&self) -> Result<Option<ClipboardCopy>, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if self.math_has_non_empty_selection() {
            // v1 没有 MathTree/MathText 内部片段形状：公式内选区复制必须整体失败，
            // 不输出降级文本，也不覆盖系统剪贴板。
            return Err(EditError::ClipboardHostMismatch);
        }
        if let Some(parts) = self.inline_math_range_parts() {
            if parts.formula_ranges().any(|(_, range)| !range.is_empty()) {
                return Err(EditError::ClipboardHostMismatch);
            }
            let anchor = parts.body.anchor.clone();
            let focus = parts.body.focus.clone();
            return self.copy_text_range(&anchor, &focus);
        }
        if self.has_structure_selection() {
            return self.copy_structure_selection();
        }
        let anchor = self.selection.anchor.clone();
        let focus = self.selection.focus.clone();
        self.copy_text_range(&anchor, &focus)
    }

    /// 当前公式编辑目标是否存在非空局部选区。
    fn math_has_non_empty_selection(&self) -> bool {
        if self.math_edit.is_none() {
            return false;
        }
        if let Some(context) = self.math_text_input_context()
            && context.anchor_utf16 != context.focus_utf16
        {
            return true;
        }
        if let Some((_, anchor, focus)) = self.math_tree_selection_context()
            && anchor != focus
        {
            return true;
        }
        false
    }

    /// 把一个已解码的 Caret 片段粘贴到当前选区，产生恰好一次文档事务。
    pub fn paste_clipboard_fragment(
        &mut self,
        fragment: ClipboardFragment,
    ) -> Result<Effect, EditError> {
        self.run_with_inline_math_focus(
            super::super::Command::PasteClipboardFragment(Box::new(fragment)),
            false,
        )
    }

    /// 按剪贴板架构识别外部纯文本中的闭合数学片段并粘贴，产生恰好一次事务。
    pub fn paste_external_clipboard_text(&mut self, text: &str) -> Result<Effect, EditError> {
        self.run_with_inline_math_focus(
            super::super::Command::PasteExternalClipboardText(text.to_owned()),
            false,
        )
    }

    /// 命令入口：把片段粘贴转换为完整规划；失败发生在任何写入之前。
    pub(super) fn execute_paste_clipboard_fragment(
        &mut self,
        fragment: ClipboardFragment,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if self.math_edit.is_some() || self.inline_math_range_parts().is_some() {
            return Err(EditError::ClipboardHostMismatch);
        }
        match fragment.shape {
            ClipboardFragmentShape::Inline => self.paste_inline_fragment(fragment),
            ClipboardFragmentShape::Blocks => self.paste_block_fragment(fragment),
        }
    }

    /// 命令入口：解析外部纯文本数学并返回执行计划；不可转换时整段按普通文本。
    pub(super) fn plan_external_clipboard_text(
        &self,
        text: &str,
    ) -> Result<ExternalClipboardPlan, EditError> {
        if text.is_empty() {
            return Ok(ExternalClipboardPlan::PlainText);
        }
        match caret_math::scan_plain_text_math(text) {
            caret_math::PlainTextMathScan::Plain => Ok(ExternalClipboardPlan::PlainText),
            caret_math::PlainTextMathScan::WholeInline(fragment) => {
                let Some(tree) = fragment_tree(&fragment)? else {
                    return Ok(ExternalClipboardPlan::PlainText);
                };
                if !self.inline_math_target_available() {
                    return Ok(ExternalClipboardPlan::PlainText);
                }
                Ok(ExternalClipboardPlan::ReplaceInline(vec![
                    ParagraphContent::Math(InlineMath {
                        id: NodeId::generate(),
                        math_tree: tree,
                        extra: Default::default(),
                    }),
                ]))
            }
            caret_math::PlainTextMathScan::WholeDisplay(fragment) => {
                if !self.display_math_target_available() {
                    return Ok(ExternalClipboardPlan::PlainText);
                }
                Ok(ExternalClipboardPlan::InsertDisplay(fragment.into_math()))
            }
            caret_math::PlainTextMathScan::MixedInline(segments) => {
                if !self.inline_math_target_available() {
                    return Ok(ExternalClipboardPlan::PlainText);
                }
                // 行内插入路径不承载硬换行；混合文本含换行时整段交给普通粘贴拆段。
                if segments.iter().any(|segment| {
                    matches!(
                        segment,
                        caret_math::PlainTextMathSegment::Text(text)
                            if text.contains('\n') || text.contains('\r')
                    )
                }) {
                    return Ok(ExternalClipboardPlan::PlainText);
                }
                let mut nodes = Vec::new();
                for segment in segments {
                    match segment {
                        caret_math::PlainTextMathSegment::Text(text) => {
                            if !text.is_empty() {
                                nodes.push(ParagraphContent::Text(TextFlow {
                                    runs: vec![caret_doc::Run {
                                        text,
                                        language: None,
                                        props: Default::default(),
                                        extra: Default::default(),
                                    }],
                                    extra: Default::default(),
                                }));
                            }
                        }
                        caret_math::PlainTextMathSegment::Math(span) => {
                            let Some(tree) = fragment_tree(&span.fragment)? else {
                                return Ok(ExternalClipboardPlan::PlainText);
                            };
                            nodes.push(ParagraphContent::Math(InlineMath {
                                id: NodeId::generate(),
                                math_tree: tree,
                                extra: Default::default(),
                            }));
                        }
                    }
                }
                if nodes.is_empty() {
                    return Ok(ExternalClipboardPlan::PlainText);
                }
                Ok(ExternalClipboardPlan::ReplaceInline(nodes))
            }
        }
    }

    /// 命令入口：把整段行内/混合内容作为行内内容替换当前正文选区。
    pub(super) fn replace_selection_with_inline_content(
        &mut self,
        nodes: Vec<ParagraphContent>,
    ) -> Result<Effect, EditError> {
        self.replace_selection_with_inline_nodes(nodes)
    }

    /// 命令入口：在当前段落后插入整块显示公式。
    pub(super) fn insert_external_display_math(
        &mut self,
        math: ParsedMath,
    ) -> Result<Effect, EditError> {
        self.insert_display_math_value(math)
    }

    // ------------------------------------------------------------------
    // 复制
    // ------------------------------------------------------------------

    fn copy_text_range(
        &self,
        anchor: &Pos,
        focus: &Pos,
    ) -> Result<Option<ClipboardCopy>, EditError> {
        if anchor == focus {
            return Ok(None);
        }
        if anchor.story != focus.story {
            return Err(EditError::CrossParagraphSelection);
        }
        let (start, end) = self.ordered_text_range(anchor, focus)?;
        if start.paragraph_id == end.paragraph_id {
            return self.copy_inline_range(&start, &end);
        }
        let start_ordinal = self
            .structure_index
            .paragraph_ordinal(&start.paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        let end_ordinal = self
            .structure_index
            .paragraph_ordinal(&end.paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        let context = CopyContext {
            start: Some(&start),
            end: Some(&end),
            start_ordinal: Some(start_ordinal),
            end_ordinal: Some(end_ordinal),
            environments: Vec::new(),
            math_blocks: Vec::new(),
        };
        self.assemble_copy_units(&context)
    }

    fn copy_structure_selection(&self) -> Result<Option<ClipboardCopy>, EditError> {
        let (environments, math_blocks) = self.structure_range_block_nodes()?;
        if environments.is_empty() && math_blocks.is_empty() {
            return Err(EditError::ClipboardHostMismatch);
        }
        let text_bounds = self.usable_structure_text_bounds()?;
        let (start, end, start_ordinal, end_ordinal) = match &text_bounds {
            Some((start, end, start_ordinal, end_ordinal)) => (
                Some(start.clone()),
                Some(end.clone()),
                Some(*start_ordinal),
                Some(*end_ordinal),
            ),
            None => (None, None, None, None),
        };
        let context = CopyContext {
            start: start.as_ref(),
            end: end.as_ref(),
            start_ordinal,
            end_ordinal,
            environments,
            math_blocks,
        };
        self.assemble_copy_units(&context)
    }

    fn copy_inline_range(
        &self,
        start: &Pos,
        end: &Pos,
    ) -> Result<Option<ClipboardCopy>, EditError> {
        let paragraph = self.paragraph_for(start)?.clone();
        let map = TextMap::new(&paragraph)?;
        map.validate_pos(start)?;
        map.validate_pos(end)?;
        let start_offset = start.offset.min(end.offset);
        let end_offset = start.offset.max(end.offset);
        if start_offset == end_offset {
            return Ok(None);
        }
        let (through_end, _) = split_inline_nodes(paragraph.inlines.clone(), end_offset)?;
        let (_, content) = split_inline_nodes(through_end, start_offset)?;
        let mut clipped = paragraph;
        clipped.inlines = content;
        validate_copy_paragraph(&clipped)?;
        let plain_text = paragraph_clipboard_text(&clipped)?;
        Ok(Some(ClipboardCopy {
            fragment: ClipboardFragment {
                shape: ClipboardFragmentShape::Inline,
                start_clipped: start_offset > 0,
                end_clipped: end_offset < map.text.len(),
                blocks: vec![Block::Paragraph(clipped)],
            },
            plain_text,
        }))
    }

    fn assemble_copy_units(
        &self,
        context: &CopyContext<'_>,
    ) -> Result<Option<ClipboardCopy>, EditError> {
        let units = self.collect_copy_units(&self.document.body, &mut Vec::new(), context)?;
        if units.is_empty() {
            return Ok(None);
        }
        let start_clipped = units.first().is_some_and(|unit| unit.clipped_start);
        let end_clipped = units.last().is_some_and(|unit| unit.clipped_end);
        let same_paragraph = context
            .start_ordinal
            .zip(context.end_ordinal)
            .is_some_and(|(start, end)| start == end);
        if units.len() == 1
            && same_paragraph
            && let Block::Paragraph(paragraph) = &units[0].block
        {
            if paragraph.inlines.is_empty() {
                return Ok(None);
            }
            let plain_text = paragraph_clipboard_text(paragraph)?;
            return Ok(Some(ClipboardCopy {
                fragment: ClipboardFragment {
                    shape: ClipboardFragmentShape::Inline,
                    start_clipped,
                    end_clipped,
                    blocks: vec![Block::Paragraph(paragraph.clone())],
                },
                plain_text,
            }));
        }
        let mut blocks = Vec::with_capacity(units.len());
        for unit in units {
            blocks.push(unit.block);
        }
        let mut lines = Vec::with_capacity(blocks.len());
        for block in &blocks {
            lines.push(block_clipboard_text(block)?);
        }
        Ok(Some(ClipboardCopy {
            fragment: ClipboardFragment {
                shape: ClipboardFragmentShape::Blocks,
                start_clipped,
                end_clipped,
                blocks,
            },
            plain_text: lines.join("\n"),
        }))
    }

    fn collect_copy_units(
        &self,
        blocks: &[Block],
        path: &mut Vec<usize>,
        context: &CopyContext<'_>,
    ) -> Result<Vec<CopyUnit>, EditError> {
        let mut units = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            path.push(index);
            match block {
                Block::Table(_) => return Err(EditError::UnsupportedTableContent),
                Block::Paragraph(paragraph) => {
                    if let Some(unit) = self.copy_paragraph_unit(paragraph, context)? {
                        units.push(unit);
                    }
                }
                Block::Environment(environment) => {
                    if context.environments.contains(&environment.id) {
                        validate_copy_block(block)?;
                        units.push(CopyUnit {
                            block: block.clone(),
                            clipped_start: false,
                            clipped_end: false,
                        });
                    } else {
                        if let Some(title) = &environment.title
                            && let Some(unit) = self.copy_paragraph_unit(title, context)?
                        {
                            units.push(unit);
                        }
                        units.extend(self.collect_copy_units(
                            &environment.children,
                            path,
                            context,
                        )?);
                    }
                }
                Block::Equation(_) | Block::Align(_) => {
                    if self.block_selected_for_copy(block, path, context)? {
                        validate_copy_block(block)?;
                        units.push(CopyUnit {
                            block: block.clone(),
                            clipped_start: false,
                            clipped_end: false,
                        });
                    }
                }
                Block::ExplicitBreak(_) => {
                    if self.block_path_strictly_inside(path, context)? {
                        validate_copy_block(block)?;
                        units.push(CopyUnit {
                            block: block.clone(),
                            clipped_start: false,
                            clipped_end: false,
                        });
                    }
                }
                Block::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
            }
            path.pop();
        }
        Ok(units)
    }

    fn copy_paragraph_unit(
        &self,
        paragraph: &Paragraph,
        context: &CopyContext<'_>,
    ) -> Result<Option<CopyUnit>, EditError> {
        if !context.has_text_bounds() {
            return Ok(None);
        }
        let Some(start) = context.start else {
            return Ok(None);
        };
        let Some(end) = context.end else {
            return Ok(None);
        };
        let Some(start_ordinal) = context.start_ordinal else {
            return Ok(None);
        };
        let Some(end_ordinal) = context.end_ordinal else {
            return Ok(None);
        };
        let ordinal = self
            .structure_index
            .paragraph_ordinal(&paragraph.id)
            .ok_or(EditError::InvalidPosition)?;
        if ordinal < start_ordinal || ordinal > end_ordinal {
            return Ok(None);
        }
        let map = TextMap::new(paragraph)?;
        let text_len = map.text.len();
        let start_offset = if ordinal == start_ordinal {
            start.offset
        } else {
            0
        };
        let end_offset = if ordinal == end_ordinal {
            end.offset
        } else {
            text_len
        };
        if ordinal == start_ordinal && ordinal != end_ordinal && start_offset >= text_len {
            return Ok(None);
        }
        if ordinal == end_ordinal && ordinal != start_ordinal && end_offset == 0 {
            return Ok(None);
        }
        let end_offset = end_offset.min(text_len);
        let start_offset = start_offset.min(end_offset);
        if start_offset == 0 && end_offset == text_len {
            validate_copy_paragraph(paragraph)?;
            return Ok(Some(CopyUnit {
                block: Block::Paragraph(paragraph.clone()),
                clipped_start: false,
                clipped_end: false,
            }));
        }
        let (through_end, _) = split_inline_nodes(paragraph.inlines.clone(), end_offset)?;
        let (_, content) = split_inline_nodes(through_end, start_offset)?;
        let mut clipped = paragraph.clone();
        clipped.inlines = content;
        validate_copy_paragraph(&clipped)?;
        Ok(Some(CopyUnit {
            block: Block::Paragraph(clipped),
            clipped_start: start_offset > 0,
            clipped_end: end_offset < text_len,
        }))
    }

    fn block_selected_for_copy(
        &self,
        block: &Block,
        path: &[usize],
        context: &CopyContext<'_>,
    ) -> Result<bool, EditError> {
        let block_id = match block {
            Block::Equation(equation) => &equation.id,
            Block::Align(align) => &align.id,
            _ => return Ok(false),
        };
        if context.math_blocks.contains(block_id) {
            return Ok(true);
        }
        self.block_path_strictly_inside(path, context)
    }

    fn block_path_strictly_inside(
        &self,
        path: &[usize],
        context: &CopyContext<'_>,
    ) -> Result<bool, EditError> {
        let (Some(start), Some(end)) = (context.start, context.end) else {
            return Ok(false);
        };
        let start_location = self
            .paragraph_index
            .get(&start.paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        let end_location = self
            .paragraph_index
            .get(&end.paragraph_id)
            .ok_or(EditError::InvalidPosition)?;
        Ok(
            path > start_location.block_path.as_slice()
                && path < end_location.block_path.as_slice(),
        )
    }
}

/// 取整段行内公式的规范树；宿主类型不符时按普通文本回退。
fn fragment_tree(fragment: &MathFragment) -> Result<Option<MathTree>, EditError> {
    match fragment.math() {
        ParsedMath::Tree(tree) => Ok(Some(tree.clone())),
        ParsedMath::Align(_) => Ok(None),
    }
}

fn clipboard_math_loss(error: caret_math::MathFragmentError) -> EditError {
    EditError::ClipboardMathLoss(error.to_string())
}

fn checked_tex(math: &MathTree, kind: MathInputKind) -> Result<String, EditError> {
    let fragment = MathFragment::from_math(kind, ParsedMath::Tree(math.clone()))
        .map_err(clipboard_math_loss)?;
    let tex = fragment.to_tex().map_err(clipboard_math_loss)?;
    Ok(caret_math::wrap_tex(&tex, kind))
}

fn checked_align_tex(align: &Align) -> Result<String, EditError> {
    let fragment = MathFragment::from_math(MathInputKind::Align, ParsedMath::Align(align.clone()))
        .map_err(clipboard_math_loss)?;
    let tex = fragment.to_tex().map_err(clipboard_math_loss)?;
    Ok(caret_math::wrap_tex(&tex, MathInputKind::Align))
}

fn ensure_empty_extra(
    extra: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<(), EditError> {
    if extra.is_empty() {
        Ok(())
    } else {
        Err(EditError::UnsupportedOpaqueContent)
    }
}

fn validate_copy_paragraph(paragraph: &Paragraph) -> Result<(), EditError> {
    ensure_empty_extra(&paragraph.extra)?;
    if let Some(layout) = &paragraph.layout {
        ensure_empty_extra(&layout.extra)?;
        if let Some(line_spacing) = &layout.line_spacing {
            let extra = match line_spacing {
                ParagraphLineSpacing::Natural { extra }
                | ParagraphLineSpacing::Multiple { extra, .. }
                | ParagraphLineSpacing::AtLeast { extra, .. }
                | ParagraphLineSpacing::Exactly { extra, .. } => extra,
            };
            ensure_empty_extra(extra)?;
        }
    }
    validate_copy_inlines(&paragraph.inlines)
}

fn validate_copy_inlines(inlines: &[ParagraphContent]) -> Result<(), EditError> {
    for content in inlines {
        match content {
            ParagraphContent::Text(flow) => {
                ensure_empty_extra(&flow.extra)?;
                for run in &flow.runs {
                    ensure_empty_extra(&run.extra)?;
                }
            }
            ParagraphContent::Environment(environment) => {
                ensure_empty_extra(&environment.extra)?;
                validate_copy_inlines(&environment.children)?;
            }
            ParagraphContent::SoftBreak(soft_break) => {
                ensure_empty_extra(&soft_break.extra)?;
            }
            ParagraphContent::Math(math) => {
                if !math.extra.is_empty() {
                    return Err(EditError::ClipboardMathLoss(
                        "inline math has unknown extension fields".to_owned(),
                    ));
                }
            }
            ParagraphContent::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
        }
    }
    Ok(())
}

fn validate_copy_block(block: &Block) -> Result<(), EditError> {
    match block {
        Block::Paragraph(paragraph) => validate_copy_paragraph(paragraph),
        Block::Environment(environment) => {
            ensure_empty_extra(&environment.extra)?;
            if let Some(title) = &environment.title {
                validate_copy_paragraph(title)?;
            }
            for child in &environment.children {
                validate_copy_block(child)?;
            }
            Ok(())
        }
        Block::Equation(equation) => {
            if !equation.extra.is_empty() {
                return Err(EditError::ClipboardMathLoss(
                    "equation has unknown extension fields".to_owned(),
                ));
            }
            Ok(())
        }
        Block::Align(align) => {
            if !align.extra.is_empty() {
                return Err(EditError::ClipboardMathLoss(
                    "align has unknown extension fields".to_owned(),
                ));
            }
            Ok(())
        }
        Block::Table(_) => Err(EditError::UnsupportedTableContent),
        Block::ExplicitBreak(explicit_break) => ensure_empty_extra(&explicit_break.extra),
        Block::Opaque(_) => Err(EditError::UnsupportedOpaqueContent),
    }
}

fn paragraph_clipboard_text(paragraph: &Paragraph) -> Result<String, EditError> {
    let mut text = String::new();
    push_inline_clipboard_text(&paragraph.inlines, &mut text)?;
    Ok(text)
}

fn push_inline_clipboard_text(
    inlines: &[ParagraphContent],
    text: &mut String,
) -> Result<(), EditError> {
    for content in inlines {
        match content {
            ParagraphContent::Text(flow) => {
                for run in &flow.runs {
                    text.push_str(&run.text);
                }
            }
            ParagraphContent::Environment(environment) => {
                push_inline_clipboard_text(&environment.children, text)?;
            }
            ParagraphContent::SoftBreak(_) => text.push('\n'),
            ParagraphContent::Math(math) => {
                let tex = checked_tex(&math.math_tree, MathInputKind::Inline)?;
                text.push_str(&tex);
            }
            ParagraphContent::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
        }
    }
    Ok(())
}

fn block_clipboard_text(block: &Block) -> Result<String, EditError> {
    match block {
        Block::Paragraph(paragraph) => paragraph_clipboard_text(paragraph),
        Block::Equation(equation) => checked_tex(&equation.math_tree, MathInputKind::Equation),
        Block::Align(align) => checked_align_tex(align),
        Block::Environment(environment) => {
            ensure_empty_extra(&environment.extra)?;
            let mut parts = Vec::new();
            if let Some(title) = &environment.title {
                parts.push(paragraph_clipboard_text(title)?);
            }
            for child in &environment.children {
                parts.push(block_clipboard_text(child)?);
            }
            Ok(parts.join("\n"))
        }
        Block::Table(_) => Err(EditError::UnsupportedTableContent),
        Block::ExplicitBreak(_) => Ok(String::new()),
        Block::Opaque(_) => Err(EditError::UnsupportedOpaqueContent),
    }
}

// ------------------------------------------------------------------
// 行内粘贴
// ------------------------------------------------------------------

impl Session {
    fn inline_math_target_available(&self) -> bool {
        self.inline_paste_target().is_some()
    }

    fn display_math_target_available(&self) -> bool {
        if self.composition.is_some()
            || self.math_edit.is_some()
            || self.command_line.is_some()
            || self.has_structure_selection()
            || self.host_caret_position.is_some()
            || self.selection.anchor != self.selection.focus
        {
            return false;
        }
        let focus = &self.selection.focus;
        if focus.story != StoryId::Body {
            return false;
        }
        let Some(location) = self.paragraph_index.get(&focus.paragraph_id) else {
            return false;
        };
        if location.is_title || location.story != StoryId::Body {
            return false;
        }
        let Ok(paragraph) = self.paragraph_for(focus) else {
            return false;
        };
        let Ok(map) = TextMap::new(paragraph) else {
            return false;
        };
        map.validate_pos(focus).is_ok()
    }

    fn inline_paste_target(&self) -> Option<(Pos, Pos, ParagraphLocation)> {
        if self.composition.is_some()
            || self.math_edit.is_some()
            || self.command_line.is_some()
            || self.has_structure_selection()
            || self.host_caret_position.is_some()
        {
            return None;
        }
        let anchor = self.selection.anchor.clone();
        let focus = self.selection.focus.clone();
        if anchor.story != StoryId::Body || focus.story != StoryId::Body {
            return None;
        }
        if anchor.paragraph_id != focus.paragraph_id {
            return None;
        }
        let location = self.paragraph_index.get(&anchor.paragraph_id)?.clone();
        if location.is_title || location.story != StoryId::Body {
            return None;
        }
        let paragraph = self.paragraph_for(&anchor).ok()?;
        let map = TextMap::new(paragraph).ok()?;
        map.validate_pos(&anchor).ok()?;
        map.validate_pos(&focus).ok()?;
        Some((anchor, focus, location))
    }

    fn paste_inline_fragment(
        &mut self,
        mut fragment: ClipboardFragment,
    ) -> Result<Effect, EditError> {
        let [Block::Paragraph(paragraph)] = fragment.blocks.as_slice() else {
            return Err(EditError::ClipboardHostMismatch);
        };
        if paragraph.inlines.is_empty() {
            return Err(EditError::ClipboardHostMismatch);
        }
        validate_copy_paragraph(paragraph)?;
        // 入站片段身份在源文档中可能仍然存在；插入前统一重建全部节点身份，
        // 避免同文档复制/粘贴在整文档校验时产生重复 ID。
        caret_doc::regenerate_clipboard_ids(&mut fragment.blocks);
        let [Block::Paragraph(paragraph)] = fragment.blocks.as_slice() else {
            return Err(EditError::ClipboardHostMismatch);
        };
        let nodes = paragraph.inlines.clone();
        self.replace_selection_with_inline_nodes(nodes)
    }

    fn replace_selection_with_inline_nodes(
        &mut self,
        nodes: Vec<ParagraphContent>,
    ) -> Result<Effect, EditError> {
        if nodes.is_empty() {
            return Ok(Effect::default());
        }
        let Some((anchor, focus, location)) = self.inline_paste_target() else {
            return Err(EditError::ClipboardHostMismatch);
        };
        let paragraph = self.paragraph_for(&anchor)?.clone();
        validate_copy_paragraph(&paragraph)?;
        let start = anchor.offset.min(focus.offset);
        let end = anchor.offset.max(focus.offset);
        let inserted_len = inline_nodes_length(&nodes)?;
        let (through_end, right) = split_inline_nodes(paragraph.inlines.clone(), end)?;
        let (left, _) = split_inline_nodes(through_end, start)?;
        let mut inlines = Vec::with_capacity(left.len() + nodes.len() + right.len());
        inlines.extend(left);
        inlines.extend(nodes);
        inlines.extend(right);
        merge_text_flows(&mut inlines);
        let mut updated = paragraph.clone();
        updated.inlines = inlines;
        let mut candidate_document = self.document.clone();
        let slot = paragraph_at_location_mut(&mut candidate_document.body, &location)
            .ok_or(EditError::InvalidPosition)?;
        *slot = updated.clone();
        candidate_document
            .validate()
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;

        let before_state = self.capture_paste_before_state();
        let before_paragraph_state =
            ParagraphState::capture(&self.document, &self.paragraph_index, &paragraph.id)
                .ok_or(EditError::InvalidPosition)?;
        let candidate_index = build_paragraph_index(&candidate_document.body);
        let after = ParagraphState::capture(&candidate_document, &candidate_index, &updated.id)
            .ok_or(EditError::InvalidPosition)?;
        let result_map = TextMap::new(&updated)?;
        let caret_offset =
            result_map.snap_to_grapheme_boundary(start + inserted_len, Affinity::Downstream);
        let caret = Pos {
            story: StoryId::Body,
            paragraph_id: updated.id.clone(),
            offset: caret_offset,
            affinity: Affinity::Downstream,
        };
        result_map.validate_pos(&caret)?;

        self.document = candidate_document;
        self.text_map_cache.clear();
        self.paragraph_index = build_paragraph_index(&self.document.body);
        self.structure_index =
            std::sync::Arc::new(caret_structure::StructureIndex::build(&self.document));
        self.selection = Selection {
            anchor: caret.clone(),
            focus: caret,
        };
        self.selection_visual_affinity = crate::VisualAffinity::Downstream;
        self.selection_anchor_visual_affinity = crate::VisualAffinity::Downstream;
        self.clear_structure_selection_state();
        self.clear_host_caret_position();
        self.clear_math_edit();
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();

        let patch = DocumentPatch::Paragraphs {
            before: vec![before_paragraph_state],
            after: vec![after],
            merge_group: None,
            merge_epoch: self.history_merge_epoch,
        };
        self.commit_document_edit(patch, before_state, DocumentEditSnapshot::Body);
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            changed_paragraph_id: Some(updated.id),
            position_map: Some(PositionMap::TextEdit {
                paragraph_id: anchor.paragraph_id,
                start,
                end,
                inserted_len,
                snap_graphemes: true,
                result_boundaries: result_map.grapheme_boundaries.clone(),
            }),
            ..Effect::default()
        })
    }

    fn capture_paste_before_state(&self) -> SessionHistoryState {
        SessionHistoryState::new(
            self.selection.clone(),
            self.selection_visual_affinity,
            self.pending_inline_style.clone(),
            self.history_state_id,
        )
        .with_anchor_visual_affinity(self.selection_anchor_visual_affinity)
        .with_math_edit(None)
        .with_host_caret_position(self.host_caret_position.clone())
    }

    fn insert_display_math_value(&mut self, math: ParsedMath) -> Result<Effect, EditError> {
        if !self.display_math_target_available() {
            return Err(EditError::ClipboardHostMismatch);
        }
        let focus = self.selection.focus.clone();
        let location = self
            .paragraph_index
            .get(&focus.paragraph_id)
            .cloned()
            .ok_or(EditError::InvalidPosition)?;
        let paragraph = self.paragraph_for(&focus)?.clone();
        if !paragraph.extra.is_empty() {
            return Err(EditError::UnsupportedOpaqueContent);
        }
        let map = TextMap::new(&paragraph)?;
        map.validate_pos(&focus)?;
        let (parent_environment_ids, sibling_index) =
            paragraph_parent_path(&self.document.body, &location)
                .ok_or(EditError::InvalidPosition)?;
        let block = match math {
            ParsedMath::Tree(tree) => Block::Equation(Equation {
                id: NodeId::generate(),
                math_tree: tree,
                extra: Default::default(),
            }),
            ParsedMath::Align(align) => Block::Align(align),
        };
        let paragraph_empty = map.text.is_empty();
        let (after, next_position) = if paragraph_empty {
            (
                vec![block, Block::Paragraph(paragraph.clone())],
                focus.clone(),
            )
        } else {
            let (left_inlines, right_inlines) =
                split_inline_nodes(paragraph.inlines.clone(), focus.offset)?;
            let mut left = paragraph.clone();
            left.inlines = left_inlines;
            let mut right = paragraph;
            right.id = NodeId::generate();
            right.inlines = right_inlines;
            right.extra.clear();
            let position = Pos {
                story: StoryId::Body,
                paragraph_id: right.id.clone(),
                offset: 0,
                affinity: Affinity::Downstream,
            };
            (
                vec![Block::Paragraph(left), block, Block::Paragraph(right)],
                position,
            )
        };
        let before_block = Block::Paragraph(self.paragraph_for(&focus)?.clone());
        let mut candidate_document = self.document.clone();
        let blocks = caret_structure::block_list_by_environment_ids_mut(
            &mut candidate_document.body,
            &parent_environment_ids,
        )
        .ok_or(EditError::InvalidPosition)?;
        if sibling_index >= blocks.len() {
            return Err(EditError::InvalidPosition);
        }
        blocks.splice(sibling_index..=sibling_index, after.clone());
        candidate_document
            .validate()
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;

        let before_selection = self.selection.clone();
        let before_anchor_visual_affinity = self.selection_anchor_visual_affinity;
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        self.document = candidate_document;
        self.selection = Selection {
            anchor: next_position.clone(),
            focus: next_position,
        };
        self.selection_visual_affinity = crate::VisualAffinity::Downstream;
        self.selection_anchor_visual_affinity = crate::VisualAffinity::Downstream;
        self.clear_structure_selection_state();
        self.clear_host_caret_position();
        self.clear_math_edit();
        self.pending_inline_style.clear();
        let index = build_paragraph_index(&self.document.body);
        self.finalize_structure_edit(
            index,
            DocumentPatch::BlockRange {
                parent_environment_ids,
                sibling_index,
                before: vec![before_block],
                after,
            },
            before_selection,
            before_anchor_visual_affinity,
            before_visual_affinity,
            before_state_id,
        );
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            ..Effect::default()
        })
    }
}

fn inline_nodes_length(nodes: &[ParagraphContent]) -> Result<usize, EditError> {
    nodes.iter().try_fold(0usize, |length, node| {
        length
            .checked_add(inline_length(node)?)
            .ok_or(EditError::InvalidPosition)
    })
}

/// 规范化行内内容：丢弃空 TextFlow，并合并接缝处的相邻 TextFlow。
fn merge_text_flows(inlines: &mut Vec<ParagraphContent>) {
    inlines.retain(|content| match content {
        ParagraphContent::Text(flow) => !flow.runs.iter().all(|run| run.text.is_empty()),
        _ => true,
    });
    let mut index = 1;
    while index < inlines.len() {
        let mergeable = matches!(&inlines[index - 1], ParagraphContent::Text(_))
            && matches!(&inlines[index], ParagraphContent::Text(_));
        if !mergeable {
            index += 1;
            continue;
        }
        let right = inlines.remove(index);
        if let (Some(ParagraphContent::Text(left)), ParagraphContent::Text(right)) =
            (inlines.get_mut(index - 1), right)
        {
            left.runs.extend(right.runs);
        }
    }
}

fn body_has_paragraph(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(_) => true,
        Block::Environment(environment) => {
            body_has_paragraph(&environment.children)
                || environment.title.as_ref().is_some_and(|_| true)
        }
        Block::Table(_) => false,
        Block::Equation(_) | Block::Align(_) | Block::ExplicitBreak(_) | Block::Opaque(_) => false,
    })
}

fn text_length_of_inlines(inlines: &[ParagraphContent]) -> Result<usize, EditError> {
    inline_nodes_length(inlines)
}

// ------------------------------------------------------------------
// 块粘贴
// ------------------------------------------------------------------

impl Session {
    fn paste_block_fragment(&mut self, fragment: ClipboardFragment) -> Result<Effect, EditError> {
        if fragment.blocks.is_empty() {
            return Err(EditError::ClipboardHostMismatch);
        }
        for block in &fragment.blocks {
            validate_copy_block(block)?;
        }
        if fragment.start_clipped && !matches!(fragment.blocks.first(), Some(Block::Paragraph(_))) {
            return Err(EditError::ClipboardHostMismatch);
        }
        if fragment.end_clipped && !matches!(fragment.blocks.last(), Some(Block::Paragraph(_))) {
            return Err(EditError::ClipboardHostMismatch);
        }
        let mut blocks = fragment.blocks;
        caret_doc::regenerate_clipboard_ids(&mut blocks);
        let (start, end) = self.paste_target_boundaries()?;
        let plan = self.plan_block_paste(
            blocks,
            &start,
            &end,
            fragment.start_clipped,
            fragment.end_clipped,
        )?;
        let mut candidate_document = self.document.clone();
        candidate_document.body = plan.body;
        candidate_document
            .validate()
            .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
        if candidate_document == self.document {
            return Ok(Effect::default());
        }
        let mut candidate_paragraph_index = build_paragraph_index(&candidate_document.body);
        let mut caret_result =
            self.paste_caret_position(&candidate_document, &candidate_paragraph_index, &plan.caret);
        if matches!(plan.caret, PasteCaretHint::Block { .. })
            && !body_has_paragraph(&candidate_document.body)
        {
            // 块级对象成为唯一内容时补一个空段落作为合法 caret 之家，与显示
            // 公式插入的既有约定一致；该段落属于同一次补丁与 Undo。
            let paragraph = Paragraph::empty(NodeId::generate());
            let hint = PasteCaretHint::Paragraph {
                paragraph_id: paragraph.id.clone(),
                offset: 0,
            };
            candidate_document.body.push(Block::Paragraph(paragraph));
            candidate_document
                .validate()
                .map_err(|error| EditError::InvalidDocument(error.to_string()))?;
            candidate_paragraph_index = build_paragraph_index(&candidate_document.body);
            caret_result =
                self.paste_caret_position(&candidate_document, &candidate_paragraph_index, &hint);
        }
        let (caret, host_caret_position) = caret_result?;

        let before_body = self.document.body.clone();
        let before_state = self.capture_paste_before_state();
        self.document = candidate_document;
        self.paragraph_index = candidate_paragraph_index;
        self.structure_index =
            std::sync::Arc::new(caret_structure::StructureIndex::build(&self.document));
        self.text_map_cache.clear();
        let affinity = match caret.affinity {
            Affinity::Upstream => crate::VisualAffinity::Upstream,
            Affinity::Downstream => crate::VisualAffinity::Downstream,
        };
        self.selection = Selection {
            anchor: caret.clone(),
            focus: caret,
        };
        self.selection_visual_affinity = affinity;
        self.selection_anchor_visual_affinity = affinity;
        self.set_host_caret_position(host_caret_position);
        self.clear_structure_selection_state();
        self.clear_math_edit();
        self.pending_inline_style.clear();
        self.advance_history_merge_epoch();
        self.commit_document_edit(
            DocumentPatch::Body {
                before: before_body,
                after: self.document.body.clone(),
            },
            before_state,
            DocumentEditSnapshot::Body,
        );
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            ..Effect::default()
        })
    }

    fn paste_target_boundaries(&self) -> Result<(PasteBoundary, PasteBoundary), EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if self.command_line.is_some() {
            return Err(EditError::InvalidPosition);
        }
        if self.math_edit.is_some() {
            return Err(EditError::ClipboardHostMismatch);
        }
        if self.has_structure_selection() {
            let (start, end) = self.ordered_structure_selection()?;
            return Ok((self.paste_boundary(start)?, self.paste_boundary(end)?));
        }
        if let Some((block_id, direction)) = self.block_boundary_exit() {
            let start = DocumentRangeEndpoint::BlockEdge {
                block_id: block_id.clone(),
                side: direction,
            };
            let end = DocumentRangeEndpoint::BlockEdge {
                block_id,
                side: direction,
            };
            return Ok((self.paste_boundary(&start)?, self.paste_boundary(&end)?));
        }
        let anchor = self.selection.anchor.clone();
        let focus = self.selection.focus.clone();
        if anchor.story != focus.story {
            return Err(EditError::CrossParagraphSelection);
        }
        let (start, end) = self.ordered_text_range(&anchor, &focus)?;
        Ok((
            self.paste_boundary(&DocumentRangeEndpoint::Text(start))?,
            self.paste_boundary(&DocumentRangeEndpoint::Text(end))?,
        ))
    }

    fn paste_boundary(&self, endpoint: &DocumentRangeEndpoint) -> Result<PasteBoundary, EditError> {
        match endpoint {
            DocumentRangeEndpoint::Text(position) => {
                if position.story != StoryId::Body {
                    return Err(EditError::ClipboardHostMismatch);
                }
                let location = self
                    .paragraph_index
                    .get(&position.paragraph_id)
                    .cloned()
                    .ok_or(EditError::InvalidPosition)?;
                if location.story != StoryId::Body || location.is_title {
                    return Err(EditError::ClipboardHostMismatch);
                }
                let paragraph = paragraph_at_location(&self.document.body, &location)
                    .ok_or(EditError::InvalidPosition)?;
                let map = TextMap::new(paragraph)?;
                map.validate_pos(position)?;
                let length = map.text.len();
                let child_index = *location
                    .block_path
                    .last()
                    .ok_or(EditError::InvalidPosition)?;
                if position.offset == 0 {
                    Ok(PasteBoundary::AtIndex {
                        parent_environment_ids: location.parent_environment_ids,
                        index: child_index,
                    })
                } else if position.offset >= length {
                    Ok(PasteBoundary::AtIndex {
                        parent_environment_ids: location.parent_environment_ids,
                        index: child_index + 1,
                    })
                } else {
                    Ok(PasteBoundary::InParagraph {
                        parent_environment_ids: location.parent_environment_ids,
                        child_index,
                        paragraph_id: position.paragraph_id.clone(),
                        offset: position.offset,
                    })
                }
            }
            DocumentRangeEndpoint::BlockEdge { block_id, side } => {
                let location = caret_structure::find_document_node(&self.document, block_id)
                    .ok_or(EditError::InvalidPosition)?;
                if !location.kind.is_range_block() {
                    return Err(EditError::ClipboardHostMismatch);
                }
                let index = location.sibling_index.ok_or(EditError::InvalidPosition)?;
                let index = match side {
                    Direction::Backward => index,
                    Direction::Forward => index + 1,
                };
                Ok(PasteBoundary::AtIndex {
                    parent_environment_ids: location.parent_environment_ids,
                    index,
                })
            }
            DocumentRangeEndpoint::InlineMathTree { .. }
            | DocumentRangeEndpoint::InlineMathText { .. } => Err(EditError::ClipboardHostMismatch),
        }
    }

    fn plan_block_paste(
        &self,
        fragment: Vec<Block>,
        start: &PasteBoundary,
        end: &PasteBoundary,
        start_clipped: bool,
        end_clipped: bool,
    ) -> Result<BodyPastePlan, EditError> {
        let common = common_parent_ids(start.parent_ids(), end.parent_ids());
        let children = caret_structure::block_list_by_environment_ids(&self.document.body, &common)
            .ok_or(EditError::InvalidPosition)?
            .to_vec();
        let mut planned = self.rebuild_at_level(
            &children,
            &common,
            start,
            end,
            fragment,
            ClipFlags {
                start_clipped,
                end_clipped,
            },
        )?;
        if planned.caret.is_none() {
            planned.caret = fallback_caret(&mut planned.blocks)?;
        }
        let mut body = self.document.body.clone();
        {
            let target = caret_structure::block_list_by_environment_ids_mut(&mut body, &common)
                .ok_or(EditError::InvalidPosition)?;
            *target = planned.blocks;
        }
        Ok(BodyPastePlan {
            body,
            caret: planned.caret.ok_or(EditError::InvalidPosition)?,
        })
    }

    fn rebuild_at_level(
        &self,
        children: &[Block],
        level_ids: &[NodeId],
        start: &PasteBoundary,
        end: &PasteBoundary,
        fragment: Vec<Block>,
        clipped: ClipFlags,
    ) -> Result<PlannedBlocks, EditError> {
        let ClipFlags {
            start_clipped,
            end_clipped,
        } = clipped;
        let start_local = self.resolve_local_boundary(children, level_ids, start)?;
        let end_local = self.resolve_local_boundary(children, level_ids, end)?;
        if let (
            LocalBoundary::InParagraph {
                child_index: start_child,
                offset: start_offset,
                paragraph: start_paragraph,
            },
            LocalBoundary::InParagraph {
                child_index: end_child,
                offset: end_offset,
                paragraph: end_paragraph,
            },
        ) = (&start_local, &end_local)
            && start_child == end_child
            && start_paragraph.id == end_paragraph.id
        {
            if start_offset > end_offset {
                return Err(EditError::InvalidPosition);
            }
            let (through_end, right) =
                split_inline_nodes(start_paragraph.inlines.clone(), *end_offset)?;
            let (left, _middle) = split_inline_nodes(through_end, *start_offset)?;
            let mut prefix_blocks = Vec::new();
            let mut suffix_blocks = Vec::new();
            if !left.is_empty() || start_clipped {
                let mut prefix = start_paragraph.clone();
                prefix.inlines = left;
                prefix_blocks.push(Block::Paragraph(prefix));
            }
            if !right.is_empty() || end_clipped {
                let mut suffix = end_paragraph.clone();
                // 首段保留目标段落身份；同一段被切成两个独立段落时，后缀必须
                // 获得新身份，否则整文档校验会报重复节点 ID。
                suffix.id = NodeId::generate();
                suffix.inlines = right;
                suffix_blocks.push(Block::Paragraph(suffix));
            }
            return self.splice_fragment(
                prefix_blocks,
                fragment,
                suffix_blocks,
                SpliceOptions {
                    start_merge_allowed: true,
                    end_merge_allowed: true,
                    start_clipped,
                    end_clipped,
                },
            );
        }

        let start_cut;
        let mut prefix_blocks = Vec::new();
        let mut start_merge = false;
        match start_local {
            LocalBoundary::AtIndex(index) => {
                start_cut = index;
                prefix_blocks.extend(children[..index].iter().cloned());
            }
            LocalBoundary::InParagraph {
                child_index,
                offset,
                paragraph,
            } => {
                start_cut = child_index;
                prefix_blocks.extend(children[..child_index].iter().cloned());
                let (left, _) = split_inline_nodes(paragraph.inlines.clone(), offset)?;
                if !left.is_empty() || start_clipped {
                    let mut prefix = paragraph;
                    prefix.inlines = left;
                    prefix_blocks.push(Block::Paragraph(prefix));
                    start_merge = true;
                }
            }
            LocalBoundary::Deeper {
                child_index,
                boundary,
            } => {
                start_cut = child_index;
                prefix_blocks.extend(children[..child_index].iter().cloned());
                prefix_blocks.extend(self.rebuild_prefix_block(
                    &children[child_index],
                    level_ids,
                    &boundary,
                )?);
            }
        }

        let end_cut;
        let end_after;
        let mut suffix_blocks = Vec::new();
        let mut end_merge = false;
        match end_local {
            LocalBoundary::AtIndex(index) => {
                end_cut = index;
                end_after = index;
            }
            LocalBoundary::InParagraph {
                child_index,
                offset,
                paragraph,
            } => {
                end_cut = child_index;
                end_after = child_index + 1;
                let (_, right) = split_inline_nodes(paragraph.inlines.clone(), offset)?;
                if !right.is_empty() || end_clipped {
                    let mut suffix = paragraph;
                    suffix.inlines = right;
                    suffix_blocks.push(Block::Paragraph(suffix));
                    end_merge = true;
                }
            }
            LocalBoundary::Deeper {
                child_index,
                boundary,
            } => {
                end_cut = child_index;
                end_after = child_index + 1;
                suffix_blocks.extend(self.rebuild_suffix_block(
                    &children[child_index],
                    level_ids,
                    &boundary,
                )?);
            }
        }
        if start_cut > end_cut {
            return Err(EditError::InvalidPosition);
        }
        if end_after <= children.len() {
            suffix_blocks.extend(children[end_after..].iter().cloned());
        }
        self.splice_fragment(
            prefix_blocks,
            fragment,
            suffix_blocks,
            SpliceOptions {
                start_merge_allowed: start_merge,
                end_merge_allowed: end_merge,
                start_clipped,
                end_clipped,
            },
        )
    }

    fn resolve_local_boundary(
        &self,
        children: &[Block],
        level_ids: &[NodeId],
        boundary: &PasteBoundary,
    ) -> Result<LocalBoundary, EditError> {
        let parent_ids = boundary.parent_ids();
        if parent_ids == level_ids {
            return match boundary {
                PasteBoundary::AtIndex { index, .. } => {
                    if *index > children.len() {
                        return Err(EditError::InvalidPosition);
                    }
                    Ok(LocalBoundary::AtIndex(*index))
                }
                PasteBoundary::InParagraph {
                    child_index,
                    paragraph_id,
                    offset,
                    ..
                } => {
                    let Some(Block::Paragraph(paragraph)) = children.get(*child_index) else {
                        return Err(EditError::InvalidPosition);
                    };
                    if paragraph.id != *paragraph_id {
                        return Err(EditError::InvalidPosition);
                    }
                    let map = TextMap::new(paragraph)?;
                    if *offset > map.text.len() {
                        return Err(EditError::InvalidPosition);
                    }
                    Ok(LocalBoundary::InParagraph {
                        child_index: *child_index,
                        offset: *offset,
                        paragraph: paragraph.clone(),
                    })
                }
            };
        }
        if parent_ids.len() <= level_ids.len() || !parent_ids.starts_with(level_ids) {
            return Err(EditError::InvalidPosition);
        }
        let child_id = &parent_ids[level_ids.len()];
        let child_index = children
            .iter()
            .position(|block| matches!(block, Block::Environment(environment) if &environment.id == child_id))
            .ok_or(EditError::InvalidPosition)?;
        Ok(LocalBoundary::Deeper {
            child_index,
            boundary: boundary.clone(),
        })
    }

    fn rebuild_prefix_block(
        &self,
        block: &Block,
        parent_ids: &[NodeId],
        boundary: &PasteBoundary,
    ) -> Result<Vec<Block>, EditError> {
        let Block::Environment(environment) = block else {
            return Err(EditError::InvalidPosition);
        };
        let mut child_level = parent_ids.to_vec();
        child_level.push(environment.id.clone());
        let children =
            self.rebuild_prefix_children(&environment.children, &child_level, boundary)?;
        if children.is_empty() && environment.title.is_none() {
            return Ok(Vec::new());
        }
        let mut updated = environment.clone();
        updated.children = children;
        Ok(vec![Block::Environment(updated)])
    }

    fn rebuild_prefix_children(
        &self,
        children: &[Block],
        level_ids: &[NodeId],
        boundary: &PasteBoundary,
    ) -> Result<Vec<Block>, EditError> {
        match self.resolve_local_boundary(children, level_ids, boundary)? {
            LocalBoundary::AtIndex(index) => Ok(children[..index].to_vec()),
            LocalBoundary::InParagraph {
                child_index,
                offset,
                paragraph,
            } => {
                let (left, _) = split_inline_nodes(paragraph.inlines.clone(), offset)?;
                let mut output = children[..child_index].to_vec();
                if !left.is_empty() {
                    let mut prefix = paragraph;
                    prefix.inlines = left;
                    output.push(Block::Paragraph(prefix));
                }
                Ok(output)
            }
            LocalBoundary::Deeper {
                child_index,
                boundary,
            } => {
                let mut output = children[..child_index].to_vec();
                output.extend(self.rebuild_prefix_block(
                    &children[child_index],
                    level_ids,
                    &boundary,
                )?);
                Ok(output)
            }
        }
    }

    fn rebuild_suffix_block(
        &self,
        block: &Block,
        parent_ids: &[NodeId],
        boundary: &PasteBoundary,
    ) -> Result<Vec<Block>, EditError> {
        let Block::Environment(environment) = block else {
            return Err(EditError::InvalidPosition);
        };
        let mut child_level = parent_ids.to_vec();
        child_level.push(environment.id.clone());
        let children =
            self.rebuild_suffix_children(&environment.children, &child_level, boundary)?;
        if children.is_empty() {
            return Ok(Vec::new());
        }
        let mut updated = environment.clone();
        updated.title = None;
        updated.children = children;
        Ok(vec![Block::Environment(updated)])
    }

    fn rebuild_suffix_children(
        &self,
        children: &[Block],
        level_ids: &[NodeId],
        boundary: &PasteBoundary,
    ) -> Result<Vec<Block>, EditError> {
        match self.resolve_local_boundary(children, level_ids, boundary)? {
            LocalBoundary::AtIndex(index) => Ok(children[index..].to_vec()),
            LocalBoundary::InParagraph {
                child_index,
                offset,
                paragraph,
            } => {
                let (_, right) = split_inline_nodes(paragraph.inlines.clone(), offset)?;
                let mut output = Vec::new();
                if !right.is_empty() {
                    let mut suffix = paragraph;
                    suffix.inlines = right;
                    output.push(Block::Paragraph(suffix));
                }
                output.extend(children[child_index + 1..].iter().cloned());
                Ok(output)
            }
            LocalBoundary::Deeper {
                child_index,
                boundary,
            } => {
                let mut output =
                    self.rebuild_suffix_block(&children[child_index], level_ids, &boundary)?;
                output.extend(children[child_index + 1..].iter().cloned());
                Ok(output)
            }
        }
    }

    fn splice_fragment(
        &self,
        mut prefix: Vec<Block>,
        mut fragment: Vec<Block>,
        mut suffix: Vec<Block>,
        options: SpliceOptions,
    ) -> Result<PlannedBlocks, EditError> {
        let SpliceOptions {
            start_merge_allowed,
            end_merge_allowed,
            start_clipped,
            end_clipped,
        } = options;
        if start_clipped && !matches!(fragment.first(), Some(Block::Paragraph(_))) {
            return Err(EditError::ClipboardHostMismatch);
        }
        if end_clipped && !matches!(fragment.last(), Some(Block::Paragraph(_))) {
            return Err(EditError::ClipboardHostMismatch);
        }
        let mut caret: Option<PasteCaretHint> = None;
        let mut end_merged = false;
        if start_clipped && start_merge_allowed {
            let Some(Block::Paragraph(prefix_paragraph)) = prefix.last_mut() else {
                return Err(EditError::ClipboardHostMismatch);
            };
            let first = fragment.remove(0);
            let Block::Paragraph(first) = first else {
                return Err(EditError::ClipboardHostMismatch);
            };
            prefix_paragraph.inlines.extend(first.inlines);
            merge_text_flows(&mut prefix_paragraph.inlines);
            caret = Some(PasteCaretHint::Paragraph {
                paragraph_id: prefix_paragraph.id.clone(),
                offset: text_length_of_inlines(&prefix_paragraph.inlines)?,
            });
        }
        if end_clipped && end_merge_allowed {
            end_merged = true;
            if fragment.is_empty() {
                let Some(Block::Paragraph(prefix_paragraph)) = prefix.last_mut() else {
                    return Err(EditError::ClipboardHostMismatch);
                };
                let Some(Block::Paragraph(suffix_paragraph)) = suffix.first() else {
                    return Err(EditError::ClipboardHostMismatch);
                };
                let offset = text_length_of_inlines(&prefix_paragraph.inlines)?;
                prefix_paragraph
                    .inlines
                    .extend(suffix_paragraph.inlines.clone());
                merge_text_flows(&mut prefix_paragraph.inlines);
                suffix.remove(0);
                caret = Some(PasteCaretHint::Paragraph {
                    paragraph_id: prefix_paragraph.id.clone(),
                    offset,
                });
            } else {
                let Some(Block::Paragraph(fragment_paragraph)) = fragment.last_mut() else {
                    return Err(EditError::ClipboardHostMismatch);
                };
                let Some(Block::Paragraph(suffix_paragraph)) = suffix.first() else {
                    return Err(EditError::ClipboardHostMismatch);
                };
                let offset = text_length_of_inlines(&fragment_paragraph.inlines)?;
                fragment_paragraph
                    .inlines
                    .extend(suffix_paragraph.inlines.clone());
                merge_text_flows(&mut fragment_paragraph.inlines);
                suffix.remove(0);
                caret = Some(PasteCaretHint::Paragraph {
                    paragraph_id: fragment_paragraph.id.clone(),
                    offset,
                });
            }
        }
        if matches!(fragment.last(), Some(Block::ExplicitBreak(_))) {
            // 片段以页断结束时，caret 落到页断后的首个段落；没有后缀时落在前缀段尾。
            caret = first_paragraph_hint(&suffix)?
                .or(last_paragraph_hint(&prefix)?)
                .or(caret);
        } else if !fragment.is_empty() && !end_merged {
            // start 合并的 caret 在仍有后续块时必须让位给片段末尾；
            // end 合并已把后缀拼进片段末段，caret 应停留在插入接缝。
            caret = caret_hint_for_blocks(&fragment)?.or(caret);
        } else if caret.is_none() {
            caret = caret_hint_for_blocks(&fragment)?;
        }
        let mut blocks = prefix;
        blocks.extend(fragment);
        blocks.extend(suffix);
        Ok(PlannedBlocks { blocks, caret })
    }

    fn paste_caret_position(
        &self,
        document: &caret_doc::Document,
        paragraph_index: &super::super::paragraph_ops::ParagraphIndex,
        hint: &PasteCaretHint,
    ) -> Result<(Pos, Option<HostCaretPosition<Pos>>), EditError> {
        match hint {
            PasteCaretHint::Paragraph {
                paragraph_id,
                offset,
            } => {
                let location = paragraph_index
                    .get(paragraph_id)
                    .ok_or(EditError::InvalidPosition)?;
                let paragraph = paragraph_at_location(&document.body, location)
                    .ok_or(EditError::InvalidPosition)?;
                let map = TextMap::new(paragraph)?;
                let offset = map
                    .snap_to_grapheme_boundary((*offset).min(map.text.len()), Affinity::Downstream);
                let position = Pos {
                    story: location.story.clone(),
                    paragraph_id: paragraph_id.clone(),
                    offset,
                    affinity: Affinity::Downstream,
                };
                map.validate_pos(&position)?;
                Ok((position, None))
            }
            PasteCaretHint::Block { block_id } => {
                let block_path =
                    block_path_of(&document.body, block_id).ok_or(EditError::InvalidPosition)?;
                let mut before: Option<(&ParagraphLocation, usize)> = None;
                let mut after: Option<(&ParagraphLocation, usize)> = None;
                for location in paragraph_index.values() {
                    if location.block_path.as_slice() < block_path.as_slice() {
                        let paragraph = paragraph_at_location(&document.body, location)
                            .ok_or(EditError::InvalidPosition)?;
                        let length = TextMap::new(paragraph)?.text.len();
                        if before
                            .as_ref()
                            .is_none_or(|(current, _)| location.block_path > current.block_path)
                        {
                            before = Some((location, length));
                        }
                    } else if location.block_path.as_slice() > block_path.as_slice()
                        && after
                            .as_ref()
                            .is_none_or(|(current, _)| location.block_path < current.block_path)
                    {
                        after = Some((location, 0));
                    }
                }
                if let Some((location, offset)) = before {
                    let paragraph = paragraph_at_location(&document.body, location)
                        .ok_or(EditError::InvalidPosition)?;
                    let map = TextMap::new(paragraph)?;
                    let position = Pos {
                        story: location.story.clone(),
                        paragraph_id: location.paragraph_id.clone(),
                        offset: map.snap_to_grapheme_boundary(offset, Affinity::Upstream),
                        affinity: Affinity::Upstream,
                    };
                    map.validate_pos(&position)?;
                    return Ok((
                        position,
                        Some(HostCaretPosition::After {
                            host_id: block_id.clone(),
                            kind: HostCaretKind::Block,
                        }),
                    ));
                }
                if let Some((location, offset)) = after {
                    let paragraph = paragraph_at_location(&document.body, location)
                        .ok_or(EditError::InvalidPosition)?;
                    let map = TextMap::new(paragraph)?;
                    let position = Pos {
                        story: location.story.clone(),
                        paragraph_id: location.paragraph_id.clone(),
                        offset,
                        affinity: Affinity::Downstream,
                    };
                    map.validate_pos(&position)?;
                    return Ok((
                        position,
                        Some(HostCaretPosition::Before {
                            host_id: block_id.clone(),
                            kind: HostCaretKind::Block,
                        }),
                    ));
                }
                Err(EditError::InvalidPosition)
            }
        }
    }
}

/// 入站片段的首尾裁切标记。
#[derive(Debug, Clone, Copy)]
struct ClipFlags {
    start_clipped: bool,
    end_clipped: bool,
}

/// 片段拼接时的目标侧合并许可与源侧裁切标记。
#[derive(Debug, Clone, Copy)]
struct SpliceOptions {
    start_merge_allowed: bool,
    end_merge_allowed: bool,
    start_clipped: bool,
    end_clipped: bool,
}

/// 规划后的块序列与折叠选区提示。
struct PlannedBlocks {
    blocks: Vec<Block>,
    caret: Option<PasteCaretHint>,
}

fn first_paragraph_hint(blocks: &[Block]) -> Result<Option<PasteCaretHint>, EditError> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                return Ok(Some(PasteCaretHint::Paragraph {
                    paragraph_id: paragraph.id.clone(),
                    offset: 0,
                }));
            }
            Block::Environment(environment) => {
                if let Some(title) = &environment.title {
                    return Ok(Some(PasteCaretHint::Paragraph {
                        paragraph_id: title.id.clone(),
                        offset: 0,
                    }));
                }
                if let Some(result) = first_paragraph_hint(&environment.children)? {
                    return Ok(Some(result));
                }
            }
            Block::Equation(_)
            | Block::Align(_)
            | Block::Table(_)
            | Block::ExplicitBreak(_)
            | Block::Opaque(_) => {}
        }
    }
    Ok(None)
}

fn last_paragraph_hint(blocks: &[Block]) -> Result<Option<PasteCaretHint>, EditError> {
    for block in blocks.iter().rev() {
        match block {
            Block::Paragraph(paragraph) => {
                return Ok(Some(PasteCaretHint::Paragraph {
                    paragraph_id: paragraph.id.clone(),
                    offset: inline_nodes_length(&paragraph.inlines)?,
                }));
            }
            Block::Environment(environment) => {
                if let Some(result) = last_paragraph_in_environment(environment)? {
                    return Ok(Some(PasteCaretHint::Paragraph {
                        paragraph_id: result.0,
                        offset: result.1,
                    }));
                }
            }
            Block::Equation(_)
            | Block::Align(_)
            | Block::Table(_)
            | Block::ExplicitBreak(_)
            | Block::Opaque(_) => {}
        }
    }
    Ok(None)
}

fn caret_hint_for_blocks(blocks: &[Block]) -> Result<Option<PasteCaretHint>, EditError> {
    let Some(last) = blocks.last() else {
        return Ok(None);
    };
    let hint = match last {
        Block::Paragraph(paragraph) => Some(PasteCaretHint::Paragraph {
            paragraph_id: paragraph.id.clone(),
            offset: inline_nodes_length(&paragraph.inlines)?,
        }),
        Block::Environment(environment) => match last_paragraph_in_environment(environment)? {
            Some((paragraph, offset)) => Some(PasteCaretHint::Paragraph {
                paragraph_id: paragraph,
                offset,
            }),
            None => Some(PasteCaretHint::Block {
                block_id: environment.id.clone(),
            }),
        },
        Block::Equation(equation) => Some(PasteCaretHint::Block {
            block_id: equation.id.clone(),
        }),
        Block::Align(align) => Some(PasteCaretHint::Block {
            block_id: align.id.clone(),
        }),
        Block::Table(_) => return Err(EditError::UnsupportedTableContent),
        Block::ExplicitBreak(_) => None,
        Block::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
    };
    Ok(hint)
}

fn fallback_caret(blocks: &mut Vec<Block>) -> Result<Option<PasteCaretHint>, EditError> {
    let hint = caret_hint_for_blocks(blocks)?;
    if hint.is_some() {
        return Ok(hint);
    }
    let paragraph = Paragraph::empty(NodeId::generate());
    let id = paragraph.id.clone();
    blocks.push(Block::Paragraph(paragraph));
    Ok(Some(PasteCaretHint::Paragraph {
        paragraph_id: id,
        offset: 0,
    }))
}

fn last_paragraph_in_environment(
    environment: &caret_doc::BlockEnvironment,
) -> Result<Option<(NodeId, usize)>, EditError> {
    for child in environment.children.iter().rev() {
        match child {
            Block::Paragraph(paragraph) => {
                return Ok(Some((
                    paragraph.id.clone(),
                    inline_nodes_length(&paragraph.inlines)?,
                )));
            }
            Block::Environment(inner) => {
                if let Some(result) = last_paragraph_in_environment(inner)? {
                    return Ok(Some(result));
                }
            }
            Block::Equation(_)
            | Block::Align(_)
            | Block::Table(_)
            | Block::ExplicitBreak(_)
            | Block::Opaque(_) => {}
        }
    }
    if let Some(title) = &environment.title {
        return Ok(Some((
            title.id.clone(),
            inline_nodes_length(&title.inlines)?,
        )));
    }
    Ok(None)
}

fn block_path_of(blocks: &[Block], block_id: &NodeId) -> Option<Vec<usize>> {
    fn visit(blocks: &[Block], path: &mut Vec<usize>, target: &NodeId) -> Option<Vec<usize>> {
        for (index, block) in blocks.iter().enumerate() {
            path.push(index);
            match block {
                Block::Environment(environment) => {
                    if &environment.id == target {
                        return Some(path.clone());
                    }
                    if let Some(found) = visit(&environment.children, path, target) {
                        return Some(found);
                    }
                }
                Block::Equation(equation) if &equation.id == target => return Some(path.clone()),
                Block::Align(align) if &align.id == target => return Some(path.clone()),
                _ => {}
            }
            path.pop();
        }
        None
    }
    visit(blocks, &mut Vec::new(), block_id)
}

fn common_parent_ids(left: &[NodeId], right: &[NodeId]) -> Vec<NodeId> {
    let mut common = Vec::new();
    for (left_id, right_id) in left.iter().zip(right) {
        if left_id != right_id {
            break;
        }
        common.push(left_id.clone());
    }
    common
}

#[cfg(test)]
#[path = "clipboard_fragment/tests.rs"]
mod tests;
