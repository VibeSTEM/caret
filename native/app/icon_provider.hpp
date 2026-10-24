#pragma once

#include <QSet>

class QQmlEngine;

namespace caret {

/// 返回唯一允许交给单色 provider 着色的图标名。
const QSet<QString>& monochrome_icon_names();

/// 为 GUI 线程的 Shell 引擎安装内置单色 SVG provider。
/// provider 由引擎拥有，只接受内置图标名与 ARGB 色值，不读取外部路径。
void install_icon_provider(QQmlEngine& engine);

} // namespace caret
