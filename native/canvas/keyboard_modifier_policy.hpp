// 集中定义 Qt 修饰键到物理键意图的跨平台映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

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

[[nodiscard]] constexpr bool isToggleTableInsert(Qt::KeyboardModifiers modifiers, int key) {
    return modifiers == Qt::AltModifier && key == Qt::Key_T;
}

/// 插入行/列：Windows/Linux 为 Alt+方向键，macOS 为 Control+Option+Shift+方向键。
[[nodiscard]] constexpr bool isTableShapeInsert(Qt::KeyboardModifiers modifiers, int key) {
    const bool arrow = key == Qt::Key_Up || key == Qt::Key_Down || key == Qt::Key_Left ||
                       key == Qt::Key_Right;
#ifdef Q_OS_MACOS
    return modifiers == (physicalControlModifier() | Qt::AltModifier | Qt::ShiftModifier) &&
           arrow;
#else
    return modifiers == Qt::AltModifier && arrow;
#endif
}

/// 删除行：Alt/Option+Shift+Backspace 或 Delete。
[[nodiscard]] constexpr bool isDeleteTableRow(Qt::KeyboardModifiers modifiers, int key) {
    return modifiers == (Qt::AltModifier | Qt::ShiftModifier) &&
           (key == Qt::Key_Backspace || key == Qt::Key_Delete);
}

/// 删除列：Alt/Option+Backspace 或 Delete。
[[nodiscard]] constexpr bool isDeleteTableColumn(Qt::KeyboardModifiers modifiers, int key) {
    return modifiers == Qt::AltModifier &&
           (key == Qt::Key_Backspace || key == Qt::Key_Delete);
}

} // namespace caret::keyboard
