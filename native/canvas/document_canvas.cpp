// 多页纸张绘制与页面坐标映射实现。
#include "document_canvas.hpp"

#include <QHash>
#include <QImage>
#include <QMetaType>
#include <QQuickWindow>
#include <QRectF>
#include <QSGImageNode>
#include <QSGSimpleRectNode>
#include <QSGTexture>
#include <QSet>
#include <QVariantMap>
#include <QtAlgorithms>
#include <algorithm>
#include <cmath>
#include <limits>

namespace caret {
namespace {

constexpr qreal kPointsToLogicalPixels = 96.0 / 72.0;
constexpr qreal kMinimumZoom = 0.1;
constexpr qreal kMaximumZoom = 8.0;

bool is_finite_positive(qreal value) { return std::isfinite(value) && value > 0.0; }
bool is_finite_non_negative(qreal value) { return std::isfinite(value) && value >= 0.0; }
bool is_boolean_variant(const QVariant& value) { return value.metaType().id() == QMetaType::Bool; }

class PageGlyphNode final : public QSGNode {
  public:
    ~PageGlyphNode() override { clearGlyphResources(); }

    void clearGlyphNodes() {
        while (firstChild() != nullptr) {
            QSGNode* child = firstChild();
            removeChildNode(child);
            delete child;
        }
    }

    void clearGlyphResources() {
        clearGlyphNodes();
        qDeleteAll(textures);
        textures.clear();
    }

    QVariantMap frame;
    QVector<QSGTexture*> textures;
};

class GlyphGroupNode final : public QSGNode {
  public:
    QHash<int, PageGlyphNode*> pages;
};

class CanvasRootNode final : public QSGNode {
  public:
    int page_count = -1;
    GlyphGroupNode* glyph_group = nullptr;
    QSGSimpleRectNode* caret_node = nullptr;
    quint64 glyph_revision = std::numeric_limits<quint64>::max();
    quint64 glyph_geometry_revision = std::numeric_limits<quint64>::max();
};

QSGImageNode* make_glyph_node(QQuickWindow* window, QSGTexture* texture, const QRectF& rect,
                              const QRectF& source_rect) {
    if (window == nullptr || texture == nullptr || rect.isEmpty()) {
        return nullptr;
    }
    QSGImageNode* node = window->createImageNode();
    if (node == nullptr) {
        return nullptr;
    }
    node->setTexture(texture);
    node->setOwnsTexture(false);
    node->setSourceRect(source_rect);
    node->setFiltering(QSGTexture::Linear);
    node->setRect(rect);
    return node;
}

void build_glyph_frame(PageGlyphNode* group, QQuickWindow* window, const QVariantMap& frame,
                       const std::function<QRectF(int)>& page_rect,
                       const std::function<QPointF(int, const QPointF&)>& document_to_content,
                       qreal logical_scale, bool create_textures) {
    if (window == nullptr || !frame.value(QStringLiteral("valid")).toBool()) {
        return;
    }
    const int page_index = frame.value(QStringLiteral("pageIndex")).toInt();
    const QVariantList image_values = frame.value(QStringLiteral("images")).toList();
    const QVariantList placement_values = frame.value(QStringLiteral("placements")).toList();
    const qreal dpr = window->effectiveDevicePixelRatio();
    const qreal raster_pixels_per_point = frame.value(QStringLiteral("rasterPxPerPt")).toReal();
    const QRectF page_clip_rect = page_rect(page_index);
    if (page_clip_rect.isEmpty() || !is_finite_positive(dpr) ||
        !is_finite_positive(raster_pixels_per_point) || image_values.isEmpty()) {
        return;
    }
    const qreal image_to_logical_scale = logical_scale / raster_pixels_per_point;

    if (create_textures) {
        group->textures.reserve(image_values.size());
        for (const QVariant& value : image_values) {
            const QVariantMap image = value.toMap();
            const int width = image.value(QStringLiteral("width")).toInt();
            const int height = image.value(QStringLiteral("height")).toInt();
            const QByteArray pixels = image.value(QStringLiteral("pixels")).toByteArray();
            const quint64 expected_size = static_cast<quint64>(std::max(width, 0)) *
                                          static_cast<quint64>(std::max(height, 0)) * 4U;
            if (width <= 0 || height <= 0 || expected_size != static_cast<quint64>(pixels.size()) ||
                width > std::numeric_limits<int>::max() / 4) {
                group->textures.push_back(nullptr);
                continue;
            }
            const QImage image_data(reinterpret_cast<const uchar*>(pixels.constData()), width,
                                    height, width * 4, QImage::Format_RGBA8888_Premultiplied);
            QSGTexture* texture =
                window->createTextureFromImage(image_data.copy(), QQuickWindow::TextureCanUseAtlas);
            group->textures.push_back(texture);
        }
    }

    for (const QVariant& value : placement_values) {
        const QVariantMap placement = value.toMap();
        const uint image_index = placement.value(QStringLiteral("imageIndex")).toUInt();
        if (image_index >= static_cast<uint>(group->textures.size()) ||
            group->textures[static_cast<int>(image_index)] == nullptr) {
            continue;
        }
        const qreal x_pt = placement.value(QStringLiteral("xPt")).toReal();
        const qreal baseline_y_pt = placement.value(QStringLiteral("baselineYPt")).toReal();
        const qreal phase_x = placement.value(QStringLiteral("pixelPhaseX")).toReal();
        const qreal phase_y = placement.value(QStringLiteral("pixelPhaseY")).toReal();
        if (!std::isfinite(x_pt) || !std::isfinite(baseline_y_pt) || !std::isfinite(phase_x) ||
            !std::isfinite(phase_y)) {
            continue;
        }
        const QVariantMap image = image_values.at(static_cast<qsizetype>(image_index)).toMap();
        const qreal image_left = image.value(QStringLiteral("left")).toReal();
        const qreal image_top = image.value(QStringLiteral("top")).toReal();
        const qreal width_px = image.value(QStringLiteral("width")).toReal();
        const qreal height_px = image.value(QStringLiteral("height")).toReal();
        const bool phase_applied = placement.value(QStringLiteral("pixelPhaseApplied")).toBool();
        const QPointF baseline = document_to_content(page_index, QPointF(x_pt, baseline_y_pt));
        const qreal phase_x_adjustment = phase_applied ? phase_x : 0.0;
        const qreal phase_y_adjustment = phase_applied ? phase_y : 0.0;
        const QRectF glyph_rect(
            baseline.x() + (image_left - phase_x_adjustment) * image_to_logical_scale,
            baseline.y() + (-image_top - phase_y_adjustment) * image_to_logical_scale,
            width_px * image_to_logical_scale, height_px * image_to_logical_scale);
        QRectF clip_rect = page_clip_rect;
        if (placement.value(QStringLiteral("clipEnabled")).toBool()) {
            const qreal clip_y0 = placement.value(QStringLiteral("clipY0Pt")).toReal();
            const qreal clip_y1 = placement.value(QStringLiteral("clipY1Pt")).toReal();
            if (!std::isfinite(clip_y0) || !std::isfinite(clip_y1) || clip_y0 < 0.0 ||
                clip_y1 <= clip_y0) {
                continue;
            }
            const QPointF clip_top = document_to_content(page_index, QPointF(0.0, clip_y0));
            const QRectF line_clip_rect(page_clip_rect.left(), clip_top.y(), page_clip_rect.width(),
                                        (clip_y1 - clip_y0) * logical_scale);
            clip_rect = clip_rect.intersected(line_clip_rect);
        }
        const QRectF visible_rect = glyph_rect.intersected(clip_rect);
        if (visible_rect.isEmpty() || glyph_rect.width() <= 0.0 || glyph_rect.height() <= 0.0) {
            continue;
        }
        const qreal u0 = (visible_rect.left() - glyph_rect.left()) / glyph_rect.width();
        const qreal v0 = (visible_rect.top() - glyph_rect.top()) / glyph_rect.height();
        const qreal u1 = (visible_rect.right() - glyph_rect.left()) / glyph_rect.width();
        const qreal v1 = (visible_rect.bottom() - glyph_rect.top()) / glyph_rect.height();
        const QRectF source_rect(u0 * width_px, v0 * height_px, (u1 - u0) * width_px,
                                 (v1 - v0) * height_px);
        QSGImageNode* glyph_node = make_glyph_node(
            window, group->textures[static_cast<int>(image_index)], visible_rect, source_rect);
        if (glyph_node != nullptr) {
            group->appendChildNode(glyph_node);
        }
    }
}

void build_glyph_frames(GlyphGroupNode* group, QQuickWindow* window, const QVariantList& frames,
                        const std::function<QRectF(int)>& page_rect,
                        const std::function<QPointF(int, const QPointF&)>& document_to_content,
                        qreal logical_scale, bool frames_changed, bool geometry_changed) {
    QSet<int> visible_pages;
    for (const QVariant& value : frames) {
        const QVariantMap frame = value.toMap();
        const int page_index = frame.value(QStringLiteral("pageIndex")).toInt();
        QVariantMap frame_identity = frame;
        frame_identity.remove(QStringLiteral("documentGeneration"));
        frame_identity.remove(QStringLiteral("fontGeneration"));
        visible_pages.insert(page_index);
        PageGlyphNode* page_node = group->pages.value(page_index, nullptr);
        if (page_node == nullptr) {
            page_node = new PageGlyphNode();
            page_node->frame = frame_identity;
            group->pages.insert(page_index, page_node);
            group->appendChildNode(page_node);
            build_glyph_frame(page_node, window, frame, page_rect, document_to_content,
                              logical_scale, true);
            continue;
        }

        const bool frame_changed = page_node->frame != frame_identity;
        if (frame_changed) {
            page_node->clearGlyphResources();
            page_node->frame = frame_identity;
        } else if (frames_changed && !geometry_changed) {
            continue;
        }
        page_node->clearGlyphNodes();
        build_glyph_frame(page_node, window, frame, page_rect, document_to_content, logical_scale,
                          frame_changed);
    }

    const QList<int> existing_pages = group->pages.keys();
    for (const int page_index : existing_pages) {
        if (visible_pages.contains(page_index)) {
            continue;
        }
        PageGlyphNode* page_node = group->pages.take(page_index);
        group->removeChildNode(page_node);
        delete page_node;
    }
}

} // namespace

DocumentCanvas::DocumentCanvas(QQuickItem* parent) : QQuickItem(parent) {
    setFlag(ItemHasContents, true);
    connect(this, &QQuickItem::widthChanged, this, [this]() {
        ++m_glyph_geometry_revision;
        update();
    });
    connect(this, &QQuickItem::windowChanged, this, [this](QQuickWindow* target_window) {
        if (target_window == nullptr) {
            return;
        }
        const auto notify_raster_metrics = [this]() { emit glyphRasterMetricsChanged(); };
        connect(target_window, &QWindow::screenChanged, this, notify_raster_metrics);
        connect(target_window, &QWindow::xChanged, this, notify_raster_metrics);
        connect(target_window, &QWindow::yChanged, this, notify_raster_metrics);
    });
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

QVariantList DocumentCanvas::glyphFrames() const { return m_glyph_frames; }

void DocumentCanvas::setGlyphFrames(const QVariantList& frames) {
    QSet<int> page_indexes;
    for (const QVariant& value : frames) {
        const QVariantMap frame = value.toMap();
        bool page_converted = false;
        const double page_index_value =
            frame.value(QStringLiteral("pageIndex")).toDouble(&page_converted);
        if (!frame.value(QStringLiteral("valid")).toBool() || !page_converted ||
            !std::isfinite(page_index_value) || page_index_value < 0.0 ||
            page_index_value > std::numeric_limits<int>::max() ||
            std::floor(page_index_value) != page_index_value ||
            !is_finite_positive(frame.value(QStringLiteral("rasterPxPerPt")).toDouble()) ||
            !frame.value(QStringLiteral("images")).canConvert<QVariantList>() ||
            !frame.value(QStringLiteral("placements")).canConvert<QVariantList>()) {
            return;
        }
        const int page_index = static_cast<int>(page_index_value);
        const QVariantList images = frame.value(QStringLiteral("images")).toList();
        const QVariantList placements = frame.value(QStringLiteral("placements")).toList();
        if (pageSizePt(page_index).isEmpty() || page_indexes.contains(page_index)) {
            return;
        }
        QList<bool> image_phase_applied;
        image_phase_applied.reserve(images.size());
        for (const QVariant& image_value : images) {
            const QVariantMap image = image_value.toMap();
            bool width_converted = false;
            bool height_converted = false;
            bool left_valid = false;
            bool top_valid = false;
            const double width_value =
                image.value(QStringLiteral("width")).toDouble(&width_converted);
            const double height_value =
                image.value(QStringLiteral("height")).toDouble(&height_converted);
            const double left = image.value(QStringLiteral("left")).toDouble(&left_valid);
            const double top = image.value(QStringLiteral("top")).toDouble(&top_valid);
            const QVariant phase_applied_value = image.value(QStringLiteral("pixelPhaseApplied"));
            const QByteArray pixels = image.value(QStringLiteral("pixels")).toByteArray();
            if (!width_converted || !height_converted || !std::isfinite(width_value) ||
                !std::isfinite(height_value) || width_value < 0.0 || height_value < 0.0 ||
                width_value > std::numeric_limits<int>::max() ||
                height_value > std::numeric_limits<int>::max() ||
                std::floor(width_value) != width_value ||
                std::floor(height_value) != height_value) {
                return;
            }
            const int width = static_cast<int>(width_value);
            const int height = static_cast<int>(height_value);
            const quint64 byte_count = static_cast<quint64>(std::max(width, 0)) *
                                       static_cast<quint64>(std::max(height, 0)) * 4U;
            const bool empty_image = byte_count == 0 && pixels.isEmpty();
            if (width > std::numeric_limits<int>::max() / 4 ||
                (!empty_image && byte_count != static_cast<quint64>(pixels.size())) ||
                !left_valid || !top_valid || !std::isfinite(left) || !std::isfinite(top) ||
                std::floor(left) != left || std::floor(top) != top ||
                left < std::numeric_limits<std::int32_t>::min() ||
                left > std::numeric_limits<std::int32_t>::max() ||
                top < std::numeric_limits<std::int32_t>::min() ||
                top > std::numeric_limits<std::int32_t>::max() ||
                !is_boolean_variant(phase_applied_value) ||
                image.value(QStringLiteral("pixels")).metaType().id() != QMetaType::QByteArray) {
                return;
            }
            image_phase_applied.push_back(phase_applied_value.toBool());
        }
        for (const QVariant& placement_value : placements) {
            const QVariantMap placement = placement_value.toMap();
            bool image_index_valid = false;
            const double image_index_value =
                placement.value(QStringLiteral("imageIndex")).toDouble(&image_index_valid);
            bool x_valid = false;
            bool baseline_valid = false;
            bool phase_x_valid = false;
            bool phase_y_valid = false;
            const double x_pt = placement.value(QStringLiteral("xPt")).toDouble(&x_valid);
            const double baseline_y_pt =
                placement.value(QStringLiteral("baselineYPt")).toDouble(&baseline_valid);
            const double phase_x =
                placement.value(QStringLiteral("pixelPhaseX")).toDouble(&phase_x_valid);
            const double phase_y =
                placement.value(QStringLiteral("pixelPhaseY")).toDouble(&phase_y_valid);
            const QVariant phase_applied_value =
                placement.value(QStringLiteral("pixelPhaseApplied"));
            const QVariant clip_enabled_value = placement.value(QStringLiteral("clipEnabled"));
            const bool clip_enabled = clip_enabled_value.toBool();
            const double clip_y0 = placement.value(QStringLiteral("clipY0Pt")).toDouble();
            const double clip_y1 = placement.value(QStringLiteral("clipY1Pt")).toDouble();
            if (!image_index_valid || !std::isfinite(image_index_value) ||
                image_index_value < 0.0 || image_index_value >= images.size() ||
                std::floor(image_index_value) != image_index_value || !x_valid || !baseline_valid ||
                !phase_x_valid || !phase_y_valid || !std::isfinite(x_pt) ||
                !std::isfinite(baseline_y_pt) || !std::isfinite(phase_x) ||
                !std::isfinite(phase_y) || phase_x < 0.0 || phase_x >= 1.0 || phase_y < 0.0 ||
                phase_y >= 1.0 || !is_boolean_variant(phase_applied_value) ||
                !is_boolean_variant(clip_enabled_value) ||
                phase_applied_value.toBool() != image_phase_applied[image_index_value] ||
                (clip_enabled && (!std::isfinite(clip_y0) || !std::isfinite(clip_y1) ||
                                  clip_y0 < 0.0 || clip_y1 <= clip_y0))) {
                return;
            }
        }
        page_indexes.insert(page_index);
    }
    if (m_glyph_frames == frames) {
        return;
    }
    m_glyph_frames = frames;
    ++m_glyph_frame_revision;
    emit glyphFramesChanged();
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

QSGNode* DocumentCanvas::updatePaintNode(QSGNode* old_node, UpdatePaintNodeData*) {
    auto* root = static_cast<CanvasRootNode*>(old_node);
    const int page_count = m_page_sizes_pt.size();
    if (root == nullptr || root->page_count != page_count) {
        GlyphGroupNode* glyph_group = root != nullptr ? root->glyph_group : nullptr;
        QSGSimpleRectNode* caret_node = root != nullptr ? root->caret_node : nullptr;
        if (root != nullptr) {
            root->removeChildNode(glyph_group);
            root->removeChildNode(caret_node);
        }
        delete root;
        root = new CanvasRootNode();
        for (int index = 0; index < page_count; ++index) {
            root->appendChildNode(new QSGSimpleRectNode());
            root->appendChildNode(new QSGSimpleRectNode());
        }
        root->glyph_group = glyph_group != nullptr ? glyph_group : new GlyphGroupNode();
        root->caret_node = caret_node != nullptr ? caret_node : new QSGSimpleRectNode();
        root->appendChildNode(root->glyph_group);
        root->appendChildNode(root->caret_node);
        root->page_count = page_count;
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
    auto* glyph_group = root->glyph_group;
    if (root->glyph_revision != m_glyph_frame_revision ||
        root->glyph_geometry_revision != m_glyph_geometry_revision) {
        const bool glyph_frames_changed = root->glyph_revision != m_glyph_frame_revision;
        build_glyph_frames(
            glyph_group, window(), m_glyph_frames,
            [this](int page_index) { return pageRect(page_index); },
            [this](int page_index, const QPointF& point) {
                return documentToContent(page_index, point);
            },
            scale(), glyph_frames_changed,
            root->glyph_geometry_revision != m_glyph_geometry_revision);
        root->glyph_revision = m_glyph_frame_revision;
        root->glyph_geometry_revision = m_glyph_geometry_revision;
    }
    node = root->caret_node;
    if (!m_caret_geometry.isEmpty()) {
        const int page_index = m_caret_geometry.value(QStringLiteral("pageIndex")).toInt();
        const qreal x_pt = m_caret_geometry.value(QStringLiteral("xPt")).toReal();
        const qreal y0_pt = m_caret_geometry.value(QStringLiteral("y0Pt")).toReal();
        const qreal y1_pt = m_caret_geometry.value(QStringLiteral("y1Pt")).toReal();
        const QPointF top_left = documentToContent(page_index, QPointF(x_pt, y0_pt));
        const QRectF caret_rect(top_left.x(), top_left.y(), m_caret_width_px,
                                (y1_pt - y0_pt) * scale());
        node->setRect(caret_rect.intersected(pageRect(page_index)));
        node->setColor(m_caret_color.isValid() ? m_caret_color : QColor(Qt::transparent));
    } else {
        node->setRect({});
        node->setColor(Qt::transparent);
    }
    return root;
}

} // namespace caret
