// Settings 总运行时的唯一 Qt 所有权桥接，分类投影不互相持有。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#pragma once

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"

namespace caret {

/// 应用入口在创建 QML 引擎前注入唯一 Rust SettingsRuntime。
bool install_settings_runtime(rust::Box<ffi::SettingsRuntime> runtime);
/// 返回 GUI 线程使用的设置运行时；未安装时返回空指针。
ffi::SettingsRuntime* settings_runtime();

} // namespace caret
#endif
