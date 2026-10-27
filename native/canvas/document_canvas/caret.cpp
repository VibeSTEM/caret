// 管理插入点几何、外观和平台闪烁周期，状态只属于当前画布。
#include "document_canvas.hpp"
#include "document_units.hpp"
#include <QGuiApplication>
#include <QStyleHints>
#include <algorithm>
#include <cmath>
#include <limits>
#include <qcolor.h>
#include <qcontainerfwd.h>
#include <qhashfunctions.h>
#include <qnamespace.h>
#include <qpoint.h>
#include <qquickitem.h>
#include <qsize.h>
#include <qtmetamacros.h>
#include <qvectornd.h>
#include <utility>

namespace caret {
namespace {
constexpr int kCaretBlinkPhasesPerCycle = 2;
constexpr int kMinimumCaretBlinkIntervalMs = 1;
} // namespace

void DocumentCanvas::initializeCaretBlink() {
    m_caret_blink_timer.setTimerType(Qt::CoarseTimer);
    connect(&m_caret_blink_timer, &QTimer::timeout, this, &DocumentCanvas::toggleCaretVisibility);
    connect(this, &QQuickItem::activeFocusChanged, this, &DocumentCanvas::refreshCaretBlink);
    if (const QStyleHints* style_hints = QGuiApplication::styleHints()) {
        connect(style_hints, &QStyleHints::cursorFlashTimeChanged, this,
                [this](int) { refreshCaretBlink(); });
    }
}

QVariantMap DocumentCanvas::caretGeometry() const { return m_caret_geometry; }

void DocumentCanvas::setCaretGeometry(const QVariantMap& geometry) {
    if (geometry.isEmpty()) {
        if (m_caret_geometry.isEmpty()) {
            return;
        }
        m_caret_geometry.clear();
        refreshCaretBlink();
        emit caretGeometryChanged();
        update();
        return;
    }

    bool page_valid = false;
    bool x_valid = false;
    bool y0_valid = false;
    bool y1_valid = false;
    const double page_index_number =
        geometry.value(QStringLiteral("pageIndex")).toDouble(&page_valid);
    const qreal x_pt = geometry.value(QStringLiteral("xPt")).toDouble(&x_valid);
    const qreal y0_pt = geometry.value(QStringLiteral("y0Pt")).toDouble(&y0_valid);
    const qreal y1_pt = geometry.value(QStringLiteral("y1Pt")).toDouble(&y1_valid);
    if (!page_valid || !std::isfinite(page_index_number) || page_index_number < 0.0 ||
        page_index_number > std::numeric_limits<int>::max() ||
        std::floor(page_index_number) != page_index_number) {
        return;
    }
    const int page_index = static_cast<int>(page_index_number);
    const QSizeF page_size = pageSizePt(page_index);
    if (!page_valid || !x_valid || !y0_valid || !y1_valid || page_size.isEmpty() ||
        !std::isfinite(x_pt) || !std::isfinite(y0_pt) || !std::isfinite(y1_pt) || x_pt < 0.0 ||
        x_pt > page_size.width() || y0_pt < 0.0 || y1_pt <= y0_pt) {
        return;
    }
    QVariantMap normalized_geometry = geometry;
    normalized_geometry.insert(QStringLiteral("pageIndex"), page_index);
    if (m_caret_geometry == normalized_geometry) {
        return;
    }
    m_caret_geometry = std::move(normalized_geometry);
    refreshCaretBlink();
    emit caretGeometryChanged();
    update();
    if (QGuiApplication::inputMethod() != nullptr) {
        QGuiApplication::inputMethod()->update(Qt::ImCursorRectangle);
    }
}

bool DocumentCanvas::caretBlinkEnabled() const { return m_caret_blink_enabled; }

void DocumentCanvas::setCaretBlinkEnabled(bool enabled) {
    if (m_caret_blink_enabled == enabled) {
        return;
    }
    m_caret_blink_enabled = enabled;
    refreshCaretBlink();
    emit caretBlinkEnabledChanged();
}

QColor DocumentCanvas::caretColor() const { return m_caret_color; }

void DocumentCanvas::setCaretColor(const QColor& color) {
    if (m_caret_color == color) {
        return;
    }
    m_caret_color = color;
    emit appearanceChanged();
    update();
}

int DocumentCanvas::caretWidthCentipixels() const { return m_caret_width_centipixels; }

void DocumentCanvas::setCaretWidthCentipixels(int width_centipixels) {
    if (width_centipixels <= 0 || m_caret_width_centipixels == width_centipixels) {
        return;
    }
    m_caret_width_centipixels = width_centipixels;
    emit caretWidthCentipixelsChanged();
    update();
}

void DocumentCanvas::refreshCaretBlink() {
    m_caret_blink_timer.stop();
    m_caret_visible = hasActiveFocus() && !m_caret_geometry.isEmpty();
    if (m_caret_visible && m_caret_blink_enabled) {
        const QStyleHints* style_hints = QGuiApplication::styleHints();
        const int flash_time = style_hints != nullptr ? style_hints->cursorFlashTime() : 0;
        if (flash_time > 0) {
            m_caret_blink_timer.start(
                std::max(kMinimumCaretBlinkIntervalMs, flash_time / kCaretBlinkPhasesPerCycle));
        }
    }
    update();
}

void DocumentCanvas::toggleCaretVisibility() {
    if (!m_caret_blink_enabled || !hasActiveFocus() || m_caret_geometry.isEmpty()) {
        refreshCaretBlink();
        return;
    }
    m_caret_visible = !m_caret_visible;
    update();
}

QRectF DocumentCanvas::caretRect() const {
    const int page_index = m_caret_geometry.value(QStringLiteral("pageIndex")).toInt();
    const qreal x_pt = m_caret_geometry.value(QStringLiteral("xPt")).toReal();
    const qreal y0_pt = m_caret_geometry.value(QStringLiteral("y0Pt")).toReal();
    const qreal y1_pt = m_caret_geometry.value(QStringLiteral("y1Pt")).toReal();
    const QPointF top_left = documentToContent(page_index, QPointF(x_pt, y0_pt));
    const qreal caret_width_px =
        document_units::centipixels_to_logical_pixels(m_caret_width_centipixels);
    const QRectF caret_rect(top_left.x(), top_left.y(), caret_width_px, (y1_pt - y0_pt) * scale());
    return caret_rect.intersected(pageRect(page_index));
}

} // namespace caret
