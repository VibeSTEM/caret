// 装配多页画布的生命周期与外观属性，具体行为由职责文件实现。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "document_canvas.hpp"

#include <QQuickItem>
#include <qcolor.h>
#include <qtmetamacros.h>

namespace caret {

DocumentCanvas::DocumentCanvas(QQuickItem* parent) : QQuickItem(parent) {
    setFlag(ItemHasContents, true);
    setFlag(ItemIsFocusScope, true);
    setFlag(ItemAcceptsInputMethod, true);
    initializeCaretBlink();
    initializeGeometryNotifications();
    updateGeometry();
}

QColor DocumentCanvas::pageColor() const { return m_page_color; }

void DocumentCanvas::setPageColor(const QColor& color) {
    if (m_page_color == color) {
        return;
    }
    m_page_color = color;
    emit appearanceChanged();
    update();
}

QColor DocumentCanvas::pageBorderColor() const { return m_page_border_color; }

void DocumentCanvas::setPageBorderColor(const QColor& color) {
    if (m_page_border_color == color) {
        return;
    }
    m_page_border_color = color;
    emit appearanceChanged();
    update();
}

} // namespace caret
