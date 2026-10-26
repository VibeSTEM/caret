// 多页纸张绘制与页面坐标映射实现。
#include "document_canvas.hpp"

#include <QRectF>
#include <QSGSimpleRectNode>
#include <QVariantMap>
#include <algorithm>
#include <cmath>

namespace caret {
namespace {

constexpr qreal kPointsToLogicalPixels = 96.0 / 72.0;
constexpr qreal kMinimumZoom = 0.1;
constexpr qreal kMaximumZoom = 8.0;

bool is_finite_positive(qreal value) { return std::isfinite(value) && value > 0.0; }
bool is_finite_non_negative(qreal value) { return std::isfinite(value) && value >= 0.0; }

} // namespace

DocumentCanvas::DocumentCanvas(QQuickItem* parent) : QQuickItem(parent) {
    setFlag(ItemHasContents, true);
    connect(this, &QQuickItem::widthChanged, this, &QQuickItem::update);
    updateGeometry();
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
    const qreal bounded = std::clamp(zoom, kMinimumZoom, kMaximumZoom);
    if (qFuzzyCompare(m_zoom, bounded)) {
        return;
    }
    m_zoom = bounded;
    emit zoomChanged();
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

QVariantMap DocumentCanvas::caretGeometry() const { return m_caret_geometry; }

void DocumentCanvas::setCaretGeometry(const QVariantMap& geometry) {
    if (geometry.isEmpty()) {
        if (m_caret_geometry.isEmpty()) {
            return;
        }
        m_caret_geometry.clear();
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
    emit caretGeometryChanged();
    update();
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

qreal DocumentCanvas::caretWidthPx() const { return m_caret_width_px; }

void DocumentCanvas::setCaretWidthPx(qreal width_px) {
    if (!is_finite_positive(width_px) || qFuzzyCompare(m_caret_width_px, width_px)) {
        return;
    }
    m_caret_width_px = width_px;
    emit caretWidthPxChanged();
    update();
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

qreal DocumentCanvas::scale() const { return kPointsToLogicalPixels * m_zoom; }

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
    update();
    emit geometryChanged();
}

QSGNode* DocumentCanvas::updatePaintNode(QSGNode* old_node, UpdatePaintNodeData*) {
    auto* root = static_cast<QSGNode*>(old_node);
    if (root == nullptr) {
        root = new QSGNode();
    }
    const int page_count = m_page_sizes_pt.size();
    const bool has_caret = !m_caret_geometry.isEmpty();
    const int required_nodes = page_count * 2 + (has_caret ? 1 : 0);
    int node_count = root->childCount();
    while (node_count > required_nodes) {
        delete root->lastChild();
        --node_count;
    }
    while (node_count < required_nodes) {
        root->appendChildNode(new QSGSimpleRectNode());
        ++node_count;
    }

    auto* node = static_cast<QSGSimpleRectNode*>(root->firstChild());
    for (int index = 0; index < page_count; ++index) {
        const QRectF page = pageRect(index);
        node->setRect(page);
        node->setColor(m_page_border_color.isValid() ? m_page_border_color
                                                     : QColor(Qt::transparent));
        node = static_cast<QSGSimpleRectNode*>(node->nextSibling());
        node->setRect(page.adjusted(1.0, 1.0, -1.0, -1.0));
        node->setColor(m_page_color.isValid() ? m_page_color : QColor(Qt::transparent));
        node = static_cast<QSGSimpleRectNode*>(node->nextSibling());
    }
    if (has_caret) {
        const int page_index = m_caret_geometry.value(QStringLiteral("pageIndex")).toInt();
        const qreal x_pt = m_caret_geometry.value(QStringLiteral("xPt")).toReal();
        const qreal y0_pt = m_caret_geometry.value(QStringLiteral("y0Pt")).toReal();
        const qreal y1_pt = m_caret_geometry.value(QStringLiteral("y1Pt")).toReal();
        const QPointF top_left = documentToContent(page_index, QPointF(x_pt, y0_pt));
        const QRectF caret_rect(top_left.x(), top_left.y(), m_caret_width_px,
                                (y1_pt - y0_pt) * scale());
        node->setRect(caret_rect.intersected(pageRect(page_index)));
        node->setColor(m_caret_color.isValid() ? m_caret_color : QColor(Qt::transparent));
    }
    return root;
}

} // namespace caret
