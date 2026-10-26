// 将 Qt 键盘事件转换为文档输入命令。
#include "document_canvas.hpp"
#include <QKeyEvent>
#include <algorithm>

namespace caret {

void DocumentCanvas::keyPressEvent(QKeyEvent* event) {
    const Qt::KeyboardModifiers modifiers = event->modifiers();
    const Qt::KeyboardModifiers semantic_modifiers = modifiers & ~Qt::KeypadModifier;
    if ((event->key() == Qt::Key_Return || event->key() == Qt::Key_Enter) &&
        (semantic_modifiers == Qt::NoModifier || semantic_modifiers == Qt::ShiftModifier)) {
        emit editingCommandRequested(semantic_modifiers == Qt::ShiftModifier
                                         ? QStringLiteral("text.soft_break")
                                         : QStringLiteral("text.split_paragraph"));
        event->accept();
        return;
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
