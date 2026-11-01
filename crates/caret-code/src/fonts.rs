//! 提供代码字体角色的内置族名和静态字形资源。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

/// 代码字体角色使用的内置字体族名。
pub const BUILTIN_FONT_FAMILY: &str = "JetBrains Mono";

/// 内置代码字体的静态字形样式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinFontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

/// 可直接注册给文字塑形后端的内置字体数据。
#[derive(Debug, Clone, Copy)]
pub struct BuiltinFontFace {
    /// 此字体文件对应的样式。
    pub style: BuiltinFontStyle,
    /// 静态嵌入的 OpenType TrueType 字体字节。
    pub bytes: &'static [u8],
}

static BUILTIN_FONT_FACES: [BuiltinFontFace; 4] = [
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

/// 返回所有内置静态字形样式；切换字体后端时可直接复用这些只读字节。
pub fn builtin_font_faces() -> &'static [BuiltinFontFace] {
    &BUILTIN_FONT_FACES
}
