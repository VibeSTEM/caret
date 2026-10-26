#pragma once

#include <QByteArray>

namespace caret {

/// 读取 Rust 已校验的内置主题，并在创建 QML 引擎之前写入快照。
/// 失败时向 stderr 写诊断并返回 false。不在这里保存第二份默认值。
bool install_theme_catalog(const QByteArray& payload);

} // namespace caret
