//! 将 Caret 无关的数学语法树布局成稳定的 glyph、rule 和节点几何快照。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::{MathInputKind, MathSyntaxKind, MathSyntaxNode, MathSyntaxTree};
use convert::{register_provenance, to_latex_node};
use flatten::{append_provenance_aliases, flatten_box, to_micro_pt};
use latex_rust::{
    Dim, ExternalTextMetrics, MathFont, MathNode, MathStyle, TextShaper as LatexTextShaper,
    layout_with_caller_context,
};
mod convert;
mod flatten;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt::{Display, Formatter};

pub(super) const MICRO_POINTS_PER_POINT: i64 = 1_000_000;
const CENTIPOINTS_PER_POINT: i64 = 100;

fn transparent_child_provenance(node: &MathSyntaxNode) -> Option<&str> {
    if node.kind == MathSyntaxKind::Group && node.children.len() == 1 {
        transparent_child_provenance(&node.children[0]).or(node.provenance.as_deref())
    } else {
        node.provenance.as_deref()
    }
}

/// 单次 TeX 数学盒布局产生的页面无关绘制原语；坐标单位为百万分之一磅。
/// 公式基线为 y=0，向下为正；字形位置表示基线，矩形位置表示左上角。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathLayoutItem {
    /// STIX Two Math 字形及其字号和基线位置。
    Glyph {
        glyph_id: u16,
        font_size_micro_pt: i64,
        x_micro_pt: i64,
        baseline_y_micro_pt: i64,
        color: Option<[u8; 3]>,
    },
    /// 调用方塑形的正文片段；`handle` 由正文 shaper 在本次事务提交后持有并供绘制方解析。
    ExternalText {
        handle: u64,
        x_micro_pt: i64,
        baseline_y_micro_pt: i64,
        color: Option<[u8; 3]>,
    },
    /// TeX 填充线条，例如分数线和矩阵横线。
    Rule {
        x_micro_pt: i64,
        y_micro_pt: i64,
        width_micro_pt: i64,
        height_micro_pt: i64,
        color: Option<[u8; 3]>,
    },
    /// TeX 线段，例如取消线和装饰线。
    Line {
        x1_micro_pt: i64,
        y1_micro_pt: i64,
        x2_micro_pt: i64,
        y2_micro_pt: i64,
        thickness_micro_pt: i64,
        color: Option<[u8; 3]>,
    },
    /// 背景矩形或描边边框。
    Rectangle {
        x_micro_pt: i64,
        y_micro_pt: i64,
        width_micro_pt: i64,
        height_micro_pt: i64,
        fill: Option<[u8; 3]>,
        stroke: Option<(Option<[u8; 3]>, i64)>,
    },
}

/// 单个布局代际中 Caret 数学节点 ID 对应的几何；位置以公式基线为原点，单位为百万分之一磅。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathNodeGeometry {
    pub node_id: String,
    pub x_micro_pt: i64,
    pub y_micro_pt: i64,
    pub width_micro_pt: i64,
    pub ascent_micro_pt: i64,
    pub descent_micro_pt: i64,
}

/// 供绘制、命中与光标投影共用的公式几何快照。
///
/// `items` 保留 MathBox 的绘制原语顺序。`nodes` 只包含后端能映射到实际 MathBox 的 provenance；正文片段、分组文本和没有独立 MathBox 的环境行不会伪造精确几何。
/// 所有尺寸使用百万分之一磅；公式基线为 y=0，y 轴向下。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathLayoutSnapshot {
    /// 快照中所有数学字形所用固定字体面的内容哈希。
    pub font_resource_sha256: &'static str,
    pub width_micro_pt: i64,
    pub ascent_micro_pt: i64,
    pub descent_micro_pt: i64,
    pub items: Vec<MathLayoutItem>,
    pub nodes: Vec<MathNodeGeometry>,
    pub diagnostics: Vec<MathLayoutDiagnostic>,
}

/// 文档字体偏好不受支持时可观察到的回退结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathLayoutDiagnostic {
    UnsupportedFontFamily {
        requested_family: String,
        used_family: &'static str,
    },
}

/// 数学快照生成失败；失败时不会返回部分快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathLayoutError {
    InvalidFontSize,
    UnsupportedTree(String),
    Backend(String),
    GeometryOverflow,
}

impl Display for MathLayoutError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFontSize => formatter.write_str("math.layout.invalid_font_size"),
            Self::UnsupportedTree(reason) => {
                write!(formatter, "math.layout.unsupported_tree: {reason}")
            }
            Self::Backend(reason) => write!(formatter, "math.layout.backend: {reason}"),
            Self::GeometryOverflow => formatter.write_str("math.layout.geometry_overflow"),
        }
    }
}

impl std::error::Error for MathLayoutError {}

struct TextLayoutTransaction<'a> {
    shaper: &'a dyn MathTextShaper,
    committed: bool,
}

impl TextLayoutTransaction<'_> {
    fn commit(mut self) {
        self.committed = true;
        self.shaper.finish_layout(true);
    }
}

/// Caret 自有的数学正文塑形样式，不暴露 latex-rust 的 parser 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathTextStyle {
    /// 罗马体。
    Rm,
    /// 粗体。
    Bf,
    /// 斜体。
    It,
    /// 无衬线体。
    Sf,
    /// 等宽体。
    Tt,
    /// 双线体。
    Bb,
    /// 花体。
    Cal,
    /// 哥特体。
    Frak,
    /// 手写体。
    Scr,
    /// 数学粗体符号。
    Boldsymbol,
    /// 伪粗体。
    Pmb,
    /// 正文文本。
    Text,
}

/// 正文塑形器返回的物理度量与 provider 局部资源句柄，单位为百万分之一磅。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathTextMetrics {
    /// 由调用方正文 shaper 分配的资源句柄。
    pub handle: u64,
    /// 正文推进宽度。
    pub width_micro_pt: i64,
    /// 基线上方高度。
    pub ascent_micro_pt: i64,
    /// 基线下方深度。
    pub descent_micro_pt: i64,
}

/// `\text{...}` 使用的调用方正文塑形接口。
/// 一次布局内同步顺序调用；事务成功后调用方持有返回句柄，失败时使本次句柄失效。
pub trait MathTextShaper {
    /// 开始一次布局事务；返回错误时调用方自行清理未完成的初始化。
    fn begin_layout(&self) -> Result<(), String> {
        Ok(())
    }

    /// 按数学样式和物理字号塑形一段正文文本。
    fn shape_text(
        &self,
        text: &str,
        style: MathTextStyle,
        font_size_micro_pt: i64,
    ) -> Result<MathTextMetrics, String>;

    /// 成功时提交句柄，失败时使本次事务创建的句柄失效。
    fn finish_layout(&self, _committed: bool) {}
}

struct LatexTextShaperAdapter<'a>(&'a dyn MathTextShaper);

impl LatexTextShaper for LatexTextShaperAdapter<'_> {
    fn shape_text(
        &self,
        text: &str,
        style: latex_rust::TextStyle,
        font_size_micro_pt: i64,
    ) -> Result<ExternalTextMetrics, String> {
        let style = match style {
            latex_rust::TextStyle::Rm => MathTextStyle::Rm,
            latex_rust::TextStyle::Bf => MathTextStyle::Bf,
            latex_rust::TextStyle::It => MathTextStyle::It,
            latex_rust::TextStyle::Sf => MathTextStyle::Sf,
            latex_rust::TextStyle::Tt => MathTextStyle::Tt,
            latex_rust::TextStyle::Bb => MathTextStyle::Bb,
            latex_rust::TextStyle::Cal => MathTextStyle::Cal,
            latex_rust::TextStyle::Frak => MathTextStyle::Frak,
            latex_rust::TextStyle::Scr => MathTextStyle::Scr,
            latex_rust::TextStyle::Boldsymbol => MathTextStyle::Boldsymbol,
            latex_rust::TextStyle::Pmb => MathTextStyle::Pmb,
            latex_rust::TextStyle::Text => MathTextStyle::Text,
        };
        self.0
            .shape_text(text, style, font_size_micro_pt)
            .map(|metrics| ExternalTextMetrics {
                handle: metrics.handle,
                width_micro_pt: metrics.width_micro_pt,
                ascent_micro_pt: metrics.ascent_micro_pt,
                descent_micro_pt: metrics.descent_micro_pt,
            })
    }
}

impl Drop for TextLayoutTransaction<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.shaper.finish_layout(false);
        }
    }
}

/// 可复用的 STIX Two Math 布局后端；第三方字体和 AST 不暴露到公开接口。
pub struct MathLayoutBackend {
    font: MathFont,
    font_diagnostic: Option<MathLayoutDiagnostic>,
}

impl MathLayoutBackend {
    /// 为布局服务加载固定版本的内嵌 STIX Two Math 字体。
    pub fn new() -> Result<Self, MathLayoutError> {
        Self::for_requested_font_family(None)
    }

    /// 加载 STIX Two Math；遇到不支持的字体偏好时记录可观察的回退诊断。
    pub fn for_requested_font_family(
        requested_family: Option<&str>,
    ) -> Result<Self, MathLayoutError> {
        let font_diagnostic = requested_family
            .filter(|family| !family.eq_ignore_ascii_case(latex_rust::STIX_TWO_MATH_NAME))
            .map(|family| MathLayoutDiagnostic::UnsupportedFontFamily {
                requested_family: family.to_owned(),
                used_family: latex_rust::STIX_TWO_MATH_NAME,
            });
        MathFont::stix_two_math()
            .map(|font| Self {
                font,
                font_diagnostic,
            })
            .map_err(|error| MathLayoutError::Backend(error.to_string()))
    }

    /// 生成完整快照。正文 shaper 失败、树超出预算或几何溢出时返回错误且不产生部分结果；
    /// `begin_layout` 成功后的失败会以 `finish_layout(false)` 回滚 shaper 事务；成功则提交其资源句柄。
    pub fn layout(
        &self,
        tree: &MathSyntaxTree,
        font_size_centipoints: u16,
        text_shaper: &dyn MathTextShaper,
    ) -> Result<MathLayoutSnapshot, MathLayoutError> {
        if font_size_centipoints == 0 {
            return Err(MathLayoutError::InvalidFontSize);
        }
        let max_depth = layout_depth_limit(tree);
        validate_syntax_budget(tree, max_depth)?;
        text_shaper
            .begin_layout()
            .map_err(MathLayoutError::Backend)?;
        let transaction = TextLayoutTransaction {
            shaper: text_shaper,
            committed: false,
        };
        let latex_text_shaper = LatexTextShaperAdapter(text_shaper);
        let root_em_micro_pt = i64::from(font_size_centipoints)
            .checked_mul(MICRO_POINTS_PER_POINT)
            .and_then(|value| value.checked_div(CENTIPOINTS_PER_POINT))
            .ok_or(MathLayoutError::GeometryOverflow)?;
        let em_size_pt = Dim::ratio(root_em_micro_pt, MICRO_POINTS_PER_POINT);
        if em_size_pt.is_nan() {
            return Err(MathLayoutError::InvalidFontSize);
        }
        let mut provenance = HashMap::new();
        let mut ast = to_latex_node(&tree.root, &mut provenance)?;
        register_provenance(&ast, &tree.root, &mut provenance);
        let node_provenance =
            |node: &MathNode| provenance.get(&(node as *const MathNode as usize)).cloned();
        let math_style = match tree.input_kind {
            MathInputKind::Inline => MathStyle::Text,
            MathInputKind::Equation | MathInputKind::Align => MathStyle::Display,
        };
        let boxed = layout_with_caller_context(
            &ast,
            &self.font,
            math_style,
            max_depth,
            &em_size_pt,
            &latex_text_shaper,
            &node_provenance,
        )
        .map_err(|error| MathLayoutError::Backend(error.to_string()))?;
        let mut snapshot = MathLayoutSnapshot {
            font_resource_sha256: latex_rust::STIX_TWO_MATH_SHA256,
            width_micro_pt: to_micro_pt(&boxed.width, root_em_micro_pt)?,
            ascent_micro_pt: to_micro_pt(&boxed.height, root_em_micro_pt)?,
            descent_micro_pt: to_micro_pt(&boxed.depth, root_em_micro_pt)?,
            items: Vec::new(),
            nodes: Vec::new(),
            diagnostics: self.font_diagnostic.iter().cloned().collect(),
        };
        flatten_box(&boxed, 0, 0, None, root_em_micro_pt, true, &mut snapshot)?;
        append_provenance_aliases(tree, &mut snapshot);
        // latex-rust 为深层 AST 提供受保护的迭代析构，避免递归析构耗尽栈。
        ast = MathNode::Row(Vec::new());
        drop(ast);
        transaction.commit();
        Ok(snapshot)
    }
}

fn layout_depth_limit(tree: &MathSyntaxTree) -> usize {
    crate::MAX_MATH_NESTING_DEPTH
        + if tree.input_kind == MathInputKind::Align
            && tree.root.kind == MathSyntaxKind::Matrix
            && tree.root.attributes.get("style").map(String::as_str) == Some("aligned")
            && tree.root.children.len() <= crate::MAX_MATH_SYNTAX_NODES
            && tree
                .root
                .children
                .iter()
                .all(|row| row.kind == MathSyntaxKind::Row)
        {
            2
        } else {
            0
        }
}

fn validate_syntax_budget(tree: &MathSyntaxTree, max_depth: usize) -> Result<(), MathLayoutError> {
    let mut pending = vec![(&tree.root, 1usize)];
    let mut count = 0usize;
    while let Some((node, depth)) = pending.pop() {
        count = count
            .checked_add(1)
            .ok_or(MathLayoutError::GeometryOverflow)?;
        count = count
            .checked_add(node.provenance_aliases.len())
            .ok_or(MathLayoutError::GeometryOverflow)?;
        if depth > max_depth {
            return Err(MathLayoutError::UnsupportedTree(format!(
                "syntax tree exceeds the configured nesting depth of {max_depth}"
            )));
        }
        if count > crate::MAX_MATH_SYNTAX_NODES {
            return Err(MathLayoutError::UnsupportedTree(
                "syntax tree exceeds the configured node budget".into(),
            ));
        }
        if node.attributes.contains_key("atomClass") {
            let remaining = crate::MAX_MATH_SYNTAX_NODES.saturating_sub(count);
            let expanded_atoms = node.text.as_deref().map_or(0, |text| {
                text.chars().take(remaining.saturating_add(2)).count()
            });
            if expanded_atoms.saturating_sub(1) > remaining {
                return Err(MathLayoutError::UnsupportedTree(
                    "expanded syntax tree exceeds the configured node budget".into(),
                ));
            }
            count = count
                .checked_add(expanded_atoms.saturating_sub(1))
                .ok_or(MathLayoutError::GeometryOverflow)?;
        }
        let child_depth = depth
            .checked_add(1)
            .ok_or(MathLayoutError::GeometryOverflow)?;
        if count
            .saturating_add(pending.len())
            .saturating_add(node.children.len())
            > crate::MAX_MATH_SYNTAX_NODES
        {
            return Err(MathLayoutError::UnsupportedTree(
                "syntax tree exceeds the configured node budget".into(),
            ));
        }
        pending.extend(node.children.iter().map(|child| (child, child_depth)));
    }
    Ok(())
}
