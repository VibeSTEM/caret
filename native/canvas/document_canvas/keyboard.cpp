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
    const Qt::KeyboardModifiers command_modifiers =
        semantic_modifiers & (Qt::ControlModifier | Qt::MetaModifier);
    if (command_modifiers != Qt::NoModifier &&
        (semantic_modifiers & ~(Qt::ControlModifier | Qt::MetaModifier)) == Qt::NoModifier) {
        if (event->key() == Qt::Key_C || event->key() == Qt::Key_V) {
            emit clipboardCommandRequested(event->key() == Qt::Key_C ? QStringLiteral("copy")
                                                                     : QStringLiteral("paste"));
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
        emit structureNavigationRequested(QStringLiteral("expand-selection"));
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
    if (m_command_mode && semantic_modifiers == Qt::NoModifier) {
        QString command_key;
        switch (event->key()) {
        case Qt::Key_Backspace:
            command_key = QStringLiteral("backspace");
            break;
        case Qt::Key_Left:
            command_key = QStringLiteral("cursor-left");
            break;
        case Qt::Key_Right:
            command_key = QStringLiteral("cursor-right");
            break;
        case Qt::Key_Space:
            command_key = QStringLiteral("space");
            break;
        case Qt::Key_Up:
            command_key = QStringLiteral("previous");
            break;
        case Qt::Key_Down:
            command_key = QStringLiteral("next");
            break;
        default:
            break;
        }
        if (!command_key.isEmpty()) {
            emit commandLineKeyRequested(command_key);
            event->accept();
            return;
        }
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
        emit structureNavigationRequested(event->key() == Qt::Key_Up ? QStringLiteral("previous")
                                                                     : QStringLiteral("next"));
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
    if (event->key() == Qt::Key_Backspace && modifiers == Qt::NoModifier) {
        emit editingCommandRequested(QStringLiteral("text.delete_backward"));
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
