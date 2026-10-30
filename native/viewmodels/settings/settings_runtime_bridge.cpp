// 持有应用级 Rust SettingsRuntime，供各设置分类投影转发意图。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "settings_runtime_bridge.hpp"

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include <optional>
#include <utility>

namespace caret {
namespace {
std::optional<rust::Box<ffi::SettingsRuntime>>& installed_runtime() {
    static std::optional<rust::Box<ffi::SettingsRuntime>> runtime;
    return runtime;
}
} // namespace

bool install_settings_runtime(rust::Box<ffi::SettingsRuntime> runtime) {
    installed_runtime() = std::move(runtime);
    return true;
}

ffi::SettingsRuntime* settings_runtime() {
    auto& runtime = installed_runtime();
    return runtime.has_value() ? &**runtime : nullptr;
}

} // namespace caret
#endif
