// 提供跨页面场景图图层共用的坐标转换与可见区域裁剪。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#pragma once

#include <QPointF>
#include <QRectF>
#include <QSizeF>
#include <QVariantMap>

namespace caret::canvas_detail {

inline QRectF glyph_image_rect(const QPointF& baseline, const QPointF& image_origin_px,
                               const QSizeF& image_size_px, qreal image_to_logical_scale,
                               const QPointF& pixel_phase, bool pixel_phase_applied) {
    const QPointF applied_phase = pixel_phase_applied ? pixel_phase : QPointF{};
    const QPointF top_left(
        baseline.x() + (image_origin_px.x() - applied_phase.x()) * image_to_logical_scale,
        baseline.y() + (-image_origin_px.y() - applied_phase.y()) * image_to_logical_scale);
    return QRectF(top_left, image_size_px * image_to_logical_scale);
}

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
