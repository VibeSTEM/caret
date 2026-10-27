// 将 Qt 键盘事件转换为文档输入命令。
#include "document_canvas.hpp"
#include <QChar>
#include <QGuiApplication>
#include <QInputMethod>
#include <QInputMethodEvent>
#include <QKeyEvent>
#include <QQuickItem>
#include <QString>
#include <QVariant>
#include <Qt>
#include <QtCore/qobjectdefs.h>
#include <QtCore/qtmetamacros.h>
#include <algorithm>
#include <cmath>
#include <limits>
#include <qcontainerfwd.h>

namespace caret {

QVariantMap DocumentCanvas::inputMethodContext() const { return m_input_method_context; }

void DocumentCanvas::setInputMethodContext(const QVariantMap& context) {
    if (m_input_method_context == context) {
        return;
    }
    m_input_method_context = context;
    emit inputMethodContextChanged();
    if (QGuiApplication::inputMethod() != nullptr) {
        QGuiApplication::inputMethod()->update(Qt::ImQueryInput);
    }
}

QString DocumentCanvas::preeditText() const { return m_preedit_text; }
int DocumentCanvas::preeditCursorPosition() const { return m_preedit_cursor_position; }
void DocumentCanvas::updateInputMethodCursorRectangle() {
    if (QGuiApplication::inputMethod() != nullptr) {
        QGuiApplication::inputMethod()->update(Qt::ImCursorRectangle);
    }
}
void DocumentCanvas::cancelPreedit() {
    if (!m_preedit_text.isEmpty() || m_preedit_cursor_position != 0) {
        m_preedit_text.clear();
        m_preedit_cursor_position = 0;
        emit preeditTextChanged();
    }
    emit inputMethodEventRequested(QString(), QString(), 0, 0, 0);
}

void DocumentCanvas::inputMethodEvent(QInputMethodEvent* event) {
    const QString next_preedit = event->preeditString();
    const int maximum_position =
        static_cast<int>(std::min<qsizetype>(next_preedit.size(), std::numeric_limits<int>::max()));
    int next_cursor_position = maximum_position;
    for (const QInputMethodEvent::Attribute& attribute : event->attributes()) {
        if (attribute.type == QInputMethodEvent::Cursor) {
            next_cursor_position = std::clamp(attribute.start, 0, maximum_position);
            break;
        }
    }
    if (m_preedit_text != next_preedit || m_preedit_cursor_position != next_cursor_position) {
        m_preedit_text = next_preedit;
        m_preedit_cursor_position = next_cursor_position;
        emit preeditTextChanged();
        update();
    }
    emit inputMethodEventRequested(next_preedit, event->commitString(), event->replacementStart(),
                                   event->replacementLength(), next_cursor_position);
    event->accept();
}

QVariant DocumentCanvas::inputMethodQuery(Qt::InputMethodQuery query) const {
    switch (query) {
    case Qt::ImEnabled:
        return true;
    case Qt::ImCursorRectangle:
        return caretRect();
    case Qt::ImSurroundingText:
        return m_input_method_context.value(QStringLiteral("surroundingText"));
    case Qt::ImCursorPosition:
        return m_input_method_context.value(QStringLiteral("cursorPosition"));
    case Qt::ImAnchorPosition:
        return m_input_method_context.value(QStringLiteral("anchorPosition"));
    case Qt::ImCurrentSelection: {
        const QString surrounding =
            m_input_method_context.value(QStringLiteral("surroundingText")).toString();
        const int cursor = m_input_method_context.value(QStringLiteral("cursorPosition")).toInt();
        const int anchor = m_input_method_context.value(QStringLiteral("anchorPosition")).toInt();
        return surrounding.mid(std::min(cursor, anchor), std::abs(cursor - anchor));
    }
    default:
        return QQuickItem::inputMethodQuery(query);
    }
}

void DocumentCanvas::keyPressEvent(QKeyEvent* event) {
    const Qt::KeyboardModifiers modifiers = event->modifiers();
    const Qt::KeyboardModifiers semantic_modifiers = modifiers & ~Qt::KeypadModifier;
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
