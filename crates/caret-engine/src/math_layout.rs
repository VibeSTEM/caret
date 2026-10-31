//! 把行内公式、显示公式与对齐公式接入段落布局：收集占位槽、合成显示伪段落并换算行内盒度量。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::{collections::BTreeMap, ops::Range, sync::Arc};

use caret_doc::{
    Align, Equation, InlineMath, InlineNode, MATH_TREE_SCHEMA_VERSION, MathNode, MathNodeKind,
    MathTree, NodeId, Paragraph,
};
use caret_layout::{InkBounds, InlineObjectMetrics};
use caret_math::layout::{MathFontError, MathLayout, MathLayoutEngine, MathStyle};

/// U+FFFC 占位字符的 UTF-8 长度。
const PLACEHOLDER_LEN: usize = '\u{fffc}'.len_utf8();

/// 段落中一个已排版的公式盒；`source_bytes` 覆盖其 U+FFFC 占位字符。
#[derive(Debug, Clone)]
pub struct MathBox {
    pub id: NodeId,
    pub source_bytes: Range<usize>,
    pub layout: Arc<MathLayout>,
}

impl MathBox {
    pub(crate) fn object_metrics(&self) -> InlineObjectMetrics {
        let width = self.layout.width as f32;
        let ascent = self.layout.ascent.max(0.0) as f32;
        let descent = self.layout.descent.max(0.0) as f32;
        InlineObjectMetrics {
            source_bytes: self.source_bytes.clone(),
            advance_pt: width.max(0.0),
            ascent_pt: ascent,
            descent_pt: descent,
            ink_bounds: InkBounds {
                left: 0.0,
                top: -ascent,
                right: width.max(0.0),
                bottom: descent,
            },
        }
    }
}

/// 按逻辑顺序排版段落中的全部行内公式；字节偏移与 `paragraph_text` 的占位规则一致。
/// 公式引擎在首次遇到公式时才加载内置字体，此后驻留于布局服务中。
pub(crate) fn layout_paragraph_math(
    engine: &mut Option<MathLayoutEngine>,
    paragraph: &Paragraph,
    font_size_pt: f64,
    style: MathStyle,
) -> Result<Vec<MathBox>, MathFontError> {
    fn visit<'a>(
        nodes: &'a [InlineNode],
        offset: &mut usize,
        output: &mut Vec<(Range<usize>, &'a InlineMath)>,
    ) {
        for node in nodes {
            match node {
                InlineNode::Run(run) => *offset += run.text.len(),
                InlineNode::Environment(environment) => {
                    visit(&environment.children, offset, output)
                }
                InlineNode::SoftBreak(_) => *offset += 1,
                InlineNode::Opaque(_) => *offset += PLACEHOLDER_LEN,
                InlineNode::Math(math) => {
                    output.push((*offset..*offset + PLACEHOLDER_LEN, math));
                    *offset += PLACEHOLDER_LEN;
                }
            }
        }
    }
    let mut slots = Vec::new();
    visit(&paragraph.inlines, &mut 0, &mut slots);
    if slots.is_empty() {
        return Ok(Vec::new());
    }
    let engine = match engine {
        Some(engine) => engine,
        None => engine.insert(MathLayoutEngine::new()?),
    };
    Ok(slots
        .into_iter()
        .map(|(source_bytes, math)| MathBox {
            id: math.id.clone(),
            source_bytes,
            layout: engine.layout(&math.math_tree.root, font_size_pt, style),
        })
        .collect())
}

/// 显示公式的伪段落：段落 ID 即公式块 ID，唯一行内节点为整棵数学树。
pub(crate) fn equation_paragraph(equation: &Equation) -> Paragraph {
    display_paragraph(equation.id.clone(), equation.math_tree.clone())
}

/// 对齐公式合成为一棵 `aligned` 矩阵树：行节点沿用 MathAlignRow ID，单元格沿用各自根节点。
pub(crate) fn align_paragraph(align: &Align) -> Paragraph {
    let rows = align
        .rows
        .iter()
        .map(|row| MathNode {
            id: row.id.clone(),
            kind: MathNodeKind::Row,
            text: None,
            attributes: BTreeMap::new(),
            source_range: None,
            children: row.cells.iter().map(|cell| cell.root.clone()).collect(),
            extra: BTreeMap::new(),
        })
        .collect();
    let root = MathNode {
        id: align.id.clone(),
        kind: MathNodeKind::Matrix,
        text: None,
        attributes: BTreeMap::from([("style".to_owned(), "aligned".to_owned())]),
        source_range: None,
        children: rows,
        extra: BTreeMap::new(),
    };
    display_paragraph(
        align.id.clone(),
        MathTree {
            schema_version: MATH_TREE_SCHEMA_VERSION,
            source: align.source.clone(),
            root,
            extra: BTreeMap::new(),
        },
    )
}

fn display_paragraph(id: NodeId, math_tree: MathTree) -> Paragraph {
    Paragraph {
        id: id.clone(),
        language: None,
        layout: None,
        inlines: vec![InlineNode::Math(InlineMath {
            id,
            math_tree,
            extra: BTreeMap::new(),
        })],
        mark_style: BTreeMap::new(),
        props: BTreeMap::new(),
        extra: BTreeMap::new(),
    }
}
