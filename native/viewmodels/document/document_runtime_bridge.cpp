// 持有由应用入口注入的唯一 Rust 文档运行时。
#include "document_runtime_bridge.hpp"

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include <optional>
#include <utility>

namespace caret {
namespace {
std::optional<rust::Box<ffi::DocumentRuntime>>& installed_runtime() {
    static std::optional<rust::Box<ffi::DocumentRuntime>> runtime;
    return runtime;
}
} // namespace

bool install_document_runtime(rust::Box<ffi::DocumentRuntime> runtime) {
    installed_runtime() = std::move(runtime);
    return true;
}

ffi::DocumentRuntime* document_runtime() {
    auto& runtime = installed_runtime();
    return runtime.has_value() ? &**runtime : nullptr;
}

} // namespace caret
#endif
