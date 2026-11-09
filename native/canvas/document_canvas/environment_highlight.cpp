// 校验并发布 Rust 提供的当前环境高亮几何。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "document_canvas.hpp"
#include <QColor>
#include <QString>
#include <QVariant>
#include <cmath>
#include <functional>
#include <limits>
#include <qcontainerfwd.h>

namespace caret {
namespace {

bool valid_highlight_rectangles(const QVariantList& rectangles,
                                const std::function<QSizeF(int)>& page_size) {
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
            y1 <= y0) {
            return false;
        }
        const QSizeF size = page_size(static_cast<int>(page));
        if (size.isEmpty() || x1 > size.width() || y1 > size.height()) {
            return false;
        }
    }
    return true;
}

} // namespace

QVariantList DocumentCanvas::blockHighlightRects() const { return m_block_highlight_rects; }

void DocumentCanvas::setBlockHighlightRects(const QVariantList& rectangles) {
    if (!valid_highlight_rectangles(rectangles,
                                    [this](int page_index) { return pageSizePt(page_index); }) ||
        m_block_highlight_rects == rectangles) {
        return;
    }
    m_block_highlight_rects = rectangles;
    ++m_highlight_revision;
    emit blockHighlightRectsChanged();
    update();
}

QVariantList DocumentCanvas::inlineHighlightRects() const { return m_inline_highlight_rects; }

void DocumentCanvas::setInlineHighlightRects(const QVariantList& rectangles) {
    if (!valid_highlight_rectangles(rectangles,
                                    [this](int page_index) { return pageSizePt(page_index); }) ||
        m_inline_highlight_rects == rectangles) {
        return;
    }
    m_inline_highlight_rects = rectangles;
    ++m_highlight_revision;
    emit inlineHighlightRectsChanged();
    update();
}

QColor DocumentCanvas::blockHighlightColor() const { return m_block_highlight_color; }

void DocumentCanvas::setBlockHighlightColor(const QColor& color) {
    if (m_block_highlight_color == color) {
        return;
    }
    m_block_highlight_color = color;
    emit appearanceChanged();
    update();
}

QColor DocumentCanvas::inlineHighlightColor() const { return m_inline_highlight_color; }

void DocumentCanvas::setInlineHighlightColor(const QColor& color) {
    if (m_inline_highlight_color == color) {
        return;
    }
    m_inline_highlight_color = color;
    emit appearanceChanged();
    update();
}

qreal DocumentCanvas::environmentHighlightRadius() const { return m_environment_highlight_radius; }

void DocumentCanvas::setEnvironmentHighlightRadius(qreal radius) {
    if (!std::isfinite(radius) || radius < 0.0 || m_environment_highlight_radius == radius) {
        return;
    }
    m_environment_highlight_radius = radius;
    ++m_highlight_revision;
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
    ++m_highlight_revision;
    emit appearanceChanged();
    update();
}

} // namespace caret
