// 集中定义 Qt 修饰键到物理键意图的跨平台映射。
#pragma once

#include <Qt>

namespace caret::keyboard {

[[nodiscard]] constexpr Qt::KeyboardModifier physicalControlModifier() {
#ifdef Q_OS_MACOS
    return Qt::MetaModifier;
#else
    return Qt::ControlModifier;
#endif
}

[[nodiscard]] constexpr Qt::KeyboardModifier modModifier() { return Qt::ControlModifier; }

[[nodiscard]] constexpr bool isPhysicalControlOnly(Qt::KeyboardModifiers modifiers) {
    return modifiers == physicalControlModifier();
}

[[nodiscard]] constexpr bool isModOnly(Qt::KeyboardModifiers modifiers) {
    return modifiers == modModifier();
}

[[nodiscard]] constexpr Qt::KeyboardModifier structureNavigationModifier() {
#ifdef Q_OS_MACOS
    return Qt::AltModifier;
#else
    return Qt::ControlModifier;
#endif
}

[[nodiscard]] constexpr bool isStructureBlockNavigation(Qt::KeyboardModifiers modifiers, int key) {
    return modifiers == structureNavigationModifier() && (key == Qt::Key_Up || key == Qt::Key_Down);
}

} // namespace caret::keyboard
