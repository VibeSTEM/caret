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

/// `.cdoc` 数学节点 `atomClass` 的合法值，同时供排版层计算 TeX 原子间距。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathAtomClass {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    Inner,
}

impl MathAtomClass {
    /// 返回持久化格式中的属性值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ord => "ord",
            Self::Op => "op",
            Self::Bin => "bin",
            Self::Rel => "rel",
            Self::Open => "open",
            Self::Close => "close",
            Self::Punct => "punct",
            Self::Inner => "inner",
        }
    }

    /// 按 `.cdoc` 属性值解析原子类别。
    pub fn parse(value: &str) -> Option<Self> {
        MATH_ATOM_CLASSES
            .iter()
            .copied()
            .find(|class| class.as_str() == value)
    }
}

/// 矩阵样式的单元格列对齐语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathMatrixAlignment {
    Center,
    Left,
    Alternating,
    Columns,
}

/// 矩阵单元格相对宿主公式字号的策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathMatrixCellSize {
    Compact,
    Inherit,
}

/// 矩阵列之间的水平间距策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathMatrixColumnGap {
    HalfEm,
    Em,
    Zero,
}

/// `.cdoc` 矩阵 `style` 值及 parser、serializer 与布局共同使用的语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathMatrixStyle {
    /// 持久化属性值，也是受支持的 LaTeX 环境名。
    pub name: &'static str,
    /// 自动配对的左定界符。
    pub left_delimiter: Option<char>,
    /// 自动配对的右定界符。
    pub right_delimiter: Option<char>,
    /// 单元格列对齐策略。
    pub alignment: MathMatrixAlignment,
    /// 是否按数组的列规格读取单元格对齐和竖线。
    pub has_column_spec: bool,
    /// 单元格字号策略。
    pub cell_size: MathMatrixCellSize,
    /// 默认列间距策略。
    pub column_gap: MathMatrixColumnGap,
}

/// 旧或未验证节点缺少矩阵样式时使用的兼容默认值。
pub const DEFAULT_MATH_MATRIX_STYLE: MathMatrixStyle = MathMatrixStyle {
    name: "matrix",
    left_delimiter: None,
    right_delimiter: None,
    alignment: MathMatrixAlignment::Center,
    has_column_spec: false,
    cell_size: MathMatrixCellSize::Compact,
    column_gap: MathMatrixColumnGap::HalfEm,
};

/// Equation/Align 来源适配固定使用的矩阵环境。
pub const MATH_ALIGNED_MATRIX_STYLE: MathMatrixStyle = MathMatrixStyle {
    name: "aligned",
    left_delimiter: None,
    right_delimiter: None,
    alignment: MathMatrixAlignment::Alternating,
    has_column_spec: false,
    cell_size: MathMatrixCellSize::Inherit,
    column_gap: MathMatrixColumnGap::Zero,
};

/// 根据持久化值查找受支持的矩阵样式描述符。
pub fn math_matrix_style(name: &str) -> Option<&'static MathMatrixStyle> {
    MATH_MATRIX_STYLES.iter().find(|style| style.name == name)
}

/// `.cdoc` 中 `SizedDelimiter.size` 的值及其排版比例。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MathDelimiterSize {
    /// 持久化属性值。
    pub name: &'static str,
    /// 相对当前数学字号的目标尺寸倍数。
    pub scale: f64,
}

/// 定界符尺寸值及其布局比例的唯一来源。
pub const MATH_DELIMITER_SIZES: &[MathDelimiterSize] = &[
    MathDelimiterSize {
        name: "big",
        scale: 1.2,
    },
    MathDelimiterSize {
        name: "Big",
        scale: 1.8,
    },
    MathDelimiterSize {
        name: "bigg",
        scale: 2.4,
    },
    MathDelimiterSize {
        name: "Bigg",
        scale: 3.0,
    },
];

impl MathDelimiterSize {
    /// 按 `.cdoc` 属性值查找定界符尺寸。
    pub fn parse(value: &str) -> Option<&'static Self> {
        MATH_DELIMITER_SIZES.iter().find(|size| size.name == value)
    }
}

/// 重音标记的垂直位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathAccentPlacement {
    Over,
    Under,
}

/// 数学重音值到布局行为的格式语义映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathAccentRendering {
    Overline,
    Underline,
    CancelForward,
    CancelBackward,
    CancelCross,
    Boxed,
    Negate,
    Mark {
        character: char,
        placement: MathAccentPlacement,
        brace: bool,
    },
}

/// 数学树重音属性、LaTeX 命令与布局行为的唯一映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathAccent {
    /// `.cdoc` 中的规范属性值。
    pub value: &'static str,
    /// 规范 TeX 命令名（不含反斜杠）。
    pub command: &'static str,
    /// 对应的布局语义。
    pub rendering: MathAccentRendering,
    /// 兼容 `accent_char` 的历史显示字符；方向重音的此字符不同于排版组合符。
    pub legacy_character: Option<char>,
}

/// 根据规范属性值查找重音映射。
pub fn math_accent_by_value(value: &str) -> Option<MathAccent> {
    MATH_ACCENTS
        .iter()
        .copied()
        .find(|accent| accent.value == value)
}

/// 根据 TeX 命令名查找重音映射。
pub fn math_accent_by_command(command: &str) -> Option<MathAccent> {
    MATH_ACCENTS
        .iter()
        .copied()
        .find(|accent| accent.command == command)
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
            keys_supported(attributes, &["accent"])
                && attributes
                    .get("accent")
                    .is_some_and(|value| math_accent_by_value(value).is_some())
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
            .is_none_or(|value| MathAtomClass::parse(value).is_some())
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
        && attributes
            .get("size")
            .is_some_and(|value| MathDelimiterSize::parse(value).is_some())
        && attributes
            .get("atomClass")
            .is_some_and(|value| MathAtomClass::parse(value).is_some())
}

/// Matrix 节点：`style` 必填为矩阵样式，`columns` 必填且只含列对齐字符。
fn matrix_attributes_supported(attributes: &BTreeMap<String, String>) -> bool {
    keys_supported(attributes, &["style", "columns"])
        && attributes
            .get("style")
            .is_some_and(|value| math_matrix_style(value).is_some())
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
pub const MATH_ATOM_CLASSES: &[MathAtomClass] = &[
    MathAtomClass::Ord,
    MathAtomClass::Op,
    MathAtomClass::Bin,
    MathAtomClass::Rel,
    MathAtomClass::Open,
    MathAtomClass::Close,
    MathAtomClass::Punct,
    MathAtomClass::Inner,
];
/// 分数节点 `style` 允许的取值。
const FRACTION_STYLES: &[&str] = &["frac", "dfrac", "tfrac", "cfrac"];
/// 积分节点 `integralKind` 允许的取值。
const INTEGRAL_KINDS: &[&str] = &["int", "iint", "iiint", "oint", "oiint"];
/// 重音节点的规范属性值与其 TeX 命令和布局语义。
macro_rules! accent {
    ($value:literal, $rendering:expr) => {
        accent!(
            $value,
            $rendering,
            default_legacy_accent_character($rendering)
        )
    };
    ($value:literal, $rendering:expr, $legacy_character:expr) => {
        MathAccent {
            value: $value,
            command: $value,
            rendering: $rendering,
            legacy_character: $legacy_character,
        }
    };
}

const fn default_legacy_accent_character(rendering: MathAccentRendering) -> Option<char> {
    match rendering {
        MathAccentRendering::Mark { character, .. } => Some(character),
        _ => None,
    }
}

pub const MATH_ACCENTS: &[MathAccent] = &[
    accent!(
        "hat",
        MathAccentRendering::Mark {
            character: '\u{0302}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "check",
        MathAccentRendering::Mark {
            character: '\u{030C}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "breve",
        MathAccentRendering::Mark {
            character: '\u{0306}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "acute",
        MathAccentRendering::Mark {
            character: '\u{0301}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "grave",
        MathAccentRendering::Mark {
            character: '\u{0300}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "tilde",
        MathAccentRendering::Mark {
            character: '\u{0303}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "bar",
        MathAccentRendering::Mark {
            character: '\u{0304}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "vec",
        MathAccentRendering::Mark {
            character: '\u{20D7}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "dot",
        MathAccentRendering::Mark {
            character: '\u{0307}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "ddot",
        MathAccentRendering::Mark {
            character: '\u{0308}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "dddot",
        MathAccentRendering::Mark {
            character: '\u{20DB}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "ddddot",
        MathAccentRendering::Mark {
            character: '\u{20DC}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "widehat",
        MathAccentRendering::Mark {
            character: '\u{0302}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!(
        "widetilde",
        MathAccentRendering::Mark {
            character: '\u{0303}',
            placement: MathAccentPlacement::Over,
            brace: false
        }
    ),
    accent!("overline", MathAccentRendering::Overline),
    accent!("underline", MathAccentRendering::Underline),
    accent!(
        "overbrace",
        MathAccentRendering::Mark {
            character: '⏞',
            placement: MathAccentPlacement::Over,
            brace: true
        }
    ),
    accent!(
        "underbrace",
        MathAccentRendering::Mark {
            character: '⏟',
            placement: MathAccentPlacement::Under,
            brace: true
        }
    ),
    accent!(
        "overleftarrow",
        MathAccentRendering::Mark {
            character: '\u{20D6}',
            placement: MathAccentPlacement::Over,
            brace: false
        },
        Some('←')
    ),
    accent!(
        "overrightarrow",
        MathAccentRendering::Mark {
            character: '\u{20D7}',
            placement: MathAccentPlacement::Over,
            brace: false
        },
        Some('→')
    ),
    accent!(
        "overleftrightarrow",
        MathAccentRendering::Mark {
            character: '\u{20E1}',
            placement: MathAccentPlacement::Over,
            brace: false
        },
        Some('↔')
    ),
    accent!(
        "underleftarrow",
        MathAccentRendering::Mark {
            character: '\u{20EE}',
            placement: MathAccentPlacement::Under,
            brace: false
        },
        Some('←')
    ),
    accent!(
        "underrightarrow",
        MathAccentRendering::Mark {
            character: '\u{20EF}',
            placement: MathAccentPlacement::Under,
            brace: false
        },
        Some('→')
    ),
    accent!(
        "underleftrightarrow",
        MathAccentRendering::Mark {
            character: '\u{034D}',
            placement: MathAccentPlacement::Under,
            brace: false
        },
        Some('↔')
    ),
    accent!("cancel", MathAccentRendering::CancelForward),
    accent!("bcancel", MathAccentRendering::CancelBackward),
    accent!("xcancel", MathAccentRendering::CancelCross),
    accent!("boxed", MathAccentRendering::Boxed),
    MathAccent {
        value: "ring",
        command: "mathring",
        rendering: MathAccentRendering::Mark {
            character: '\u{030A}',
            placement: MathAccentPlacement::Over,
            brace: false,
        },
        legacy_character: Some('\u{030A}'),
    },
    accent!("not", MathAccentRendering::Negate),
];

/// `.cdoc` 中 `Delimited` 与 `SizedDelimiter` 使用的定界符编码值。
pub const MATH_DELIMITER_VALUES: &[&str] = &[
    "empty",
    "char:(",
    "char:)",
    "char:[",
    "char:]",
    "char:|",
    "char:/",
    "char:<",
    "char:>",
    "named:{",
    "named:}",
    "named:|",
    "named:langle",
    "named:rangle",
    "named:lfloor",
    "named:rfloor",
    "named:lceil",
    "named:rceil",
    "named:lvert",
    "named:rvert",
    "named:lVert",
    "named:rVert",
    "named:vert",
    "named:Vert",
    "named:uparrow",
    "named:downarrow",
    "named:Uparrow",
    "named:Downarrow",
    "named:updownarrow",
    "named:Updownarrow",
    "named:backslash",
    "named:lgroup",
    "named:rgroup",
    "named:lmoustache",
    "named:rmoustache",
];

/// 返回编码后的文档定界符是否属于 `.cdoc` 数学格式值域。
pub fn is_valid_math_delimiter(value: &str) -> bool {
    MATH_DELIMITER_VALUES.contains(&value)
}

/// 矩阵节点 `style` 值及其通用排版语义。
pub const MATH_MATRIX_STYLES: &[MathMatrixStyle] = &[
    DEFAULT_MATH_MATRIX_STYLE,
    MathMatrixStyle {
        name: "pmatrix",
        left_delimiter: Some('('),
        right_delimiter: Some(')'),
        alignment: MathMatrixAlignment::Center,
        has_column_spec: false,
        cell_size: MathMatrixCellSize::Compact,
        column_gap: MathMatrixColumnGap::HalfEm,
    },
    MathMatrixStyle {
        name: "bmatrix",
        left_delimiter: Some('['),
        right_delimiter: Some(']'),
        alignment: MathMatrixAlignment::Center,
        has_column_spec: false,
        cell_size: MathMatrixCellSize::Compact,
        column_gap: MathMatrixColumnGap::HalfEm,
    },
    MathMatrixStyle {
        name: "Bmatrix",
        left_delimiter: Some('{'),
        right_delimiter: Some('}'),
        alignment: MathMatrixAlignment::Center,
        has_column_spec: false,
        cell_size: MathMatrixCellSize::Compact,
        column_gap: MathMatrixColumnGap::HalfEm,
    },
    MathMatrixStyle {
        name: "vmatrix",
        left_delimiter: Some('|'),
        right_delimiter: Some('|'),
        alignment: MathMatrixAlignment::Center,
        has_column_spec: false,
        cell_size: MathMatrixCellSize::Compact,
        column_gap: MathMatrixColumnGap::HalfEm,
    },
    MathMatrixStyle {
        name: "Vmatrix",
        left_delimiter: Some('‖'),
        right_delimiter: Some('‖'),
        alignment: MathMatrixAlignment::Center,
        has_column_spec: false,
        cell_size: MathMatrixCellSize::Compact,
        column_gap: MathMatrixColumnGap::HalfEm,
    },
    MathMatrixStyle {
        name: "cases",
        left_delimiter: Some('{'),
        right_delimiter: None,
        alignment: MathMatrixAlignment::Left,
        has_column_spec: false,
        cell_size: MathMatrixCellSize::Compact,
        column_gap: MathMatrixColumnGap::Em,
    },
    MathMatrixStyle {
        name: "array",
        left_delimiter: None,
        right_delimiter: None,
        alignment: MathMatrixAlignment::Columns,
        has_column_spec: true,
        cell_size: MathMatrixCellSize::Compact,
        column_gap: MathMatrixColumnGap::HalfEm,
    },
    MATH_ALIGNED_MATRIX_STYLE,
    MathMatrixStyle {
        name: "split",
        left_delimiter: None,
        right_delimiter: None,
        alignment: MathMatrixAlignment::Alternating,
        has_column_spec: false,
        cell_size: MathMatrixCellSize::Inherit,
        column_gap: MathMatrixColumnGap::Zero,
    },
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
    is_valid_math_delimiter(value)
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
