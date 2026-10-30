// 管理画布接收的 Qt 输入法上下文、预编辑文本和光标查询。
#include "document_canvas.hpp"
#include <QGuiApplication>
#include <QInputMethod>
#include <QInputMethodEvent>
#include <QQuickItem>
#include <QString>
#include <QVariant>
#include <Qt>
#include <QtCore/qobjectdefs.h>
#include <QtCore/qtmetamacros.h>
#include <algorithm>
#include <cstdlib>
#include <limits>
#include <qcontainerfwd.h>

namespace caret {

QVariantMap DocumentCanvas::inputMethodContext() const { return m_input_method_context; }

QRectF DocumentCanvas::inputMethodCursorRectangle() const {
    return m_input_method_cursor_rectangle;
}

void DocumentCanvas::setInputMethodCursorRectangle(const QRectF& rectangle) {
    const QRectF normalized = rectangle.isValid() ? rectangle : QRectF{};
    if (m_input_method_cursor_rectangle == normalized) {
        return;
    }
    m_input_method_cursor_rectangle = normalized;
    emit inputMethodCursorRectangleChanged();
    updateInputMethodCursorRectangle();
}

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
    if (QInputMethod* input_method = QGuiApplication::inputMethod()) {
        input_method->reset();
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
        return m_input_method_cursor_rectangle.isEmpty() ? caretRect(m_caret_geometry)
                                                         : m_input_method_cursor_rectangle;
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

} // namespace caret
