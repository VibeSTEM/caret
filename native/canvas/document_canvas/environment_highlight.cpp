// 校验并发布 Rust 提供的当前环境高亮几何。
#include "document_canvas.hpp"
#include <QColor>
#include <QString>
#include <QVariant>
#include <cmath>
#include <limits>
#include <qcontainerfwd.h>

namespace caret {

QVariantList DocumentCanvas::environmentHighlightRects() const {
    return m_environment_highlight_rects;
}

void DocumentCanvas::setEnvironmentHighlightRects(const QVariantList& rectangles) {
    for (const QVariant& value : rectangles) {
        const QVariantMap rectangle = value.toMap();
        bool page_ok = false;
        bool x0_ok = false;
        bool y0_ok = false;
        bool x1_ok = false;
        bool y1_ok = false;
        const double page = rectangle.value(QStringLiteral("pageIndex")).toDouble(&page_ok);
        const double x0 = rectangle.value(QStringLiteral("x0Pt")).toDouble(&x0_ok);
        const double y0 = rectangle.value(QStringLiteral("y0Pt")).toDouble(&y0_ok);
        const double x1 = rectangle.value(QStringLiteral("x1Pt")).toDouble(&x1_ok);
        const double y1 = rectangle.value(QStringLiteral("y1Pt")).toDouble(&y1_ok);
        if (!page_ok || !std::isfinite(page) || page < 0.0 ||
            page > std::numeric_limits<int>::max() || std::floor(page) != page || !x0_ok ||
            !y0_ok || !x1_ok || !y1_ok || !std::isfinite(x0) || !std::isfinite(y0) ||
            !std::isfinite(x1) || !std::isfinite(y1) || x0 < 0.0 || y0 < 0.0 || x1 <= x0 ||
            y1 <= y0 || pageSizePt(static_cast<int>(page)).isEmpty() ||
            x1 > pageSizePt(static_cast<int>(page)).width() ||
            y1 > pageSizePt(static_cast<int>(page)).height()) {
            return;
        }
    }
    if (m_environment_highlight_rects == rectangles) {
        return;
    }
    m_environment_highlight_rects = rectangles;
    ++m_environment_highlight_revision;
    emit environmentHighlightRectsChanged();
    update();
}

QColor DocumentCanvas::environmentHighlightColor() const { return m_environment_highlight_color; }

void DocumentCanvas::setEnvironmentHighlightColor(const QColor& color) {
    if (m_environment_highlight_color == color) {
        return;
    }
    m_environment_highlight_color = color;
    emit appearanceChanged();
    update();
}

qreal DocumentCanvas::environmentHighlightRadius() const { return m_environment_highlight_radius; }

void DocumentCanvas::setEnvironmentHighlightRadius(qreal radius) {
    if (!std::isfinite(radius) || radius < 0.0 || m_environment_highlight_radius == radius) {
        return;
    }
    m_environment_highlight_radius = radius;
    ++m_environment_highlight_revision;
    emit appearanceChanged();
    update();
}

qreal DocumentCanvas::environmentHighlightPaddingPx() const {
    return m_environment_highlight_padding_px;
}

void DocumentCanvas::setEnvironmentHighlightPaddingPx(qreal padding_px) {
    if (!std::isfinite(padding_px) || padding_px < 0.0 ||
        m_environment_highlight_padding_px == padding_px) {
        return;
    }
    m_environment_highlight_padding_px = padding_px;
    ++m_environment_highlight_revision;
    emit appearanceChanged();
    update();
}

} // namespace caret
