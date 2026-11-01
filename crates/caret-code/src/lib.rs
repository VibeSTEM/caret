//! 提供代码文本领域能力，并隔离语法/编辑支持与排版后端。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod fonts;

pub use fonts::{BUILTIN_FONT_FAMILY, BuiltinFontFace, BuiltinFontStyle, builtin_font_faces};

#[cfg(test)]
mod tests {
    use super::{BuiltinFontStyle, builtin_font_faces};

    #[test]
    fn bundled_font_faces_expose_all_supported_static_styles() {
        let faces = builtin_font_faces();
        assert_eq!(faces.len(), 4);
        assert_eq!(
            faces.iter().map(|face| face.style).collect::<Vec<_>>(),
            [
                BuiltinFontStyle::Regular,
                BuiltinFontStyle::Bold,
                BuiltinFontStyle::Italic,
                BuiltinFontStyle::BoldItalic,
            ]
        );
        assert!(faces.iter().all(|face| !face.bytes.is_empty()));
    }
}
