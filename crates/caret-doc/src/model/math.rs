//! 定义与解析后端无关的持久化数学树。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::NodeId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 当前 Caret 数学树 schema 版本。
pub const MATH_TREE_SCHEMA_VERSION: u16 = 3;
/// 数学树允许的最大根到叶深度。
pub const MAX_MATH_TREE_DEPTH: usize = 32;
/// 单棵公式树允许的最大节点数。
pub const MAX_MATH_TREE_NODES: usize = 65_536;
/// 单棵公式树的可变长度字符串及扩展值预算。
pub const MAX_MATH_TREE_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

/// Caret 自有、版本化且不依赖解析器 AST 的数学表达式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MathTree {
    pub schema_version: u16,
    pub root: MathFlow,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 有序数学内容流；数学树根与节点的每个语义槽位都使用此类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MathFlow {
    pub id: NodeId,
    pub content: Vec<super::MathContent>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 数学树内部的结构路径步骤；不包含源码偏移或宿主会话状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathPathStep {
    /// 当前 MathFlow 的内容项。
    Content(usize),
    /// 当前行内环境的子项。
    EnvironmentChild(usize),
    /// 当前数学节点的操作数 MathFlow。
    Operand(usize),
}

/// 有序数学语义节点。`text` 保存节点自身的原子内容，`children` 保存操作数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MathNode {
    pub id: NodeId,
    pub kind: MathNodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    #[serde(default)]
    pub children: Vec<MathFlow>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 可编辑数学树的节点类别；有序子节点保留 TeX 原子和符号次序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MathNodeKind {
    Row,
    Symbol,
    Text,
    Operator,
    Fraction,
    Radical,
    Superscript,
    Subscript,
    SubSup,
    Delimited,
    SizedDelimiter,
    Matrix,
    Substack,
    Intertext,
    Hline,
    Sum,
    Integral,
    Product,
    Limit,
    OverUnder,
    CancelTo,
    Accent,
    Space,
    Color,
    TextColor,
    ColorBox,
    FColorBox,
    Strut,
    Phantom,
    Style,
    Group,
    Placeholder,
}

impl MathNodeKind {
    /// 返回在键盘插入和布局中视为可选占位的操作数索引。
    pub fn optional_placeholder_operands(self) -> &'static [usize] {
        match self {
            Self::Radical | Self::Limit => &[0],
            Self::Sum | Self::Product | Self::Integral => &[0, 1],
            Self::OverUnder => &[1, 2],
            _ => &[],
        }
    }
}

/// 校验数学节点的属性键与值域；每个 `MathNodeKind` 的契约只在此分派一次。
pub(crate) fn math_node_attributes_supported(
    kind: MathNodeKind,
    text: Option<&str>,
    attributes: &BTreeMap<String, String>,
) -> bool {
    match kind {
        MathNodeKind::Symbol => symbol_attributes_supported(attributes),
        MathNodeKind::Operator => {
            required_enum_attribute_supported(attributes, "limits", &["true", "false"])
        }
        MathNodeKind::Fraction => {
            required_enum_attribute_supported(attributes, "style", FRACTION_STYLES)
        }
        MathNodeKind::Delimited => delimited_attributes_supported(attributes),
        MathNodeKind::SizedDelimiter => sized_delimiter_attributes_supported(text, attributes),
        MathNodeKind::Matrix => matrix_attributes_supported(attributes),
        MathNodeKind::Integral => {
            required_enum_attribute_supported(attributes, "integralKind", INTEGRAL_KINDS)
        }
        MathNodeKind::Accent => {
            required_enum_attribute_supported(attributes, "accent", ACCENT_KINDS)
        }
        MathNodeKind::Space => space_attributes_supported(attributes),
        MathNodeKind::Text => required_enum_attribute_supported(attributes, "style", TEXT_STYLES),
        MathNodeKind::Color | MathNodeKind::TextColor | MathNodeKind::ColorBox => {
            css_color_attributes_supported(attributes)
        }
        MathNodeKind::FColorBox => fcolorbox_attributes_supported(attributes),
        MathNodeKind::Strut => strut_attributes_supported(attributes),
        MathNodeKind::Phantom => {
            required_enum_attribute_supported(attributes, "phantomKind", PHANTOM_KINDS)
        }
        MathNodeKind::Row
        | MathNodeKind::Substack
        | MathNodeKind::Intertext
        | MathNodeKind::Hline
        | MathNodeKind::OverUnder
        | MathNodeKind::CancelTo
        | MathNodeKind::Style
        | MathNodeKind::Group
        | MathNodeKind::Placeholder
        | MathNodeKind::Sum
        | MathNodeKind::Product
        | MathNodeKind::Limit
        | MathNodeKind::Radical
        | MathNodeKind::Superscript
        | MathNodeKind::Subscript
        | MathNodeKind::SubSup => attributes.is_empty(),
    }
}

/// Symbol 节点：`atomClass` 可缺省，出现时必须是已知原子类别。
fn symbol_attributes_supported(attributes: &BTreeMap<String, String>) -> bool {
    keys_supported(attributes, &["atomClass"])
        && attributes
            .get("atomClass")
            .is_none_or(|value| ATOM_CLASSES.contains(&value.as_str()))
}

/// Delimited 节点：`left` 与 `right` 必须同时出现且都是合法定界符。
fn delimited_attributes_supported(attributes: &BTreeMap<String, String>) -> bool {
    keys_supported(attributes, &["left", "right"])
        && attributes
            .get("left")
            .is_some_and(|value| valid_delimiter(value))
        && attributes
            .get("right")
            .is_some_and(|value| valid_delimiter(value))
}

/// SizedDelimiter 节点：文本本身必须是合法定界符，`size` 与 `atomClass` 必填。
fn sized_delimiter_attributes_supported(
    text: Option<&str>,
    attributes: &BTreeMap<String, String>,
) -> bool {
    keys_supported(attributes, &["size", "atomClass"])
        && text.is_some_and(valid_delimiter)
        && enum_attribute_matches(attributes, "size", DELIMITER_SIZES)
        && enum_attribute_matches(attributes, "atomClass", ATOM_CLASSES)
}

/// Matrix 节点：`style` 必填为矩阵样式，`columns` 必填且只含列对齐字符。
fn matrix_attributes_supported(attributes: &BTreeMap<String, String>) -> bool {
    keys_supported(attributes, &["style", "columns"])
        && enum_attribute_matches(attributes, "style", MATRIX_STYLES)
        && attributes.get("columns").is_some_and(|columns| {
            columns
                .chars()
                .all(|column| matches!(column, 'l' | 'c' | 'r' | '|'))
        })
}

/// Space 节点：`spaceKind` 必填；`hspace` 要求成组且合法的 `value` 维度，其余种类禁止值维度。
fn space_attributes_supported(attributes: &BTreeMap<String, String>) -> bool {
    keys_supported(
        attributes,
        &[
            "spaceKind",
            "valueNumerator",
            "valueDenominator",
            "valueUnit",
        ],
    ) && enum_attribute_matches(attributes, "spaceKind", SPACE_KINDS)
        && if attributes.get("spaceKind").map(String::as_str) == Some("hspace") {
            dimension_present(attributes, "value") && dimension_supported(attributes, "value")
        } else {
            !attributes.contains_key("valueNumerator")
                && !attributes.contains_key("valueDenominator")
                && !attributes.contains_key("valueUnit")
        }
}

/// Color/TextColor/ColorBox 节点：`color` 必填且为合法 CSS 颜色。
fn css_color_attributes_supported(attributes: &BTreeMap<String, String>) -> bool {
    keys_supported(attributes, &["color"])
        && attributes
            .get("color")
            .is_some_and(|color| valid_css_color(color))
}

/// FColorBox 节点：边框色与填充色都必填且为合法 CSS 颜色。
fn fcolorbox_attributes_supported(attributes: &BTreeMap<String, String>) -> bool {
    keys_supported(attributes, &["borderColor", "fillColor"])
        && attributes
            .get("borderColor")
            .is_some_and(|color| valid_css_color(color))
        && attributes
            .get("fillColor")
            .is_some_and(|color| valid_css_color(color))
}

/// Strut 节点：高度与深度两组维度都必须完整给出且合法。
fn strut_attributes_supported(attributes: &BTreeMap<String, String>) -> bool {
    keys_supported(
        attributes,
        &[
            "heightNumerator",
            "heightDenominator",
            "heightUnit",
            "depthNumerator",
            "depthDenominator",
            "depthUnit",
        ],
    ) && dimension_present(attributes, "height")
        && dimension_present(attributes, "depth")
        && dimension_supported(attributes, "height")
        && dimension_supported(attributes, "depth")
}

/// 单枚举属性节点：只允许出现 `key` 一个属性键，且该键必填并取 `allowed` 中的值。
fn required_enum_attribute_supported(
    attributes: &BTreeMap<String, String>,
    key: &str,
    allowed: &[&str],
) -> bool {
    keys_supported(attributes, &[key]) && enum_attribute_matches(attributes, key, allowed)
}

/// 属性键集合必须全部落在 `allowed` 内；空切片表示不接受任何属性。
fn keys_supported(attributes: &BTreeMap<String, String>, allowed: &[&str]) -> bool {
    attributes.keys().all(|key| allowed.contains(&key.as_str()))
}

/// `key` 必填且取值在 `allowed` 内；用于组合多个属性的契约。
fn enum_attribute_matches(
    attributes: &BTreeMap<String, String>,
    key: &str,
    allowed: &[&str],
) -> bool {
    attributes
        .get(key)
        .is_some_and(|value| allowed.contains(&value.as_str()))
}

/// 判断 `<prefix>Numerator/<prefix>Denominator/<prefix>Unit` 是否三个键同时存在。
fn dimension_present(attributes: &BTreeMap<String, String>, prefix: &str) -> bool {
    attributes.contains_key(&format!("{prefix}Numerator"))
        && attributes.contains_key(&format!("{prefix}Denominator"))
        && attributes.contains_key(&format!("{prefix}Unit"))
}

/// 校验 `<prefix>Numerator/<prefix>Denominator/<prefix>Unit` 维度：
/// 三者全缺省视为未设置，全部给出时要求单位为 `em`、分子为整数、分母为正整数。
fn dimension_supported(attributes: &BTreeMap<String, String>, prefix: &str) -> bool {
    match (
        attributes.get(&format!("{prefix}Numerator")),
        attributes.get(&format!("{prefix}Denominator")),
        attributes.get(&format!("{prefix}Unit")),
    ) {
        (None, None, None) => true,
        (Some(numerator), Some(denominator), Some(unit)) if unit == "em" => numerator
            .parse::<i128>()
            .is_ok_and(|_| denominator.parse::<i128>().is_ok_and(|value| value > 0)),
        _ => false,
    }
}

/// `atomClass` 允许的原子类别。
const ATOM_CLASSES: &[&str] = &["ord", "op", "bin", "rel", "open", "close", "punct", "inner"];
/// 定界符 `size` 允许的尺寸。
const DELIMITER_SIZES: &[&str] = &["big", "Big", "bigg", "Bigg"];
/// 分数节点 `style` 允许的取值。
const FRACTION_STYLES: &[&str] = &["frac", "dfrac", "tfrac", "cfrac"];
/// 矩阵节点 `style` 允许的取值。
const MATRIX_STYLES: &[&str] = &[
    "matrix", "pmatrix", "bmatrix", "vmatrix", "Vmatrix", "Bmatrix", "cases", "array", "aligned",
    "split",
];
/// 积分节点 `integralKind` 允许的取值。
const INTEGRAL_KINDS: &[&str] = &["int", "iint", "iiint", "oint", "oiint"];
/// 重音节点 `accent` 允许的取值。
const ACCENT_KINDS: &[&str] = &[
    "hat",
    "check",
    "breve",
    "acute",
    "grave",
    "tilde",
    "bar",
    "vec",
    "dot",
    "ddot",
    "dddot",
    "ddddot",
    "widehat",
    "widetilde",
    "overline",
    "underline",
    "overbrace",
    "underbrace",
    "overleftarrow",
    "overrightarrow",
    "overleftrightarrow",
    "underleftarrow",
    "underrightarrow",
    "underleftrightarrow",
    "cancel",
    "bcancel",
    "xcancel",
    "boxed",
    "ring",
    "not",
];
/// 空白节点 `spaceKind` 允许的取值。
const SPACE_KINDS: &[&str] = &[
    "thin",
    "medium",
    "thick",
    "negative_thin",
    "quad",
    "qquad",
    "control",
    "hspace",
    "space",
    "control_space",
    "no_break_space",
];
/// 文本节点 `style` 允许的取值。
const TEXT_STYLES: &[&str] = &[
    "rm",
    "bf",
    "it",
    "sf",
    "tt",
    "bb",
    "cal",
    "frak",
    "scr",
    "boldsymbol",
    "pmb",
    "text",
];
/// 幻影节点 `phantomKind` 允许的取值。
const PHANTOM_KINDS: &[&str] = &["phantom", "vphantom", "hphantom"];

pub(crate) fn math_node_structure_supported(
    parent: Option<MathNodeKind>,
    kind: MathNodeKind,
    text: Option<&str>,
    children: &[MathNodeKind],
) -> bool {
    let arity = |expected| children.len() == expected;
    let text_present = text.is_some();
    if text_present
        && !matches!(
            kind,
            MathNodeKind::Symbol | MathNodeKind::Operator | MathNodeKind::SizedDelimiter
        )
    {
        return false;
    }
    match kind {
        MathNodeKind::Row
        | MathNodeKind::Substack
        | MathNodeKind::Text
        | MathNodeKind::Style
        | MathNodeKind::Group => text.is_none(),
        MathNodeKind::Symbol | MathNodeKind::Operator => text_present && children.is_empty(),
        MathNodeKind::Fraction
        | MathNodeKind::Superscript
        | MathNodeKind::Subscript
        | MathNodeKind::Sum
        | MathNodeKind::Product
        | MathNodeKind::CancelTo => arity(2),
        MathNodeKind::Radical => arity(2),
        MathNodeKind::OverUnder | MathNodeKind::SubSup => arity(3),
        MathNodeKind::Delimited
        | MathNodeKind::Accent
        | MathNodeKind::Color
        | MathNodeKind::TextColor
        | MathNodeKind::ColorBox
        | MathNodeKind::FColorBox
        | MathNodeKind::Phantom => arity(1),
        MathNodeKind::Intertext => parent == Some(MathNodeKind::Matrix) && arity(1),
        MathNodeKind::SizedDelimiter => text_present && children.is_empty(),
        MathNodeKind::Matrix => children.iter().all(|child| {
            matches!(
                child,
                MathNodeKind::Row | MathNodeKind::Hline | MathNodeKind::Intertext
            )
        }),
        MathNodeKind::Integral => arity(2),
        MathNodeKind::Limit => arity(1),
        MathNodeKind::Hline => parent == Some(MathNodeKind::Matrix) && children.is_empty(),
        MathNodeKind::Space | MathNodeKind::Strut | MathNodeKind::Placeholder => {
            children.is_empty()
        }
    }
}

fn valid_delimiter(value: &str) -> bool {
    value == "empty"
        || value.strip_prefix("char:").is_some_and(|character| {
            matches!(character, "(" | ")" | "[" | "]" | "|" | "/" | "<" | ">")
        })
        || value.strip_prefix("named:").is_some_and(|name| {
            matches!(
                name,
                "{" | "}"
                    | "|"
                    | "langle"
                    | "rangle"
                    | "lfloor"
                    | "rfloor"
                    | "lceil"
                    | "rceil"
                    | "lvert"
                    | "rvert"
                    | "lVert"
                    | "rVert"
                    | "vert"
                    | "Vert"
                    | "uparrow"
                    | "downarrow"
                    | "Uparrow"
                    | "Downarrow"
                    | "updownarrow"
                    | "Updownarrow"
                    | "backslash"
                    | "lgroup"
                    | "rgroup"
                    | "lmoustache"
                    | "rmoustache"
            )
        })
}

fn valid_css_color(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

/// 行内数学对象，作为段落内联节点保存。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineMath {
    pub id: NodeId,
    pub math_tree: MathTree,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 单行显示公式，作为文档块保存。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Equation {
    pub id: NodeId,
    pub math_tree: MathTree,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 多行对齐公式。每一行由有序的、可独立编辑的公式单元格组成。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Align {
    pub id: NodeId,
    pub rows: Vec<MathAlignRow>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// 对齐公式的一行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MathAlignRow {
    pub id: NodeId,
    pub cells: Vec<MathTree>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
