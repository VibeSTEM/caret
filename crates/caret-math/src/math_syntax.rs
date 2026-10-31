//! 将 latex-rust AST 转成 Caret parser 边界自有、非持久化语法树。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::MathInputKind;
use latex_rust::{
    AccentKind, AtomKind, ColSpec, DelimSize, Delimiter, Dim, EnvRow, EqNumber, FracStyle,
    IntegralKind, MathNode, MatrixStyle, PhantomKind, SpaceKind, TextItem, TextRun, TextStyle,
};
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

/// 后端无关的、一次解析结果。它不含 Caret 文档 ID 或持久化源码跨度。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathSyntaxTree {
    /// 调用方要求的公式形态。
    pub input_kind: MathInputKind,
    /// 公式的根语法节点。
    pub root: MathSyntaxNode,
}

/// Parser 适配层的临时语法节点值；这不是 `.cdoc` schema。
///
/// `children` 顺序约定：Fraction=[numerator, denominator]；Radical=[index, body]；
/// Superscript/Subscript=[base, script]；SubSup=[base, sub, sup]；Delimited=[body]；
/// Matrix=[按顺序排列的 Row/Intertext/Hline]，每个 Row 的 children 是单元格；
/// Sum/Product/Integral=[lower, upper]；Limit=[lower]；OverUnder=[base, over, under]；
/// Accent/Intertext/Color/TextColor/ColorBox/Phantom=[body]；CancelTo=[value, body]；
/// FColorBox=[body]。Radical、算子上下限、OverUnder 的可选槽位使用 `Placeholder`，
/// 表示 parser AST 中缺少该参数；导入后映射为 Caret 可编辑树的空参数槽位。
///
/// 节点字段约定：Atom 生成的 Symbol 携带 atomClass；命名 Symbol 携带命令名；Operator 携带
/// limits。Fraction.style 为 frac/dfrac/tfrac/cfrac；Delimited.left/right 与
/// SizedDelimiter.text 使用 empty、char:<字符>、named:<名称> 编码，SizedDelimiter.size 为
/// big/Big/bigg/Bigg。Matrix.style 为 latex-rust 环境名，columns 为 l/c/r/| 字符序列；
/// Integral.integralKind 为 int/iint/iiint/oint/oiint；Accent.accent 为重音名；Text 与
/// TextStyle.style 为文本样式名。TextSpace.text 为 space/control_space/no_break_space；
/// TextItem::Math 直接作为对应数学节点出现在 Text.children 中。Space.spaceKind 为间距名；
/// Hspace 的精确尺寸键为 valueNumerator/valueDenominator/valueUnit，Strut 对应
/// heightNumerator/heightDenominator/heightUnit 与 depthNumerator/depthDenominator/depthUnit，
/// 单位固定为 em。颜色属性使用 CSS hex；Phantom.phantomKind 为命令名。
/// 解析入口将深度限制为 32 层、节点限制为 65,536；该树仍须转换后才能进入持久化模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathSyntaxNode {
    /// 语法类别。
    pub kind: MathSyntaxKind,
    /// 原子文字或 parser 符号名；结构节点通常为空。
    pub text: Option<String>,
    /// kind 专属的值属性；键由 `kind` 决定，值保留 TeX parser 语义。
    pub attributes: BTreeMap<String, String>,
    /// 依语法位置排序的子节点；可选槽位由 `Placeholder` 明确占位。
    pub children: Vec<Self>,
}

impl MathSyntaxNode {
    fn new(kind: MathSyntaxKind) -> Self {
        Self {
            kind,
            text: None,
            attributes: BTreeMap::new(),
            children: Vec::new(),
        }
    }
}

/// `caret-math` 中间语法树节点类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathSyntaxKind {
    /// 有序数学项集合。
    Row,
    /// 单个数学符号或原子。
    Symbol,
    /// 命名数学运算符。
    Operator,
    /// 分数。
    Fraction,
    /// 根式。
    Radical,
    /// 上标。
    Superscript,
    /// 下标。
    Subscript,
    /// 同时有上下标。
    SubSup,
    /// 成对定界符。
    Delimited,
    /// 带有 TeX 尺寸的定界符。
    SizedDelimiter,
    /// 矩阵或对齐环境。
    Matrix,
    /// 堆叠式下标。
    Substack,
    /// 多行环境中的 `\intertext`。
    Intertext,
    /// 多行环境中的横线。
    Hline,
    /// 求和算子。
    Sum,
    /// 积分算子。
    Integral,
    /// 连乘算子。
    Product,
    /// 极限算子。
    Limit,
    /// 上方/下方标注。
    OverUnder,
    /// 重音或装饰。
    Accent,
    /// 指向某值的取消线。
    CancelTo,
    /// 文本模式 run。
    Text,
    /// 文本模式普通字符。
    TextContent,
    /// 文本模式空格。
    TextSpace,
    /// 文本模式符号。
    TextSymbol,
    /// 文本模式嵌套样式。
    TextStyle,
    /// 显式数学间距。
    Space,
    /// 数学颜色样式。
    Color,
    /// 文本颜色样式。
    TextColor,
    /// 颜色背景盒。
    ColorBox,
    /// 有边框和填充的颜色盒。
    FColorBox,
    /// 数学垂直支撑。
    Strut,
    /// 幻影内容。
    Phantom,
    /// 无对应输入的结构槽位。
    Placeholder,
}

/// 解析器语法无法无损映射到当前中间树时返回的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathSyntaxConversionError {
    /// 未支持的语法或取值名称。
    pub construct: String,
}

impl Display for MathSyntaxConversionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "math.syntax_conversion_unsupported: {}",
            self.construct
        )
    }
}

impl std::error::Error for MathSyntaxConversionError {}

pub(super) fn convert(
    input_kind: MathInputKind,
    ast: &MathNode,
) -> Result<MathSyntaxTree, MathSyntaxConversionError> {
    Ok(MathSyntaxTree {
        input_kind,
        root: convert_node(ast)?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AstBudgetError {
    TooDeep,
    TooManyNodes,
    UnsupportedNode,
}

enum BudgetEntry<'a> {
    Node(&'a MathNode, usize),
    TextRun(&'a TextRun, usize),
}

pub(super) fn check_ast_budget(
    root: &MathNode,
    depth_limit: usize,
    node_limit: usize,
) -> Result<(), AstBudgetError> {
    let mut node_count = 0;
    let mut pending = vec![BudgetEntry::Node(root, 1)];
    while let Some(entry) = pending.pop() {
        match entry {
            BudgetEntry::Node(node, depth) => {
                record_budget_node(&mut node_count, depth, depth_limit, node_limit)?;
                let child_depth = depth + 1;
                match node {
                    MathNode::Fraction(_, a, b)
                    | MathNode::Superscript(a, b)
                    | MathNode::Subscript(a, b)
                    | MathNode::CancelTo(a, b) => {
                        pending.push(BudgetEntry::Node(a, child_depth));
                        pending.push(BudgetEntry::Node(b, child_depth));
                    }
                    MathNode::Radical(index, body) => {
                        if let Some(index) = index {
                            pending.push(BudgetEntry::Node(index, child_depth));
                        }
                        if index.is_none() {
                            record_budget_node(
                                &mut node_count,
                                child_depth,
                                depth_limit,
                                node_limit,
                            )?;
                        }
                        pending.push(BudgetEntry::Node(body, child_depth));
                    }
                    MathNode::SubSup(base, sub, sup) => {
                        pending.push(BudgetEntry::Node(base, child_depth));
                        pending.push(BudgetEntry::Node(sub, child_depth));
                        pending.push(BudgetEntry::Node(sup, child_depth));
                    }
                    MathNode::Delimited(_, body, _) | MathNode::Intertext(body) => {
                        pending.push(BudgetEntry::Node(body, child_depth));
                    }
                    MathNode::Row(items) | MathNode::Substack(items) => {
                        reserve_pending_budget(pending.len(), items.len(), node_limit)?;
                        pending.extend(
                            items
                                .iter()
                                .map(|child| BudgetEntry::Node(child, child_depth)),
                        );
                    }
                    MathNode::Matrix(_, _, rows) => {
                        for row in rows {
                            match row {
                                EnvRow::Cells { cells, .. } => {
                                    record_budget_node(
                                        &mut node_count,
                                        child_depth,
                                        depth_limit,
                                        node_limit,
                                    )?;
                                    reserve_pending_budget(pending.len(), cells.len(), node_limit)?;
                                    pending.extend(
                                        cells
                                            .iter()
                                            .map(|child| BudgetEntry::Node(child, child_depth + 1)),
                                    );
                                }
                                EnvRow::Intertext(body) => {
                                    record_budget_node(
                                        &mut node_count,
                                        child_depth,
                                        depth_limit,
                                        node_limit,
                                    )?;
                                    reserve_pending_budget(pending.len(), 1, node_limit)?;
                                    pending.push(BudgetEntry::Node(body, child_depth + 1));
                                }
                                EnvRow::Hline => {
                                    record_budget_node(
                                        &mut node_count,
                                        child_depth,
                                        depth_limit,
                                        node_limit,
                                    )?;
                                }
                            }
                        }
                    }
                    MathNode::Sum(lower, upper) | MathNode::Product(lower, upper) => {
                        push_optional_depth_nodes(
                            &mut pending,
                            &mut node_count,
                            [lower.as_deref(), upper.as_deref()],
                            child_depth,
                            depth_limit,
                            node_limit,
                        )?;
                    }
                    MathNode::Integral(_, lower, upper) => {
                        push_optional_depth_nodes(
                            &mut pending,
                            &mut node_count,
                            [lower.as_deref(), upper.as_deref()],
                            child_depth,
                            depth_limit,
                            node_limit,
                        )?;
                    }
                    MathNode::Limit(lower) => push_optional_depth_nodes(
                        &mut pending,
                        &mut node_count,
                        [lower.as_deref()],
                        child_depth,
                        depth_limit,
                        node_limit,
                    )?,
                    MathNode::OverUnder(base, over, under) => {
                        pending.push(BudgetEntry::Node(base, child_depth));
                        push_optional_depth_nodes(
                            &mut pending,
                            &mut node_count,
                            [over.as_deref(), under.as_deref()],
                            child_depth,
                            depth_limit,
                            node_limit,
                        )?;
                    }
                    MathNode::Accent(body, _)
                    | MathNode::Color(_, body)
                    | MathNode::TextColor(_, body)
                    | MathNode::ColorBox(_, body)
                    | MathNode::FColorBox(_, _, body)
                    | MathNode::Phantom(_, body) => {
                        pending.push(BudgetEntry::Node(body, child_depth));
                    }
                    MathNode::Tag { body, .. } => {
                        pending.push(BudgetEntry::Node(body, child_depth));
                    }
                    MathNode::Text(run, _) => {
                        pending.push(BudgetEntry::TextRun(run, child_depth));
                    }
                    MathNode::Atom(_, _)
                    | MathNode::SizedDelim(_, _, _)
                    | MathNode::Ref(_)
                    | MathNode::Label(_)
                    | MathNode::NoNumber
                    | MathNode::Hline
                    | MathNode::Strut(_, _)
                    | MathNode::Space(_)
                    | MathNode::Operator(_, _)
                    | MathNode::Symbol(_) => {}
                    _ => return Err(AstBudgetError::UnsupportedNode),
                }
            }
            BudgetEntry::TextRun(run, depth) => {
                for item in run.items() {
                    match item {
                        TextItem::Nested(_, nested) => {
                            record_budget_node(&mut node_count, depth, depth_limit, node_limit)?;
                            reserve_pending_budget(pending.len(), 1, node_limit)?;
                            pending.push(BudgetEntry::TextRun(nested, depth + 1));
                        }
                        TextItem::Math(math) => {
                            reserve_pending_budget(pending.len(), 1, node_limit)?;
                            pending.push(BudgetEntry::Node(math, depth));
                        }
                        TextItem::Str(_)
                        | TextItem::Space
                        | TextItem::ControlSpace
                        | TextItem::NoBreakSpace
                        | TextItem::Symbol(_) => {
                            record_budget_node(&mut node_count, depth, depth_limit, node_limit)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn push_optional_depth_nodes<'a, const N: usize>(
    pending: &mut Vec<BudgetEntry<'a>>,
    node_count: &mut usize,
    children: [Option<&'a MathNode>; N],
    depth: usize,
    depth_limit: usize,
    node_limit: usize,
) -> Result<(), AstBudgetError> {
    for child in children {
        match child {
            Some(child) => pending.push(BudgetEntry::Node(child, depth)),
            None => record_budget_node(node_count, depth, depth_limit, node_limit)?,
        }
    }
    Ok(())
}

fn record_budget_node(
    node_count: &mut usize,
    depth: usize,
    depth_limit: usize,
    node_limit: usize,
) -> Result<(), AstBudgetError> {
    if depth > depth_limit {
        return Err(AstBudgetError::TooDeep);
    }
    *node_count += 1;
    if *node_count > node_limit {
        return Err(AstBudgetError::TooManyNodes);
    }
    Ok(())
}

fn reserve_pending_budget(
    pending_len: usize,
    additional: usize,
    node_limit: usize,
) -> Result<(), AstBudgetError> {
    if pending_len.saturating_add(additional) > node_limit {
        Err(AstBudgetError::TooManyNodes)
    } else {
        Ok(())
    }
}

fn convert_node(node: &MathNode) -> Result<MathSyntaxNode, MathSyntaxConversionError> {
    let output = match node {
        MathNode::Atom(character, class) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Symbol);
            output.text = Some(character.to_string());
            output
                .attributes
                .insert("atomClass".to_owned(), atom_class_name(*class).to_owned());
            output
        }
        MathNode::Symbol(name) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Symbol);
            output.text = Some(name.clone());
            output
        }
        MathNode::Operator(name, limits) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Operator);
            output.text = Some(name.clone());
            output
                .attributes
                .insert("limits".to_owned(), limits.to_string());
            output
        }
        MathNode::Fraction(style, numerator, denominator) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Fraction);
            output
                .attributes
                .insert("style".to_owned(), frac_style_name(*style)?.to_owned());
            output.children = vec![convert_node(numerator)?, convert_node(denominator)?];
            output
        }
        MathNode::Radical(index, body) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Radical);
            output.children = vec![
                convert_optional_node(index.as_deref())?,
                convert_node(body)?,
            ];
            output
        }
        MathNode::Superscript(base, sup) => binary_node(
            MathSyntaxKind::Superscript,
            convert_node(base)?,
            convert_node(sup)?,
        ),
        MathNode::Subscript(base, sub) => binary_node(
            MathSyntaxKind::Subscript,
            convert_node(base)?,
            convert_node(sub)?,
        ),
        MathNode::SubSup(base, sub, sup) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::SubSup);
            output.children = vec![convert_node(base)?, convert_node(sub)?, convert_node(sup)?];
            output
        }
        MathNode::Delimited(left, body, right) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Delimited);
            output
                .attributes
                .insert("left".to_owned(), delimiter_name(left));
            output
                .attributes
                .insert("right".to_owned(), delimiter_name(right));
            output.children.push(convert_node(body)?);
            output
        }
        MathNode::SizedDelim(delimiter, size, class) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::SizedDelimiter);
            output.text = Some(delimiter_name(delimiter));
            output
                .attributes
                .insert("size".to_owned(), delim_size_name(*size).to_owned());
            output
                .attributes
                .insert("atomClass".to_owned(), atom_class_name(*class).to_owned());
            output
        }
        MathNode::Row(items) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Row);
            output.children = items.iter().map(convert_node).collect::<Result<_, _>>()?;
            output
        }
        MathNode::Matrix(style, columns, rows) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Matrix);
            output
                .attributes
                .insert("style".to_owned(), matrix_style_name(*style).to_owned());
            output.attributes.insert(
                "columns".to_owned(),
                columns.iter().map(col_spec_name).collect::<String>(),
            );
            for row in rows {
                match row {
                    EnvRow::Cells {
                        cells,
                        number: EqNumber::Default,
                        labels,
                    } if labels.is_empty() => {
                        let mut row_node = MathSyntaxNode::new(MathSyntaxKind::Row);
                        row_node.children =
                            cells.iter().map(convert_node).collect::<Result<_, _>>()?;
                        output.children.push(row_node);
                    }
                    EnvRow::Cells { .. } => {
                        return Err(unsupported_conversion("numbered or labeled matrix row"));
                    }
                    EnvRow::Hline => output
                        .children
                        .push(MathSyntaxNode::new(MathSyntaxKind::Hline)),
                    EnvRow::Intertext(body) => {
                        let mut intertext = MathSyntaxNode::new(MathSyntaxKind::Intertext);
                        intertext.children.push(convert_node(body)?);
                        output.children.push(intertext);
                    }
                }
            }
            output
        }
        MathNode::Substack(items) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Substack);
            output.children = items.iter().map(convert_node).collect::<Result<_, _>>()?;
            output
        }
        MathNode::Ref(_) | MathNode::Tag { .. } | MathNode::Label(_) | MathNode::NoNumber => {
            return Err(unsupported_conversion(
                "numbering or cross-reference syntax",
            ));
        }
        MathNode::Hline => MathSyntaxNode::new(MathSyntaxKind::Hline),
        MathNode::Intertext(body) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Intertext);
            output.children.push(convert_node(body)?);
            output
        }
        MathNode::Sum(lower, upper) => {
            optional_operator_node(MathSyntaxKind::Sum, lower.as_deref(), upper.as_deref())?
        }
        MathNode::Integral(kind, lower, upper) => {
            let mut output = optional_operator_node(
                MathSyntaxKind::Integral,
                lower.as_deref(),
                upper.as_deref(),
            )?;
            output.attributes.insert(
                "integralKind".to_owned(),
                integral_kind_name(*kind).to_owned(),
            );
            output
        }
        MathNode::Product(lower, upper) => {
            optional_operator_node(MathSyntaxKind::Product, lower.as_deref(), upper.as_deref())?
        }
        MathNode::Limit(lower) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Limit);
            output
                .children
                .push(convert_optional_node(lower.as_deref())?);
            output
        }
        MathNode::OverUnder(base, over, under) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::OverUnder);
            output.children = vec![
                convert_node(base)?,
                convert_optional_node(over.as_deref())?,
                convert_optional_node(under.as_deref())?,
            ];
            output
        }
        MathNode::Accent(body, kind) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Accent);
            output
                .attributes
                .insert("accent".to_owned(), accent_kind_name(*kind).to_owned());
            output.children.push(convert_node(body)?);
            output
        }
        MathNode::CancelTo(value, body) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::CancelTo);
            output.children = vec![convert_node(value)?, convert_node(body)?];
            output
        }
        MathNode::Text(run, style) => convert_text_run(run, *style)?,
        MathNode::Space(kind) => convert_space(kind)?,
        MathNode::Color(color, body) => {
            styled_node(MathSyntaxKind::Color, "color", color.css_hex(), body)?
        }
        MathNode::TextColor(color, body) => {
            styled_node(MathSyntaxKind::TextColor, "color", color.css_hex(), body)?
        }
        MathNode::ColorBox(color, body) => {
            styled_node(MathSyntaxKind::ColorBox, "color", color.css_hex(), body)?
        }
        MathNode::FColorBox(border, fill, body) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::FColorBox);
            output
                .attributes
                .insert("borderColor".to_owned(), border.css_hex());
            output
                .attributes
                .insert("fillColor".to_owned(), fill.css_hex());
            output.children.push(convert_node(body)?);
            output
        }
        MathNode::Strut(height, depth) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Strut);
            insert_exact_dimension(&mut output.attributes, "height", height)?;
            insert_exact_dimension(&mut output.attributes, "depth", depth)?;
            output
        }
        MathNode::Phantom(kind, body) => {
            let mut output = MathSyntaxNode::new(MathSyntaxKind::Phantom);
            output.attributes.insert(
                "phantomKind".to_owned(),
                phantom_kind_name(*kind).to_owned(),
            );
            output.children.push(convert_node(body)?);
            output
        }
        _ => return Err(unsupported_conversion("future latex-rust math node")),
    };
    Ok(output)
}

fn convert_text_run(
    run: &TextRun,
    style: TextStyle,
) -> Result<MathSyntaxNode, MathSyntaxConversionError> {
    let mut output = MathSyntaxNode::new(MathSyntaxKind::Text);
    output
        .attributes
        .insert("style".to_owned(), text_style_name(style).to_owned());
    for item in run.items() {
        output.children.push(match item {
            TextItem::Str(text) => {
                let mut node = MathSyntaxNode::new(MathSyntaxKind::TextContent);
                node.text = Some(text.clone());
                node
            }
            TextItem::Space | TextItem::ControlSpace | TextItem::NoBreakSpace => {
                let mut node = MathSyntaxNode::new(MathSyntaxKind::TextSpace);
                node.text = Some(
                    match item {
                        TextItem::Space => "space",
                        TextItem::ControlSpace => "control_space",
                        TextItem::NoBreakSpace => "no_break_space",
                        _ => unreachable!(),
                    }
                    .to_owned(),
                );
                node
            }
            TextItem::Symbol(name) => {
                let mut node = MathSyntaxNode::new(MathSyntaxKind::TextSymbol);
                node.text = Some((*name).to_owned());
                node
            }
            TextItem::Nested(style, nested) => {
                let mut node = MathSyntaxNode::new(MathSyntaxKind::TextStyle);
                node.attributes
                    .insert("style".to_owned(), text_style_name(*style).to_owned());
                let children = convert_text_run(nested, TextStyle::Text)?;
                node.children = children.children;
                node
            }
            TextItem::Math(math) => convert_node(math)?,
        });
    }
    Ok(output)
}

fn convert_space(kind: &SpaceKind) -> Result<MathSyntaxNode, MathSyntaxConversionError> {
    let mut output = MathSyntaxNode::new(MathSyntaxKind::Space);
    let (kind, value) = match kind {
        SpaceKind::Thin => ("thin", None),
        SpaceKind::Medium => ("medium", None),
        SpaceKind::Thick => ("thick", None),
        SpaceKind::NegThin => ("negative_thin", None),
        SpaceKind::Quad => ("quad", None),
        SpaceKind::Qquad => ("qquad", None),
        SpaceKind::ControlSpace => ("control", None),
        SpaceKind::Hspace(dim) => {
            output
                .attributes
                .insert("spaceKind".to_owned(), "hspace".to_owned());
            insert_exact_dimension(&mut output.attributes, "value", dim)?;
            return Ok(output);
        }
    };
    output
        .attributes
        .insert("spaceKind".to_owned(), kind.to_owned());
    if let Some(value) = value {
        output.attributes.insert("value".to_owned(), value);
    }
    Ok(output)
}

fn insert_exact_dimension(
    attributes: &mut BTreeMap<String, String>,
    name: &str,
    dimension: &Dim,
) -> Result<(), MathSyntaxConversionError> {
    let Some((numerator, denominator)) = dimension.as_ratio() else {
        return Err(unsupported_conversion("NaN math dimension"));
    };
    attributes.insert(format!("{name}Numerator"), numerator.to_string());
    attributes.insert(format!("{name}Denominator"), denominator.to_string());
    attributes.insert(format!("{name}Unit"), "em".to_owned());
    Ok(())
}

fn optional_operator_node(
    kind: MathSyntaxKind,
    lower: Option<&MathNode>,
    upper: Option<&MathNode>,
) -> Result<MathSyntaxNode, MathSyntaxConversionError> {
    let mut output = MathSyntaxNode::new(kind);
    output.children = vec![convert_optional_node(lower)?, convert_optional_node(upper)?];
    Ok(output)
}

fn convert_optional_node(
    node: Option<&MathNode>,
) -> Result<MathSyntaxNode, MathSyntaxConversionError> {
    match node {
        Some(node) => convert_node(node),
        None => Ok(MathSyntaxNode::new(MathSyntaxKind::Placeholder)),
    }
}

fn binary_node(
    kind: MathSyntaxKind,
    first: MathSyntaxNode,
    second: MathSyntaxNode,
) -> MathSyntaxNode {
    MathSyntaxNode {
        kind,
        text: None,
        attributes: BTreeMap::new(),
        children: vec![first, second],
    }
}

fn styled_node(
    kind: MathSyntaxKind,
    key: &str,
    value: String,
    body: &MathNode,
) -> Result<MathSyntaxNode, MathSyntaxConversionError> {
    let mut output = MathSyntaxNode::new(kind);
    output.attributes.insert(key.to_owned(), value);
    output.children.push(convert_node(body)?);
    Ok(output)
}

fn unsupported_conversion(construct: &str) -> MathSyntaxConversionError {
    MathSyntaxConversionError {
        construct: construct.to_owned(),
    }
}

fn atom_class_name(class: AtomKind) -> &'static str {
    match class {
        AtomKind::Ord => "ord",
        AtomKind::Op => "op",
        AtomKind::Bin => "bin",
        AtomKind::Rel => "rel",
        AtomKind::Open => "open",
        AtomKind::Close => "close",
        AtomKind::Punct => "punct",
        AtomKind::Inner => "inner",
    }
}

fn frac_style_name(style: FracStyle) -> Result<&'static str, MathSyntaxConversionError> {
    match style {
        FracStyle::Frac => Ok("frac"),
        FracStyle::Dfrac => Ok("dfrac"),
        FracStyle::Tfrac => Ok("tfrac"),
        FracStyle::Cfrac => Ok("cfrac"),
        _ => Err(unsupported_conversion("future fraction style")),
    }
}

pub(super) fn matrix_style_name(style: MatrixStyle) -> &'static str {
    match style {
        MatrixStyle::Align => "align",
        MatrixStyle::Equation => "equation",
        MatrixStyle::Gather => "gather",
        MatrixStyle::Multline => "multline",
        MatrixStyle::Matrix => "matrix",
        MatrixStyle::Pmatrix => "pmatrix",
        MatrixStyle::Bmatrix => "bmatrix",
        MatrixStyle::Vmatrix => "vmatrix",
        MatrixStyle::VVmatrix => "Vmatrix",
        MatrixStyle::BBmatrix => "Bmatrix",
        MatrixStyle::Cases => "cases",
        MatrixStyle::Array => "array",
        MatrixStyle::Aligned => "aligned",
        MatrixStyle::Split => "split",
    }
}

fn col_spec_name(spec: &ColSpec) -> char {
    match spec {
        ColSpec::Left => 'l',
        ColSpec::Center => 'c',
        ColSpec::Right => 'r',
        ColSpec::VRule => '|',
    }
}

fn delimiter_name(delimiter: &Delimiter) -> String {
    match delimiter {
        Delimiter::Empty => "empty".to_owned(),
        Delimiter::Char(value) => format!("char:{value}"),
        Delimiter::Named(value) => format!("named:{value}"),
    }
}

fn delim_size_name(size: DelimSize) -> &'static str {
    match size {
        DelimSize::Big => "big",
        DelimSize::Big2 => "Big",
        DelimSize::Bigg => "bigg",
        DelimSize::Bigg2 => "Bigg",
    }
}

fn integral_kind_name(kind: IntegralKind) -> &'static str {
    match kind {
        IntegralKind::Int => "int",
        IntegralKind::Iint => "iint",
        IntegralKind::Iiint => "iiint",
        IntegralKind::Oint => "oint",
        IntegralKind::Oiint => "oiint",
    }
}

fn phantom_kind_name(kind: PhantomKind) -> &'static str {
    match kind {
        PhantomKind::Full => "phantom",
        PhantomKind::Vertical => "vphantom",
        PhantomKind::Horizontal => "hphantom",
    }
}

fn text_style_name(style: TextStyle) -> &'static str {
    match style {
        TextStyle::Rm => "rm",
        TextStyle::Bf => "bf",
        TextStyle::It => "it",
        TextStyle::Sf => "sf",
        TextStyle::Tt => "tt",
        TextStyle::Bb => "bb",
        TextStyle::Cal => "cal",
        TextStyle::Frak => "frak",
        TextStyle::Scr => "scr",
        TextStyle::Boldsymbol => "boldsymbol",
        TextStyle::Pmb => "pmb",
        TextStyle::Text => "text",
    }
}

fn accent_kind_name(kind: AccentKind) -> &'static str {
    match kind {
        AccentKind::Hat => "hat",
        AccentKind::Check => "check",
        AccentKind::Breve => "breve",
        AccentKind::Acute => "acute",
        AccentKind::Grave => "grave",
        AccentKind::Tilde => "tilde",
        AccentKind::Bar => "bar",
        AccentKind::Vec => "vec",
        AccentKind::Dot => "dot",
        AccentKind::Ddot => "ddot",
        AccentKind::Dddot => "dddot",
        AccentKind::Ddddot => "ddddot",
        AccentKind::WideHat => "widehat",
        AccentKind::WideTilde => "widetilde",
        AccentKind::Overline => "overline",
        AccentKind::Underline => "underline",
        AccentKind::Overbrace => "overbrace",
        AccentKind::Underbrace => "underbrace",
        AccentKind::Overleftarrow => "overleftarrow",
        AccentKind::Overrightarrow => "overrightarrow",
        AccentKind::Overleftrightarrow => "overleftrightarrow",
        AccentKind::Underleftarrow => "underleftarrow",
        AccentKind::Underrightarrow => "underrightarrow",
        AccentKind::Underleftrightarrow => "underleftrightarrow",
        AccentKind::Cancel => "cancel",
        AccentKind::BCancel => "bcancel",
        AccentKind::XCancel => "xcancel",
        AccentKind::Boxed => "boxed",
        AccentKind::Ring => "ring",
        AccentKind::Not => "not",
    }
}
