//! 提供数学字体角色的默认字体族。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

/// 返回当前数学后端使用的内置字体族。
pub(crate) fn default_family() -> &'static str {
    caret_math::BUILTIN_FONT_FAMILY
}
