// 维护页面前缀几何、坐标转换、页面命中和设备像素栅格请求。
#include "canvas_policy.hpp"
#include "document_canvas.hpp"
#include "document_units.hpp"
#include "value_validation.hpp"
#include <QQuickWindow>
#include <QWindow>
#include <algorithm>
#include <cmath>
#include <iterator>
#include <qcontainerfwd.h>
#include <qobject.h>
#include <qpoint.h>
#include <qquickitem.h>
#include <qsize.h>
#include <qstringliteral.h>
#include <qtmetamacros.h>
#include <qvariant.h>
#include <qvectornd.h>
#include <utility>

namespace caret {
namespace {
using canvas_detail::is_finite_non_negative;
using canvas_detail::is_finite_positive;
} // namespace

void DocumentCanvas::initializeGeometryNotifications() {
    connect(this, &QQuickItem::widthChanged, this, [this]() {
        ++m_glyph_geometry_revision;
        update();
        updateInputMethodCursorRectangle();
    });
    connect(this, &DocumentCanvas::geometryChanged, this,
            &DocumentCanvas::updateInputMethodCursorRectangle);
    connect(this, &QQuickItem::windowChanged, this, [this](QQuickWindow* target_window) {
        if (target_window == nullptr) {
            return;
        }
        const auto notify_raster_metrics = [this]() { emit glyphRasterMetricsChanged(); };
        connect(target_window, &QWindow::screenChanged, this, notify_raster_metrics);
        connect(target_window, &QWindow::xChanged, this, notify_raster_metrics);
        connect(target_window, &QWindow::yChanged, this, notify_raster_metrics);
    });
}

QVariantList DocumentCanvas::pageSizesPt() const { return m_page_sizes_pt; }

void DocumentCanvas::setPageSizesPt(const QVariantList& page_sizes) {
    QVariantList valid_sizes;
    valid_sizes.reserve(page_sizes.size());
    for (const QVariant& value : page_sizes) {
        const QSizeF size = value.toSizeF();
        if (is_finite_positive(size.width()) && is_finite_positive(size.height())) {
            valid_sizes.push_back(size);
        } else {
            return;
        }
    }
    if (m_page_sizes_pt == valid_sizes) {
        return;
    }
    m_page_sizes_pt = std::move(valid_sizes);
    if (!m_caret_geometry.isEmpty()) {
        m_caret_geometry.clear();
        emit caretGeometryChanged();
    }
    emit pageSizesPtChanged();
    updateGeometry();
}

QVariantMap DocumentCanvas::pageMarginsPt() const { return m_page_margins_pt; }

void DocumentCanvas::setPageMarginsPt(const QVariantMap& margins) {
    QVariantMap valid_margins;
    for (const QString& name : {QStringLiteral("top"), QStringLiteral("right"),
                                QStringLiteral("bottom"), QStringLiteral("left")}) {
        bool converted = false;
        const qreal value = margins.value(name).toDouble(&converted);
        if (!converted || !is_finite_non_negative(value)) {
            return;
        }
        valid_margins.insert(name, value);
    }
    if (m_page_margins_pt == valid_margins) {
        return;
    }
    m_page_margins_pt = std::move(valid_margins);
    emit pageMarginsPtChanged();
}

qreal DocumentCanvas::zoom() const { return m_zoom; }

void DocumentCanvas::setZoom(qreal zoom) {
    if (!is_finite_positive(zoom)) {
        return;
    }
    const qreal bounded =
        std::clamp(zoom, document_units::percent_to_factor(canvas_policy::kMinimumZoomPercent),
                   document_units::percent_to_factor(canvas_policy::kMaximumZoomPercent));
    if (qFuzzyCompare(m_zoom, bounded)) {
        return;
    }
    m_zoom = bounded;
    emit zoomChanged();
    updateGeometry();
}

qreal DocumentCanvas::pageGapPt() const { return m_page_gap_pt; }

void DocumentCanvas::setPageGapPt(qreal gap_pt) {
    if (!is_finite_non_negative(gap_pt) || qFuzzyCompare(m_page_gap_pt + 1.0, gap_pt + 1.0)) {
        return;
    }
    m_page_gap_pt = gap_pt;
    emit pageGapPtChanged();
    updateGeometry();
}

qreal DocumentCanvas::pageMarginPt() const { return m_page_margin_pt; }

void DocumentCanvas::setPageMarginPt(qreal margin_pt) {
    if (!is_finite_non_negative(margin_pt) ||
        qFuzzyCompare(m_page_margin_pt + 1.0, margin_pt + 1.0)) {
        return;
    }
    m_page_margin_pt = margin_pt;
    emit pageMarginPtChanged();
    updateGeometry();
}

QSizeF DocumentCanvas::naturalContentSize() const {
    const qreal total_width_pt = m_widest_page_pt + 2.0 * m_page_margin_pt;
    const qreal total_height_pt = m_pages_height_pt + 2.0 * m_page_margin_pt;
    const qreal pixels_per_point = scale();
    return {total_width_pt * pixels_per_point, total_height_pt * pixels_per_point};
}

QPointF DocumentCanvas::pageOrigin(int page_index) const {
    if (page_index < 0 || page_index >= m_page_sizes_pt.size()) {
        return {};
    }
    const QSizeF natural = naturalContentSize();
    const QSizeF page = pageSizePt(page_index);
    const qreal content_extra_width = std::max(0.0, width() - natural.width());
    const qreal x_pt = m_page_margin_pt + (m_widest_page_pt - page.width()) / 2.0;
    return {x_pt * scale() + content_extra_width / 2.0,
            m_page_y_origins_pt.at(page_index) * scale()};
}

QPointF DocumentCanvas::documentToContent(int page_index, const QPointF& point_pt) const {
    if (page_index < 0 || page_index >= m_page_sizes_pt.size() || !std::isfinite(point_pt.x()) ||
        !std::isfinite(point_pt.y())) {
        return {};
    }
    return pageOrigin(page_index) + point_pt * scale();
}

QPointF DocumentCanvas::contentToDocument(int page_index, const QPointF& point_px) const {
    if (page_index < 0 || page_index >= m_page_sizes_pt.size() || !std::isfinite(point_px.x()) ||
        !std::isfinite(point_px.y())) {
        return {};
    }
    return (point_px - pageOrigin(page_index)) / scale();
}

QVariantMap DocumentCanvas::hitTestPage(const QPointF& point_px) const {
    if (m_page_y_origins_pt.isEmpty()) {
        return {{QStringLiteral("pageIndex"), -1}};
    }
    if (!std::isfinite(point_px.x()) || !std::isfinite(point_px.y())) {
        return {{QStringLiteral("pageIndex"), -1}};
    }
    const qreal y_pt = point_px.y() / scale();
    const auto after =
        std::upper_bound(m_page_y_origins_pt.cbegin(), m_page_y_origins_pt.cend(), y_pt);
    if (after == m_page_y_origins_pt.cbegin()) {
        return {{QStringLiteral("pageIndex"), -1}};
    }
    const int index = static_cast<int>(std::distance(m_page_y_origins_pt.cbegin(), after) - 1);
    const QRectF rect = pageRect(index);
    if (rect.contains(point_px)) {
        const QPointF point_pt = (point_px - rect.topLeft()) / scale();
        QVariantMap hit{{QStringLiteral("pageIndex"), index},
                        {QStringLiteral("xPt"), point_pt.x()},
                        {QStringLiteral("yPt"), point_pt.y()}};
        if (m_page_margins_pt.size() == 4) {
            hit.insert(QStringLiteral("contentXPt"),
                       point_pt.x() - m_page_margins_pt.value(QStringLiteral("left")).toDouble());
            hit.insert(QStringLiteral("contentYPt"),
                       point_pt.y() - m_page_margins_pt.value(QStringLiteral("top")).toDouble());
        }
        return hit;
    }
    return {{QStringLiteral("pageIndex"), -1}};
}

QVariantMap DocumentCanvas::glyphRasterRequest(int page_index) const {
    QQuickWindow* target_window = window();
    if (target_window == nullptr || pageSizePt(page_index).isEmpty()) {
        return {{QStringLiteral("valid"), false}};
    }
    const qreal dpr = target_window->effectiveDevicePixelRatio();
    if (!is_finite_positive(dpr)) {
        return {{QStringLiteral("valid"), false}};
    }
    const QPointF page_origin = documentToContent(page_index, QPointF(0.0, 0.0));
    const QPointF window_origin = mapToItem(target_window->contentItem(), page_origin);
    const qreal px_per_pt = scale() * dpr;
    if (!std::isfinite(window_origin.x()) || !std::isfinite(window_origin.y()) ||
        !is_finite_positive(px_per_pt)) {
        return {{QStringLiteral("valid"), false}};
    }
    return {{QStringLiteral("valid"), true},
            {QStringLiteral("pxPerPt"), px_per_pt},
            {QStringLiteral("pageOriginDeviceXPx"), window_origin.x() * dpr},
            {QStringLiteral("pageOriginDeviceYPx"), window_origin.y() * dpr}};
}

qreal DocumentCanvas::scale() const { return document_units::logical_pixels_per_point() * m_zoom; }

QSizeF DocumentCanvas::pageSizePt(int page_index) const {
    if (page_index < 0 || page_index >= m_page_sizes_pt.size()) {
        return {};
    }
    return m_page_sizes_pt.at(page_index).toSizeF();
}

QRectF DocumentCanvas::pageRect(int page_index) const {
    const QSizeF size_pt = pageSizePt(page_index);
    if (size_pt.isEmpty()) {
        return {};
    }
    return {pageOrigin(page_index), size_pt * scale()};
}

void DocumentCanvas::updateGeometry() {
    m_widest_page_pt = 0.0;
    m_pages_height_pt = 0.0;
    m_page_y_origins_pt.clear();
    m_page_y_origins_pt.reserve(m_page_sizes_pt.size());
    for (int index = 0; index < m_page_sizes_pt.size(); ++index) {
        const QSizeF page = pageSizePt(index);
        m_widest_page_pt = std::max(m_widest_page_pt, page.width());
        m_page_y_origins_pt.push_back(m_page_margin_pt + m_pages_height_pt);
        m_pages_height_pt += page.height();
        if (index + 1 < m_page_sizes_pt.size()) {
            m_pages_height_pt += m_page_gap_pt;
        }
    }
    const QSizeF size = naturalContentSize();
    setImplicitSize(size.width(), size.height());
    ++m_glyph_geometry_revision;
    update();
    emit geometryChanged();
}

void DocumentCanvas::itemChange(ItemChange change, const ItemChangeData& data) {
    QQuickItem::itemChange(change, data);
    if (change == ItemDevicePixelRatioHasChanged) {
        ++m_glyph_geometry_revision;
        emit glyphRasterMetricsChanged();
        update();
    }
}

} // namespace caret
