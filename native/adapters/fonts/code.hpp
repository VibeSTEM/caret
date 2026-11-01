// 提供内置代码字体族名并确保字体资源注册到当前 Qt 应用。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#pragma once

#include <QString>

namespace caret::fonts {

/// 将四种内置字形样式全部注册到 Qt；有任何失败时回滚并返回空族名。
[[nodiscard]] auto load_code_font() -> const QString&;

} // namespace caret::fonts
