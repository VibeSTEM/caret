// 在场景图线程维护纸张、字形纹理和插入点节点，并复用未变化页面。
#include "document_canvas.hpp"
#include "value_validation.hpp"
#include <QByteArray>
#include <QColor>
#include <QHash>
#include <QImage>
#include <QList>
#include <QMap>
#include <QQuickWindow>
#include <QRectF>
#include <QSGImageNode>
#include <QSGSimpleRectNode>
#include <QSGTexture>
#include <QSet>
#include <QVector>
#include <QtAlgorithms>
#include <algorithm>
#include <cmath>
#include <functional>
#include <limits>
#include <qcontainerfwd.h>
#include <qhashfunctions.h>
#include <qnamespace.h>
#include <qpoint.h>
#include <utility>

namespace caret {
namespace {
using canvas_detail::is_finite_positive;

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
        images.clear();
    }

    QMap<QString, QVariant> frame;
    QVariantList images;
    QVector<QSGTexture*> textures;
};

class GlyphGroupNode final : public QSGNode {
  public:
    QHash<int, PageGlyphNode*> pages;
};

bool has_retryable_missing_texture(const PageGlyphNode* page_node) {
    const QVariantList& images = page_node->images;
    for (qsizetype index = 0; index < images.size(); ++index) {
        if (index < static_cast<qsizetype>(page_node->textures.size()) &&
            page_node->textures.at(static_cast<int>(index)) != nullptr) {
            continue;
        }
        const QVariantMap image = images.at(index).toMap();
        const int width = image.value(QStringLiteral("width")).toInt();
        const int height = image.value(QStringLiteral("height")).toInt();
        const QByteArray pixels = image.value(QStringLiteral("pixels")).toByteArray();
        const quint64 expected_size = static_cast<quint64>(std::max(width, 0)) *
                                      static_cast<quint64>(std::max(height, 0)) * 4U;
        if (width > 0 && height > 0 && width <= std::numeric_limits<int>::max() / 4 &&
            expected_size == static_cast<quint64>(pixels.size())) {
            return true;
        }
    }
    return false;
}

bool has_retryable_missing_texture(const GlyphGroupNode* group) {
    return std::any_of(
        group->pages.cbegin(), group->pages.cend(),
        [](const PageGlyphNode* page_node) { return has_retryable_missing_texture(page_node); });
}

class PageBackgroundNode final : public QSGNode {
  public:
    PageBackgroundNode() {
        border = new QSGSimpleRectNode();
        page = new QSGSimpleRectNode();
        appendChildNode(border);
        appendChildNode(page);
    }

    QSGSimpleRectNode* border = nullptr;
    QSGSimpleRectNode* page = nullptr;
};

class PageBackgroundGroupNode final : public QSGNode {
  public:
    QHash<int, PageBackgroundNode*> pages;
};

class SelectionGroupNode final : public QSGNode {
  public:
    void clearRectangles() {
        while (firstChild() != nullptr) {
            QSGNode* child = firstChild();
            removeChildNode(child);
            delete child;
        }
        rectangles.clear();
    }

    QVector<QSGSimpleRectNode*> rectangles;
};

class CanvasRootNode final : public QSGNode {
  public:
    int page_count = -1;
    PageBackgroundGroupNode* page_background_group = nullptr;
    GlyphGroupNode* glyph_group = nullptr;
    SelectionGroupNode* selection_group = nullptr;
    QSGSimpleRectNode* caret_node = nullptr;
    quint64 glyph_revision = std::numeric_limits<quint64>::max();
    quint64 glyph_geometry_revision = std::numeric_limits<quint64>::max();
    quint64 selection_revision = std::numeric_limits<quint64>::max();
    quint64 selection_geometry_revision = std::numeric_limits<quint64>::max();
};

void update_page_backgrounds(PageBackgroundGroupNode* group, int page_count, int first_page,
                             int page_end, const std::function<QRectF(int)>& page_rect,
                             const QColor& border_color, const QColor& page_color) {
    first_page = std::clamp(first_page, 0, page_count);
    page_end = std::clamp(page_end, first_page, page_count);

    const QList<int> existing_pages = group->pages.keys();
    for (const int page_index : existing_pages) {
        if (page_index >= first_page && page_index < page_end) {
            continue;
        }
        PageBackgroundNode* page_node = group->pages.take(page_index);
        group->removeChildNode(page_node);
        delete page_node;
    }

    for (int page_index = first_page; page_index < page_end; ++page_index) {
        PageBackgroundNode* page_node = group->pages.value(page_index, nullptr);
        if (page_node == nullptr) {
            page_node = new PageBackgroundNode();
            group->pages.insert(page_index, page_node);
            group->appendChildNode(page_node);
        }
        const QRectF page = page_rect(page_index);
        page_node->border->setRect(page);
        page_node->border->setColor(border_color);
        page_node->page->setRect(page.adjusted(1.0, 1.0, -1.0, -1.0));
        page_node->page->setColor(page_color);
    }
}

struct GlyphRectangles {
    QRectF target;
    QRectF source;
};

QSGImageNode* make_glyph_node(QQuickWindow* window, QSGTexture* texture,
                              const GlyphRectangles& rectangles) {
    if (window == nullptr || texture == nullptr || rectangles.target.isEmpty()) {
        return nullptr;
    }
    QSGImageNode* node = window->createImageNode();
    if (node == nullptr) {
        return nullptr;
    }
    node->setOwnsTexture(false);
    node->setFiltering(QSGTexture::Linear);
    node->setTexture(texture);
    node->setSourceRect(rectangles.source);
    node->setRect(rectangles.target);
    return node;
}

void update_glyph_node(QSGImageNode* node, QSGTexture* texture, const GlyphRectangles& rectangles) {
    if (node == nullptr || texture == nullptr || rectangles.target.isEmpty()) {
        return;
    }
    if (node->ownsTexture()) {
        node->setOwnsTexture(false);
    }
    if (node->filtering() != QSGTexture::Linear) {
        node->setFiltering(QSGTexture::Linear);
    }
    if (node->texture() != texture) {
        node->setTexture(texture);
    }
    if (node->sourceRect() != rectangles.source) {
        node->setSourceRect(rectangles.source);
    }
    if (node->rect() != rectangles.target) {
        node->setRect(rectangles.target);
    }
}

void build_glyph_frame(PageGlyphNode* group, QQuickWindow* window, const QVariantMap& frame,
                       const std::function<QRectF(int)>& page_rect,
                       const std::function<QPointF(int, const QPointF&)>& document_to_content,
                       qreal logical_scale, bool create_textures) {
    if (window == nullptr || !frame.value(QStringLiteral("valid")).toBool()) {
        group->clearGlyphNodes();
        return;
    }
    const int page_index = frame.value(QStringLiteral("pageIndex")).toInt();
    const QList<QVariant> image_values = frame.value(QStringLiteral("images")).toList();
    const QList<QVariant> placement_values = frame.value(QStringLiteral("placements")).toList();
    const qreal dpr = window->effectiveDevicePixelRatio();
    const qreal raster_pixels_per_point = frame.value(QStringLiteral("rasterPxPerPt")).toReal();
    const QRectF page_clip_rect = page_rect(page_index);
    if (page_clip_rect.isEmpty() || !is_finite_positive(dpr) ||
        !is_finite_positive(raster_pixels_per_point) || image_values.isEmpty()) {
        group->clearGlyphNodes();
        return;
    }
    const qreal image_to_logical_scale = logical_scale / raster_pixels_per_point;

    if (create_textures) {
        QVector<QSGTexture*> previous_textures = std::move(group->textures);
        const QVariantList previous_images = std::move(group->images);
        group->textures.reserve(image_values.size());
        group->images.reserve(image_values.size());
        for (qsizetype index = 0; index < image_values.size(); ++index) {
            const QVariant& value = image_values.at(index);
            if (index < previous_images.size() &&
                index < static_cast<qsizetype>(previous_textures.size()) &&
                previous_images.at(index) == value &&
                previous_textures[static_cast<int>(index)] != nullptr) {
                const int texture_index = static_cast<int>(index);
                group->textures.push_back(previous_textures[texture_index]);
                previous_textures[texture_index] = nullptr;
                group->images.push_back(value);
                continue;
            }
            const QVariantMap image = value.toMap();
            const int width = image.value(QStringLiteral("width")).toInt();
            const int height = image.value(QStringLiteral("height")).toInt();
            const QByteArray pixels = image.value(QStringLiteral("pixels")).toByteArray();
            const quint64 expected_size = static_cast<quint64>(std::max(width, 0)) *
                                          static_cast<quint64>(std::max(height, 0)) * 4U;
            if (width <= 0 || height <= 0 || expected_size != static_cast<quint64>(pixels.size()) ||
                width > std::numeric_limits<int>::max() / 4) {
                group->textures.push_back(nullptr);
                group->images.push_back(value);
                continue;
            }
            const QImage image_data(reinterpret_cast<const uchar*>(pixels.constData()), width,
                                    height, static_cast<qsizetype>(width) * 4,
                                    QImage::Format_RGBA8888_Premultiplied);
            QSGTexture* texture =
                window->createTextureFromImage(image_data.copy(), QQuickWindow::TextureCanUseAtlas);
            group->textures.push_back(texture);
            group->images.push_back(value);
        }
        qDeleteAll(previous_textures);
    }

    QSGNode* reusable_node = group->firstChild();
    for (const QVariant& value : placement_values) {
        const QVariantMap placement = value.toMap();
        const quint32 image_index = placement.value(QStringLiteral("imageIndex")).toUInt();
        if (image_index >= static_cast<quint32>(group->textures.size()) ||
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
        const GlyphRectangles rectangles{visible_rect, source_rect};
        auto* glyph_node = static_cast<QSGImageNode*>(reusable_node);
        if (reusable_node != nullptr) {
            reusable_node = reusable_node->nextSibling();
        }
        if (glyph_node == nullptr) {
            glyph_node =
                make_glyph_node(window, group->textures[static_cast<int>(image_index)], rectangles);
            if (glyph_node != nullptr) {
                group->appendChildNode(glyph_node);
            }
        } else {
            update_glyph_node(glyph_node, group->textures[static_cast<int>(image_index)],
                              rectangles);
        }
    }
    while (reusable_node != nullptr) {
        QSGNode* next_node = reusable_node->nextSibling();
        group->removeChildNode(reusable_node);
        delete reusable_node;
        reusable_node = next_node;
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
        const bool missing_texture = has_retryable_missing_texture(page_node);
        if (frame_changed) {
            const QVariantList next_images = frame.value(QStringLiteral("images")).toList();
            if (next_images.isEmpty() && !page_node->images.isEmpty()) {
                page_node->clearGlyphResources();
            }
            page_node->frame = frame_identity;
        } else if (frames_changed && !geometry_changed && !missing_texture) {
            continue;
        }
        const bool images_changed =
            page_node->images != frame.value(QStringLiteral("images")).toList();
        build_glyph_frame(page_node, window, frame, page_rect, document_to_content, logical_scale,
                          images_changed || missing_texture);
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

QSGNode* DocumentCanvas::updatePaintNode(QSGNode* old_node, UpdatePaintNodeData*) {
    auto* root = static_cast<CanvasRootNode*>(old_node);
    const qsizetype page_count_size = m_page_sizes_pt.size();
    if (page_count_size > std::numeric_limits<int>::max()) {
        delete old_node;
        return nullptr;
    }
    const int page_count = static_cast<int>(page_count_size);
    if (root == nullptr || root->page_count != page_count) {
        PageBackgroundGroupNode* page_background_group =
            root != nullptr ? root->page_background_group : nullptr;
        GlyphGroupNode* glyph_group = root != nullptr ? root->glyph_group : nullptr;
        SelectionGroupNode* selection_group = root != nullptr ? root->selection_group : nullptr;
        QSGSimpleRectNode* caret_node = root != nullptr ? root->caret_node : nullptr;
        if (root != nullptr) {
            root->removeChildNode(page_background_group);
            root->removeChildNode(glyph_group);
            root->removeChildNode(selection_group);
            root->removeChildNode(caret_node);
        }
        delete root;
        root = new CanvasRootNode();
        root->page_background_group = page_background_group != nullptr
                                          ? page_background_group
                                          : new PageBackgroundGroupNode();
        root->selection_group =
            selection_group != nullptr ? selection_group : new SelectionGroupNode();
        root->glyph_group = glyph_group != nullptr ? glyph_group : new GlyphGroupNode();
        root->caret_node = caret_node != nullptr ? caret_node : new QSGSimpleRectNode();
        root->appendChildNode(root->page_background_group);
        root->appendChildNode(root->selection_group);
        root->appendChildNode(root->glyph_group);
        root->appendChildNode(root->caret_node);
        root->page_count = page_count;
    }

    update_page_backgrounds(
        root->page_background_group, page_count, m_first_visible_page,
        m_page_end_visible < 0 ? page_count : m_page_end_visible,
        [this](int page_index) { return pageRect(page_index); },
        m_page_border_color.isValid() ? m_page_border_color : QColor(Qt::transparent),
        m_page_color.isValid() ? m_page_color : QColor(Qt::transparent));
    if (root->selection_revision != m_selection_revision ||
        root->selection_geometry_revision != m_glyph_geometry_revision) {
        root->selection_group->clearRectangles();
        QVariantList selection_rectangles = m_selection_rects;
        selection_rectangles.append(m_selection_preview_rects);
        for (const QVariant& value : selection_rectangles) {
            const QVariantMap selection = value.toMap();
            const int page_index = selection.value(QStringLiteral("pageIndex")).toInt();
            const QPointF top_left = documentToContent(
                page_index, QPointF(selection.value(QStringLiteral("x0Pt")).toReal(),
                                    selection.value(QStringLiteral("y0Pt")).toReal()));
            const QPointF bottom_right = documentToContent(
                page_index, QPointF(selection.value(QStringLiteral("x1Pt")).toReal(),
                                    selection.value(QStringLiteral("y1Pt")).toReal()));
            const QRectF rect(top_left, bottom_right);
            const QRectF visible_rect = rect.normalized().intersected(pageRect(page_index));
            if (visible_rect.isEmpty()) {
                continue;
            }
            auto* rectangle = new QSGSimpleRectNode(visible_rect, m_selection_color);
            root->selection_group->rectangles.push_back(rectangle);
            root->selection_group->appendChildNode(rectangle);
        }
        root->selection_revision = m_selection_revision;
        root->selection_geometry_revision = m_glyph_geometry_revision;
    } else {
        for (QSGSimpleRectNode* rectangle : root->selection_group->rectangles) {
            rectangle->setColor(m_selection_color);
        }
    }
    auto* glyph_group = root->glyph_group;
    const bool missing_glyph_texture = has_retryable_missing_texture(glyph_group);
    if (root->glyph_revision != m_glyph_frame_revision ||
        root->glyph_geometry_revision != m_glyph_geometry_revision || missing_glyph_texture) {
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
    auto* node = root->caret_node;
    const bool preview_caret_visible = hasActiveFocus() && !m_caret_preview_geometry.isEmpty();
    if (preview_caret_visible ||
        (m_caret_visible && hasActiveFocus() && !m_caret_geometry.isEmpty())) {
        const QVariantMap& geometry =
            preview_caret_visible ? m_caret_preview_geometry : m_caret_geometry;
        node->setRect(caretRect(geometry));
        node->setColor(m_caret_color.isValid() ? m_caret_color : QColor(Qt::transparent));
    } else {
        node->setRect({});
        node->setColor(Qt::transparent);
    }
    return root;
}

} // namespace caret
