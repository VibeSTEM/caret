//! 管理共享字体角色、默认族名及静态字体资源。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

/// 字体请求所属的语义角色；角色默认族不代表用户偏好或 fallback 策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontRole {
    /// 正文文字，由宿主字体后端选择系统默认字体。
    #[default]
    Body,
    /// 数学公式使用的内置数学字体角色。
    Math,
    /// 代码文字使用的内置等宽字体角色。
    Code,
}

impl FontRole {
    /// 返回该角色当前的默认族名；正文角色由宿主字体后端决定。
    pub const fn default_font_family(self) -> Option<&'static str> {
        match self {
            Self::Body => None,
            Self::Math => Some(DEFAULT_MATH_FONT_FAMILY),
            Self::Code => Some(DEFAULT_CODE_FONT_FAMILY),
        }
    }
}

/// 当前数学角色使用的默认字体族。
pub const DEFAULT_MATH_FONT_FAMILY: &str = "STIX Two Math";

/// 当前代码角色使用的默认字体族。
pub const DEFAULT_CODE_FONT_FAMILY: &str = "JetBrains Mono";

/// 内置静态代码字体的字形样式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinFontStyle {
    /// 常规字形。
    Regular,
    /// 粗体字形。
    Bold,
    /// 斜体字形。
    Italic,
    /// 粗斜体字形。
    BoldItalic,
}

/// 可注册给文字塑形后端的内置代码字体数据。
#[derive(Debug, Clone, Copy)]
pub struct BuiltinFontFace {
    /// 此字体文件对应的样式。
    pub style: BuiltinFontStyle,
    /// 静态嵌入的 OpenType TrueType 字体字节。
    pub bytes: &'static [u8],
}

static DEFAULT_CODE_FONT_FACES: [BuiltinFontFace; 4] = [
    BuiltinFontFace {
        style: BuiltinFontStyle::Regular,
        bytes: include_bytes!(
            "../../../resources/fonts/code/jetbrains-mono/JetBrainsMono-Regular.ttf"
        ),
    },
    BuiltinFontFace {
        style: BuiltinFontStyle::Bold,
        bytes: include_bytes!(
            "../../../resources/fonts/code/jetbrains-mono/JetBrainsMono-Bold.ttf"
        ),
    },
    BuiltinFontFace {
        style: BuiltinFontStyle::Italic,
        bytes: include_bytes!(
            "../../../resources/fonts/code/jetbrains-mono/JetBrainsMono-Italic.ttf"
        ),
    },
    BuiltinFontFace {
        style: BuiltinFontStyle::BoldItalic,
        bytes: include_bytes!(
            "../../../resources/fonts/code/jetbrains-mono/JetBrainsMono-BoldItalic.ttf"
        ),
    },
];

/// 返回当前默认数学字体的静态字节；许可证见仓库字体资源目录中的 `OFL.txt`。
pub fn default_math_font() -> &'static [u8] {
    include_bytes!("../../../resources/fonts/math/stix-two-math/STIXTwoMath-Regular.otf")
}

/// 返回当前默认代码字体的所有字形面；字节由本 crate 持有且为静态只读数据。
pub fn default_code_font_faces() -> &'static [BuiltinFontFace] {
    &DEFAULT_CODE_FONT_FACES
}

#[cfg(test)]
mod tests {
    use super::{
        BuiltinFontStyle, DEFAULT_CODE_FONT_FACES, DEFAULT_CODE_FONT_FAMILY,
        DEFAULT_MATH_FONT_FAMILY, FontRole, default_math_font,
    };

    #[test]
    fn roles_resolve_only_fixed_builtin_defaults() {
        assert_eq!(FontRole::Body.default_font_family(), None);
        assert_eq!(
            FontRole::Math.default_font_family(),
            Some(DEFAULT_MATH_FONT_FAMILY)
        );
        assert_eq!(
            FontRole::Code.default_font_family(),
            Some(DEFAULT_CODE_FONT_FAMILY)
        );
    }

    #[test]
    fn static_font_resources_include_all_code_styles_and_math_data() {
        assert_eq!(
            DEFAULT_CODE_FONT_FACES
                .iter()
                .map(|face| face.style)
                .collect::<Vec<_>>(),
            [
                BuiltinFontStyle::Regular,
                BuiltinFontStyle::Bold,
                BuiltinFontStyle::Italic,
                BuiltinFontStyle::BoldItalic,
            ]
        );
        assert!(
            DEFAULT_CODE_FONT_FACES
                .iter()
                .all(|face| !face.bytes.is_empty())
        );
        assert!(!default_math_font().is_empty());
    }
}
