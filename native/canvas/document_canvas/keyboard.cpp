// 将 Qt 画布按键映射为编辑器意图，并按焦点作用域交给对应处理层。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "document_canvas.hpp"
#include "keyboard_modifier_policy.hpp"
#include <QChar>
#include <QKeyEvent>
#include <QQuickItem>
#include <QString>
#include <Qt>
#include <algorithm>

namespace caret {

void DocumentCanvas::keyPressEvent(QKeyEvent* event) {
    const Qt::KeyboardModifiers modifiers = event->modifiers();
    const Qt::KeyboardModifiers semantic_modifiers = modifiers & ~Qt::KeypadModifier;
    if (!m_preedit_text.isEmpty() && event->key() == Qt::Key_Escape &&
        (semantic_modifiers == Qt::NoModifier || semantic_modifiers == Qt::ShiftModifier)) {
        emit editorKeyRequested(QStringLiteral("escape"));
        event->accept();
        return;
    }
    if (!m_preedit_text.isEmpty()) {
        // 组字按键由输入法处理；未被平台输入法消费的按键不得改动命令或正文。
        event->accept();
        return;
    }
    if (keyboard::isModOnly(semantic_modifiers)) {
        if (event->key() == Qt::Key_C || event->key() == Qt::Key_V) {
            emit editorKeyRequested(event->key() == Qt::Key_C ? QStringLiteral("mod-c")
                                                              : QStringLiteral("mod-v"));
            event->accept();
            return;
        }
    }
    if (event->key() == Qt::Key_Escape &&
        (semantic_modifiers == Qt::NoModifier || semantic_modifiers == Qt::ShiftModifier)) {
        emit editorKeyRequested(QStringLiteral("escape"));
        event->accept();
        return;
    }
    if (keyboard::isPhysicalControlOnly(semantic_modifiers) && event->key() == Qt::Key_Space) {
        emit editorKeyRequested(QStringLiteral("expand-structure"));
        event->accept();
        return;
    }
    if (keyboard::isModOnly(semantic_modifiers) && event->key() == Qt::Key_Backspace) {
        emit editorKeyRequested(QStringLiteral("mod-backspace"));
        event->accept();
        return;
    }
    if (semantic_modifiers == Qt::NoModifier && event->key() == Qt::Key_Tab) {
        emit editorKeyRequested(QStringLiteral("tab"));
        event->accept();
        return;
    }
    if (event->key() == Qt::Key_Return || event->key() == Qt::Key_Enter) {
        if (semantic_modifiers == Qt::NoModifier || semantic_modifiers == Qt::ShiftModifier) {
            emit editorKeyRequested(semantic_modifiers == Qt::ShiftModifier
                                        ? QStringLiteral("shift-enter")
                                        : QStringLiteral("enter"));
            event->accept();
            return;
        }
    }
    if (keyboard::isStructureBlockNavigation(semantic_modifiers, event->key())) {
        emit editorKeyRequested(event->key() == Qt::Key_Up ? QStringLiteral("structure-previous")
                                                           : QStringLiteral("structure-next"));
        event->accept();
        return;
    }
    if (semantic_modifiers == Qt::NoModifier || semantic_modifiers == Qt::ShiftModifier) {
        const bool extend = semantic_modifiers == Qt::ShiftModifier;
        switch (event->key()) {
        case Qt::Key_Left:
            emit visualNavigationRequested(false, extend);
            event->accept();
            return;
        case Qt::Key_Right:
            emit visualNavigationRequested(true, extend);
            event->accept();
            return;
        case Qt::Key_Up:
            emit verticalNavigationRequested(-1, extend);
            event->accept();
            return;
        case Qt::Key_Down:
            emit verticalNavigationRequested(1, extend);
            event->accept();
            return;
        case Qt::Key_Home:
            emit lineEdgeNavigationRequested(false, extend);
            event->accept();
            return;
        case Qt::Key_End:
            emit lineEdgeNavigationRequested(true, extend);
            event->accept();
            return;
        default:
            break;
        }
    }
    if (event->key() == Qt::Key_Backspace && semantic_modifiers == Qt::NoModifier) {
        emit editorKeyRequested(QStringLiteral("backspace"));
        event->accept();
        return;
    }
    if (event->key() == Qt::Key_Delete && semantic_modifiers == Qt::NoModifier) {
        emit editorKeyRequested(QStringLiteral("delete"));
        event->accept();
        return;
    }

    const bool group_switch = modifiers.testFlag(Qt::GroupSwitchModifier);
    const QString text = event->text();
    const bool contains_control_character =
        std::any_of(text.cbegin(), text.cend(),
                    [](QChar character) { return character.category() == QChar::Other_Control; });
    const bool altgr_text = modifiers.testFlag(Qt::ControlModifier) &&
                            modifiers.testFlag(Qt::AltModifier) && !text.isEmpty() &&
                            !contains_control_character;
    const bool has_command_modifier =
        modifiers.testFlag(Qt::MetaModifier) ||
        (modifiers.testFlag(Qt::ControlModifier) && !group_switch && !altgr_text);
    if (!has_command_modifier && !text.isEmpty() && !contains_control_character) {
        emit textInputRequested(text);
        event->accept();
        return;
    }
    QQuickItem::keyPressEvent(event);
}

} // namespace caret
