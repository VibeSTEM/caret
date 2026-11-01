//! 定义与解析后端无关的持久化数学树。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::NodeId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 当前 Caret 数学树 schema 版本。
pub const MATH_TREE_SCHEMA_VERSION: u16 = 2;
/// `.cdoc` 中单个公式源码的最大 UTF-8 字节数。
pub const MAX_MATH_SOURCE_BYTES: usize = 1024 * 1024;
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
    /// 与 `root` 对应的 LaTeX 源码。导入时为原文；树被编辑后由 `caret-math` 序列化器
    /// 重新生成，因此始终描述当前树，而不是历史输入。`sourceRange` 相对该字符串。
    pub source: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<MathSourceRange>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_range: Option<MathSourceRange>,
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

pub(crate) fn math_node_attributes_supported(
    kind: MathNodeKind,
    text: Option<&str>,
    attributes: &BTreeMap<String, String>,
) -> bool {
    let allowed_keys: &[&str] = match kind {
        MathNodeKind::Symbol => &["atomClass"],
        MathNodeKind::Operator => &["limits"],
        MathNodeKind::Fraction => &["style"],
        MathNodeKind::Delimited => &["left", "right"],
        MathNodeKind::SizedDelimiter => &["size", "atomClass"],
        MathNodeKind::Matrix => &["style", "columns"],
        MathNodeKind::Sum | MathNodeKind::Product | MathNodeKind::Limit => &[],
        MathNodeKind::Integral => &["integralKind"],
        MathNodeKind::Accent => &["accent"],
        MathNodeKind::Space => &[
            "spaceKind",
            "valueNumerator",
            "valueDenominator",
            "valueUnit",
        ],
        MathNodeKind::Text => &["style"],
        MathNodeKind::Color | MathNodeKind::TextColor | MathNodeKind::ColorBox => &["color"],
        MathNodeKind::FColorBox => &["borderColor", "fillColor"],
        MathNodeKind::Strut => &[
            "heightNumerator",
            "heightDenominator",
            "heightUnit",
            "depthNumerator",
            "depthDenominator",
            "depthUnit",
        ],
        MathNodeKind::Phantom => &["phantomKind"],
        MathNodeKind::Row
        | MathNodeKind::Substack
        | MathNodeKind::Intertext
        | MathNodeKind::Hline
        | MathNodeKind::OverUnder
        | MathNodeKind::CancelTo
        | MathNodeKind::Style
        | MathNodeKind::Group
        | MathNodeKind::Placeholder => &[],
        MathNodeKind::Radical
        | MathNodeKind::Superscript
        | MathNodeKind::Subscript
        | MathNodeKind::SubSup => &[],
    };
    if attributes
        .keys()
        .any(|key| !allowed_keys.contains(&key.as_str()))
    {
        return false;
    }

    let enum_value = |key: &str, allowed: &[&str]| {
        attributes
            .get(key)
            .is_some_and(|value| allowed.contains(&value.as_str()))
    };
    let dimension = |prefix: &str| {
        let numerator = format!("{prefix}Numerator");
        let denominator = format!("{prefix}Denominator");
        let unit = format!("{prefix}Unit");
        match (
            attributes.get(&numerator),
            attributes.get(&denominator),
            attributes.get(&unit),
        ) {
            (None, None, None) => true,
            (Some(numerator), Some(denominator), Some(unit)) if unit == "em" => numerator
                .parse::<i128>()
                .is_ok_and(|_| denominator.parse::<i128>().is_ok_and(|value| value > 0)),
            _ => false,
        }
    };
    let dimension_present = |prefix: &str| {
        attributes.contains_key(&format!("{prefix}Numerator"))
            && attributes.contains_key(&format!("{prefix}Denominator"))
            && attributes.contains_key(&format!("{prefix}Unit"))
    };

    match kind {
        MathNodeKind::Symbol => attributes.get("atomClass").is_none_or(|_| {
            enum_value(
                "atomClass",
                &["ord", "op", "bin", "rel", "open", "close", "punct", "inner"],
            )
        }),
        MathNodeKind::Operator => enum_value("limits", &["true", "false"]),
        MathNodeKind::Fraction => enum_value("style", &["frac", "dfrac", "tfrac", "cfrac"]),
        MathNodeKind::Delimited => {
            attributes.contains_key("left")
                && attributes.contains_key("right")
                && attributes
                    .get("left")
                    .is_none_or(|value| valid_delimiter(value))
                && attributes
                    .get("right")
                    .is_none_or(|value| valid_delimiter(value))
        }
        MathNodeKind::SizedDelimiter => {
            text.is_some_and(valid_delimiter)
                && enum_value("size", &["big", "Big", "bigg", "Bigg"])
                && enum_value(
                    "atomClass",
                    &["ord", "op", "bin", "rel", "open", "close", "punct", "inner"],
                )
        }
        MathNodeKind::Matrix => {
            enum_value(
                "style",
                &[
                    "matrix", "pmatrix", "bmatrix", "vmatrix", "Vmatrix", "Bmatrix", "cases",
                    "array", "aligned", "split",
                ],
            ) && attributes.get("columns").is_some_and(|columns| {
                columns
                    .chars()
                    .all(|column| matches!(column, 'l' | 'c' | 'r' | '|'))
            })
        }
        MathNodeKind::Integral => {
            enum_value("integralKind", &["int", "iint", "iiint", "oint", "oiint"])
        }
        MathNodeKind::Accent => enum_value(
            "accent",
            &[
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
            ],
        ),
        MathNodeKind::Space => {
            enum_value(
                "spaceKind",
                &[
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
                ],
            ) && if attributes.get("spaceKind").map(String::as_str) == Some("hspace") {
                dimension_present("value") && dimension("value")
            } else {
                !attributes.contains_key("valueNumerator")
                    && !attributes.contains_key("valueDenominator")
                    && !attributes.contains_key("valueUnit")
            }
        }
        MathNodeKind::Text => enum_value(
            "style",
            &[
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
            ],
        ),
        MathNodeKind::Color | MathNodeKind::TextColor | MathNodeKind::ColorBox => attributes
            .get("color")
            .is_some_and(|color| valid_css_color(color)),
        MathNodeKind::FColorBox => {
            attributes
                .get("borderColor")
                .is_some_and(|color| valid_css_color(color))
                && attributes
                    .get("fillColor")
                    .is_some_and(|color| valid_css_color(color))
        }
        MathNodeKind::Strut => {
            dimension_present("height")
                && dimension_present("depth")
                && dimension("height")
                && dimension("depth")
        }
        MathNodeKind::Phantom => enum_value("phantomKind", &["phantom", "vphantom", "hphantom"]),
        _ => true,
    }
}

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

/// 相对 `MathTree::source` 的半开 UTF-8 字节范围。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MathSourceRange {
    pub start: u32,
    pub end: u32,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
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
    /// 完整 `aligned` 环境的 LaTeX 源码，包含行与单元格分隔符。导入时为原文；
    /// 编辑任一单元格后由序列化器按当前行与单元格重新生成。
    pub source: String,
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
