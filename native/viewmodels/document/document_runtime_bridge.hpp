// Rust 文档运行时在桌面入口和文档投影之间的单一所有权桥接。
#pragma once

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"

namespace caret {

bool install_document_runtime(rust::Box<ffi::DocumentRuntime> runtime);
/// 返回 GUI 线程使用的文档运行时；未安装时返回空指针。
ffi::DocumentRuntime* document_runtime();

} // namespace caret
#endif
