// 声明将 Rust 字形帧转换为 Qt 可绑定值的适配入口。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#pragma once

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include <QVariantMap>

namespace caret::glyph_frame_adapter {

/// 转换 Rust 页面帧；比例必须是有限正数，无效输入返回 `valid: false` 和稳定错误键。
[[nodiscard]] QVariantMap to_variant_map(const ffi::PageGlyphFrame& frame, double px_per_pt);

} // namespace caret::glyph_frame_adapter
#endif
