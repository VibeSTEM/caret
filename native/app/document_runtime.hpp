// 持久 Rust 文档运行时在桌面入口与 ViewModel 之间的所有权转交。
#pragma once

#include "caret_ffi.h"

namespace caret {

/// 安装由桌面入口持有至事件循环结束的 Rust 文档运行时。
bool install_document_runtime(rust::Box<ffi::DocumentRuntime> runtime);

} // namespace caret
