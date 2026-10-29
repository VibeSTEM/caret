// 提供跨页面场景图图层共用的坐标转换与可见区域裁剪。
#pragma once

#include <QPointF>
#include <QRectF>
#include <QVariantMap>

namespace caret::canvas_detail {

template <typename DocumentToContent, typename PageRect>
QRectF visible_canvas_rect(int page_index, const QVariantMap& rectangle,
                           DocumentToContent&& document_to_content, PageRect&& page_rect) {
    const QPointF top_left =
        document_to_content(page_index, QPointF(rectangle.value(QStringLiteral("x0Pt")).toReal(),
                                                rectangle.value(QStringLiteral("y0Pt")).toReal()));
    const QPointF bottom_right =
        document_to_content(page_index, QPointF(rectangle.value(QStringLiteral("x1Pt")).toReal(),
                                                rectangle.value(QStringLiteral("y1Pt")).toReal()));
    return QRectF(top_left, bottom_right).normalized().intersected(page_rect(page_index));
}

} // namespace caret::canvas_detail
