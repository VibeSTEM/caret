//! 注册供代码字体角色与命令输入使用的内置 JetBrains Mono 字体。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use fontique::{Blob, Collection};

/// 每次构建或刷新目录时，将四种内置字形样式注册到 Fontique。
pub(super) fn register(collection: &mut Collection) {
    for face in caret_code::builtin_font_faces() {
        let data: std::sync::Arc<dyn AsRef<[u8]> + Send + Sync> = std::sync::Arc::new(face.bytes);
        collection.register_fonts(Blob::new(data), None);
    }
}

/// 返回代码角色使用的内置默认族。
pub(crate) fn default_family() -> &'static str {
    caret_code::BUILTIN_FONT_FAMILY
}
