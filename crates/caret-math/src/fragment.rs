//! 共享数学片段转换：受宿主类型约束的解析与检查式序列化。
//!
//! 各文档格式适配器、公式源码输入和剪贴板都只通过 [`MathFragment`] 处理数学
//! 子语言：解析复用 [`crate::parse`]，序列化复用现有规范 TeX 序列化器，不引入
//! 第二套 AST 或第二套输出实现。检查式序列化会拒绝无法无损表达的元数据与
//! 不透明内容，而不是静默丢弃；损失以路径和原因返回给调用方。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::fmt;

use caret_doc::{
    Align, InlineEnvironmentKind, MathContent, MathFlow, MathNode, MathNodeKind, MathTree,
    ModelError,
};

use crate::{MathInputKind, MathParseError, ParsedMath, parse, serialize_align, serialize_tree};

/// 经过宿主类型约束的数学片段。
///
/// `kind` 决定规范模型变体：`Inline`/`Equation` 对应 [`MathTree`]，`Align` 对应
/// [`Align`]。构造与解析都维持该不变式，消费者不需要再次分派宿主种类。
#[derive(Debug, Clone, PartialEq)]
pub struct MathFragment {
    kind: MathInputKind,
    math: ParsedMath,
}

/// 数学片段模型的路径步骤；只描述片段内部位置，不包含源码偏移或文档会话状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathFragmentPathStep {
    /// 当前 [`MathFlow`] 的内容项。
    Content(usize),
    /// 当前数学节点的操作数流。
    Operand(usize),
    /// 当前行内环境的子项。
    EnvironmentChild(usize),
    /// 文本流中的 Run。
    TextRun(usize),
    /// Align 的行。
    AlignRow(usize),
    /// Align 的行内 cell（行号由前一个 [`Self::AlignRow`] 步骤给出）。
    AlignCell { cell: usize },
}

/// 从片段根到具体位置的路径。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MathFragmentPath {
    steps: Vec<MathFragmentPathStep>,
}

impl MathFragmentPath {
    fn root() -> Self {
        Self::default()
    }

    fn child(&self, step: MathFragmentPathStep) -> Self {
        let mut steps = self.steps.clone();
        steps.push(step);
        Self { steps }
    }

    /// 路径步骤（从外到内）。
    pub fn steps(&self) -> &[MathFragmentPathStep] {
        &self.steps
    }

    /// 是否指向片段根部。
    pub fn is_root(&self) -> bool {
        self.steps.is_empty()
    }
}

impl fmt::Display for MathFragmentPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.steps.is_empty() {
            return formatter.write_str("root");
        }
        formatter.write_str("root")?;
        for step in &self.steps {
            match step {
                MathFragmentPathStep::Content(index) => write!(formatter, ".content[{index}]")?,
                MathFragmentPathStep::Operand(index) => write!(formatter, ".operand[{index}]")?,
                MathFragmentPathStep::EnvironmentChild(index) => {
                    write!(formatter, ".environmentChild[{index}]")?;
                }
                MathFragmentPathStep::TextRun(index) => write!(formatter, ".textRun[{index}]")?,
                MathFragmentPathStep::AlignRow(row) => write!(formatter, ".row[{row}]")?,
                MathFragmentPathStep::AlignCell { cell } => {
                    write!(formatter, ".cell[{cell}]")?;
                }
            }
        }
        Ok(())
    }
}

/// 检查式序列化拒绝的具体损失类别。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathLossReason {
    /// 对象带有不透明扩展字段，规范 TeX 无法表达。
    ExtraField,
    /// 不透明内容（`Opaque` 节点）没有规范 TeX 表达。
    OpaqueContent,
    /// 文本 Run 的属性无法在规范 TeX 中表达。
    TextRunProperty,
    /// 文本 Run 的语言标注无法在规范 TeX 中表达。
    TextRunLanguage,
    /// 文本流没有 Run，序列化会整体删除该可编辑文本槽位。
    TextFlowWithoutRuns,
    /// 行内环境引用主题模板，片段转换没有主题解析上下文。
    EnvironmentTemplateReference,
    /// 行内环境带有无法表达的实例属性。
    EnvironmentProperty { key: String },
    /// 行内 Font 环境缺少可表达的字体族，无法保持继承语义。
    MissingFontFamily,
    /// 行内 Font 环境的字体族不在规范 TeX 命令可表达范围内。
    UnsupportedFontFamily { family: String },
    /// 空白/支柱尺寸超出规范 TeX 解析器精度预算或序列化算术溢出。
    UnrepresentableDimension,
}

impl fmt::Display for MathLossReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExtraField => formatter.write_str("opaque extension field has no TeX form"),
            Self::OpaqueContent => formatter.write_str("opaque content has no TeX form"),
            Self::TextRunProperty => {
                formatter.write_str("text run properties have no TeX form in a math fragment")
            }
            Self::TextRunLanguage => {
                formatter.write_str("text run language has no TeX form in a math fragment")
            }
            Self::TextFlowWithoutRuns => {
                formatter.write_str("text flow without runs has no TeX form and would be deleted")
            }
            Self::EnvironmentTemplateReference => formatter
                .write_str("inline environment template reference has no TeX form without a theme"),
            Self::EnvironmentProperty { key } => {
                write!(
                    formatter,
                    "inline environment property `{key}` has no TeX form"
                )
            }
            Self::MissingFontFamily => formatter
                .write_str("inline font environment has no fontFamily and no TeX fallback form"),
            Self::UnsupportedFontFamily { family } => write!(
                formatter,
                "inline font family `{family}` has no TeX form; only serif, sans-serif and monospace are expressible"
            ),
            Self::UnrepresentableDimension => formatter.write_str(
                "dimension exceeds the canonical TeX precision budget or overflows serialization arithmetic",
            ),
        }
    }
}

/// 一处无法无损表达的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathFragmentLoss {
    path: MathFragmentPath,
    reason: MathLossReason,
}

impl MathFragmentLoss {
    /// 模型内位置。
    pub fn path(&self) -> &MathFragmentPath {
        &self.path
    }

    /// 损失原因。
    pub fn reason(&self) -> &MathLossReason {
        &self.reason
    }
}

/// 数学片段解析或检查式序列化失败。
#[derive(Debug, Clone, PartialEq)]
pub enum MathFragmentError {
    /// 源码解析失败；保留解析器的结构化分类（语法、长度、深度、宿主类型）。
    Parse(MathParseError),
    /// 已有的规范模型变体与片段宿主种类不一致。
    KindMismatch { kind: MathInputKind },
    /// 模型本身不满足当前 schema（属性、形状、深度、ID 或版本）。
    Validation(ModelError),
    /// 模型合法，但规范 TeX 无法无损表达。
    Loss(MathFragmentLoss),
}

impl fmt::Display for MathFragmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "math fragment parse failed: {error}"),
            Self::KindMismatch { kind } => {
                write!(
                    formatter,
                    "math fragment model does not match {kind:?} host kind"
                )
            }
            Self::Validation(error) => write!(formatter, "math fragment model is invalid: {error}"),
            Self::Loss(loss) => {
                write!(formatter, "math fragment is not expressible: {}", loss.path)?;
                write!(formatter, ": {}", loss.reason)
            }
        }
    }
}

impl std::error::Error for MathFragmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Validation(error) => Some(error),
            Self::KindMismatch { .. } | Self::Loss(_) => None,
        }
    }
}

impl MathFragment {
    /// 解析一段数学片段并按宿主种类约束其规范模型。
    pub fn parse(source: &str, kind: MathInputKind) -> Result<Self, MathFragmentError> {
        let math = parse(source, kind).map_err(MathFragmentError::Parse)?;
        Self::from_math(kind, math)
    }

    /// 用已有的规范模型构造片段；模型变体必须与宿主种类一致。
    ///
    /// 期望模型来自解析器或编辑器维护的规范树；检查式序列化会验证 schema、
    /// 显式元数据和尺寸精度预算，但不承诺任意手工构造的 schema 合法模型都能
    /// 重新被当前解析器读取（例如自定义 `MathNode.text` 的组合不在其可表达子集内）。
    pub fn from_math(kind: MathInputKind, math: ParsedMath) -> Result<Self, MathFragmentError> {
        let consistent = match (&math, kind) {
            (ParsedMath::Tree(_), MathInputKind::Inline | MathInputKind::Equation) => true,
            (ParsedMath::Align(_), MathInputKind::Align) => true,
            (ParsedMath::Tree(_), MathInputKind::Align)
            | (ParsedMath::Align(_), MathInputKind::Inline | MathInputKind::Equation) => false,
        };
        if consistent {
            Ok(Self { kind, math })
        } else {
            Err(MathFragmentError::KindMismatch { kind })
        }
    }

    /// 片段宿主种类。
    pub fn kind(&self) -> MathInputKind {
        self.kind
    }

    /// 规范模型。
    pub fn math(&self) -> &ParsedMath {
        &self.math
    }

    /// 取出规范模型。
    pub fn into_math(self) -> ParsedMath {
        self.math
    }

    /// 生成规范 TeX 片段正文（不含外层 `$` / `\[` 定界符）。
    ///
    /// 外层定界符、源位置和格式诊断由各格式适配器负责。模型包含无法无损表达的
    /// 扩展、不透明内容或行内环境元数据时返回 [`MathFragmentError::Loss`]。
    pub fn to_tex(&self) -> Result<String, MathFragmentError> {
        match &self.math {
            ParsedMath::Tree(tree) => {
                check_tree(tree)?;
                Ok(serialize_tree(tree))
            }
            ParsedMath::Align(align) => {
                check_align(align)?;
                Ok(serialize_align(align))
            }
        }
    }
}

fn check_tree(tree: &MathTree) -> Result<(), MathFragmentError> {
    tree.validate().map_err(MathFragmentError::Validation)?;
    if !tree.extra.is_empty() {
        return Err(loss(MathFragmentPath::root(), MathLossReason::ExtraField));
    }
    check_flow(&tree.root, MathFragmentPath::root())
}

fn check_align(align: &Align) -> Result<(), MathFragmentError> {
    align.validate().map_err(MathFragmentError::Validation)?;
    if !align.extra.is_empty() {
        return Err(loss(MathFragmentPath::root(), MathLossReason::ExtraField));
    }
    for (row_index, row) in align.rows.iter().enumerate() {
        let row_path = MathFragmentPath::root().child(MathFragmentPathStep::AlignRow(row_index));
        if !row.extra.is_empty() {
            return Err(loss(row_path, MathLossReason::ExtraField));
        }
        for (cell_index, cell) in row.cells.iter().enumerate() {
            let cell_path = row_path.child(MathFragmentPathStep::AlignCell { cell: cell_index });
            if !cell.extra.is_empty() {
                return Err(loss(cell_path, MathLossReason::ExtraField));
            }
            check_flow(&cell.root, cell_path)?;
        }
    }
    Ok(())
}

fn check_flow(flow: &MathFlow, path: MathFragmentPath) -> Result<(), MathFragmentError> {
    if !flow.extra.is_empty() {
        return Err(loss(path, MathLossReason::ExtraField));
    }
    for (index, content) in flow.content.iter().enumerate() {
        let content_path = path.child(MathFragmentPathStep::Content(index));
        check_content(content, content_path)?;
    }
    Ok(())
}

fn check_content(content: &MathContent, path: MathFragmentPath) -> Result<(), MathFragmentError> {
    match content {
        MathContent::Math(node) => check_node(node, path),
        MathContent::Text(text) => check_text_flow(text, path),
        MathContent::Environment(environment) => check_environment(environment, path),
        MathContent::SoftBreak(value) => {
            if value.extra.is_empty() {
                Ok(())
            } else {
                Err(loss(path, MathLossReason::ExtraField))
            }
        }
        MathContent::Opaque(_) => Err(loss(path, MathLossReason::OpaqueContent)),
    }
}

fn check_node(node: &MathNode, path: MathFragmentPath) -> Result<(), MathFragmentError> {
    if !node.extra.is_empty() {
        return Err(loss(path, MathLossReason::ExtraField));
    }
    check_node_dimensions(node, &path)?;
    for (index, operand) in node.children.iter().enumerate() {
        let operand_path = path.child(MathFragmentPathStep::Operand(index));
        check_flow(operand, operand_path)?;
    }
    Ok(())
}

/// 规范序列化会重新格式化 em 长度；超出解析器 18 位精度预算的合法模型不能
/// 产生可再读取的 TeX，必须在检查阶段返回损失而不是写出无效输出或算术溢出。
fn check_node_dimensions(
    node: &MathNode,
    path: &MathFragmentPath,
) -> Result<(), MathFragmentError> {
    match node.kind {
        MathNodeKind::Space
            if node.attributes.get("spaceKind").map(String::as_str) == Some("hspace") =>
        {
            let numerator = dimension_attribute(node, "valueNumerator", path)?;
            let denominator = dimension_attribute(node, "valueDenominator", path)?;
            if dimension_is_expressible(numerator, denominator) {
                Ok(())
            } else {
                Err(loss(path.clone(), MathLossReason::UnrepresentableDimension))
            }
        }
        MathNodeKind::Strut => {
            let height = (
                dimension_attribute(node, "heightNumerator", path)?,
                dimension_attribute(node, "heightDenominator", path)?,
            );
            let depth = (
                dimension_attribute(node, "depthNumerator", path)?,
                dimension_attribute(node, "depthDenominator", path)?,
            );
            let total = height
                .0
                .checked_mul(depth.1)
                .and_then(|height_times_depth| {
                    depth
                        .0
                        .checked_mul(height.1)
                        .and_then(|depth_times_height| {
                            height_times_depth.checked_add(depth_times_height)
                        })
                });
            let denominator = height.1.checked_mul(depth.1);
            let negated_depth = depth.0.checked_neg();
            let expressible = match (total, denominator, negated_depth) {
                (Some(total), Some(denominator), Some(negated_depth)) => {
                    dimension_is_expressible(total, denominator)
                        && dimension_is_expressible(negated_depth, depth.1)
                }
                _ => false,
            };
            if expressible {
                Ok(())
            } else {
                Err(loss(path.clone(), MathLossReason::UnrepresentableDimension))
            }
        }
        _ => Ok(()),
    }
}

fn dimension_attribute(
    node: &MathNode,
    key: &str,
    path: &MathFragmentPath,
) -> Result<i128, MathFragmentError> {
    node.attributes
        .get(key)
        .and_then(|value| value.parse::<i128>().ok())
        .ok_or_else(|| loss(path.clone(), MathLossReason::UnrepresentableDimension))
}

fn dimension_is_expressible(numerator: i128, denominator: i128) -> bool {
    crate::dimension::format_dimension_checked((numerator, denominator)).is_some()
}

fn check_text_flow(
    text: &caret_doc::TextFlow,
    path: MathFragmentPath,
) -> Result<(), MathFragmentError> {
    if !text.extra.is_empty() {
        return Err(loss(path, MathLossReason::ExtraField));
    }
    if text.runs.is_empty() {
        return Err(loss(path, MathLossReason::TextFlowWithoutRuns));
    }
    for (index, run) in text.runs.iter().enumerate() {
        let run_path = path.child(MathFragmentPathStep::TextRun(index));
        if !run.props.is_empty() {
            return Err(loss(run_path, MathLossReason::TextRunProperty));
        }
        if run.language.is_some() {
            return Err(loss(run_path, MathLossReason::TextRunLanguage));
        }
        if !run.extra.is_empty() {
            return Err(loss(run_path, MathLossReason::ExtraField));
        }
    }
    Ok(())
}

fn check_environment(
    environment: &caret_doc::InlineEnvironment<MathContent>,
    path: MathFragmentPath,
) -> Result<(), MathFragmentError> {
    if !environment.extra.is_empty() {
        return Err(loss(path, MathLossReason::ExtraField));
    }
    if environment.definition_id.is_some() {
        return Err(loss(path, MathLossReason::EnvironmentTemplateReference));
    }
    match environment.kind {
        InlineEnvironmentKind::Bold | InlineEnvironmentKind::Italic => {
            for (key, value) in &environment.props {
                if key != "enabled" || !value.is_boolean() {
                    return Err(loss(
                        path,
                        MathLossReason::EnvironmentProperty { key: key.clone() },
                    ));
                }
            }
        }
        InlineEnvironmentKind::Font => {
            let Some(font_family) = environment.props.get("fontFamily") else {
                return Err(loss(path, MathLossReason::MissingFontFamily));
            };
            if !font_family.is_string() {
                return Err(loss(
                    path,
                    MathLossReason::EnvironmentProperty {
                        key: "fontFamily".to_owned(),
                    },
                ));
            }
            for (key, value) in &environment.props {
                if key != "fontFamily" {
                    return Err(loss(
                        path,
                        MathLossReason::EnvironmentProperty { key: key.clone() },
                    ));
                }
                let Some(family) = value.as_str() else {
                    return Err(loss(
                        path.clone(),
                        MathLossReason::EnvironmentProperty { key: key.clone() },
                    ));
                };
                if !matches!(family, "serif" | "sans-serif" | "monospace") {
                    return Err(loss(
                        path.clone(),
                        MathLossReason::UnsupportedFontFamily {
                            family: family.to_owned(),
                        },
                    ));
                }
            }
        }
    }
    for (index, child) in environment.children.iter().enumerate() {
        let child_path = path.child(MathFragmentPathStep::EnvironmentChild(index));
        check_content(child, child_path)?;
    }
    Ok(())
}

fn loss(path: MathFragmentPath, reason: MathLossReason) -> MathFragmentError {
    MathFragmentError::Loss(MathFragmentLoss { path, reason })
}
