// 校验并发布 Rust 提供的页面选区几何。
#include "document_canvas.hpp"
#include <QVariant>
#include <cmath>
#include <limits>
#include <qtmetamacros.h>

namespace caret {

QVariantList DocumentCanvas::selectionRects() const { return m_selection_rects; }

void DocumentCanvas::setSelectionRects(const QVariantList& rectangles) {
    for (const QVariant& value : rectangles) {
        const QVariantMap rectangle = value.toMap();
        bool page_ok = false;
        const double page_value = rectangle.value(QStringLiteral("pageIndex")).toDouble(&page_ok);
        bool x0_ok = false;
        bool y0_ok = false;
        bool x1_ok = false;
        bool y1_ok = false;
        const double x0 = rectangle.value(QStringLiteral("x0Pt")).toDouble(&x0_ok);
        const double y0 = rectangle.value(QStringLiteral("y0Pt")).toDouble(&y0_ok);
        const double x1 = rectangle.value(QStringLiteral("x1Pt")).toDouble(&x1_ok);
        const double y1 = rectangle.value(QStringLiteral("y1Pt")).toDouble(&y1_ok);
        if (!page_ok || !std::isfinite(page_value) || page_value < 0.0 ||
            page_value > std::numeric_limits<int>::max() || std::floor(page_value) != page_value ||
            !x0_ok || !y0_ok || !x1_ok || !y1_ok || !std::isfinite(x0) || !std::isfinite(y0) ||
            !std::isfinite(x1) || !std::isfinite(y1) || x0 < 0.0 || y0 < 0.0 || x1 <= x0 ||
            y1 <= y0 || pageSizePt(static_cast<int>(page_value)).isEmpty() ||
            x1 > pageSizePt(static_cast<int>(page_value)).width() ||
            y1 > pageSizePt(static_cast<int>(page_value)).height()) {
            return;
        }
    }
    if (m_selection_rects == rectangles) {
        return;
    }
    m_selection_rects = rectangles;
    ++m_selection_revision;
    emit selectionRectsChanged();
    update();
}

QVariantList DocumentCanvas::selectionPreviewRects() const { return m_selection_preview_rects; }

void DocumentCanvas::setSelectionPreviewRects(const QVariantList& rectangles) {
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
            y1 <= y0 || pageSizePt(static_cast<int>(page)).isEmpty()) {
            return;
        }
    }
    if (m_selection_preview_rects == rectangles) {
        return;
    }
    m_selection_preview_rects = rectangles;
    ++m_selection_revision;
    emit selectionPreviewRectsChanged();
    update();
}

QColor DocumentCanvas::selectionColor() const { return m_selection_color; }

void DocumentCanvas::setSelectionColor(const QColor& color) {
    if (m_selection_color == color) {
        return;
    }
    m_selection_color = color;
    emit appearanceChanged();
    update();
}

} // namespace caret
