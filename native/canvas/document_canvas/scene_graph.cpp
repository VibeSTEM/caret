// 在场景图线程维护纸张、字形纹理和插入点节点，并复用未变化页面。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "document_canvas.hpp"
#include "environment_highlight_scene.hpp"
#include "scene_graph_geometry.hpp"
#include "value_validation.hpp"
#include <QByteArray>
#include <QColor>
#include <QHash>
#include <QImage>
#include <QList>
#include <QMap>
#include <QMatrix4x4>
#include <QMetaObject>
#include <QQuickWindow>
#include <QRectF>
#include <QSGClipNode>
#include <QSGGeometry>
#include <QSGImageNode>
#include <QSGSimpleRectNode>
#include <QSGTexture>
#include <QSGTransformNode>
#include <QSet>
#include <QVector>
#include <QtAlgorithms>
#include <QtMath>
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
    PageGlyphNode() {
        draw_nodes = new QSGNode();
        appendChildNode(draw_nodes);
    }

    ~PageGlyphNode() override { clearGlyphResources(); }

    void clearGlyphNodes() { clearNodeChildren(draw_nodes); }

    void clearGlyphResources() {
        clearGlyphNodes();
        qDeleteAll(textures);
        textures.clear();
        images.clear();
    }

    QMap<QString, QVariant> frame;
    QVariantList images;
    QVector<QSGTexture*> textures;
    QSGNode* draw_nodes = nullptr;
    int texture_retry_count = 0;
    QVariantMap retry_frame;
    qreal logical_scale = 0.0;
    QRectF page_clip_rect;
    bool has_committed_frame = false;

  private:
    static void clearNodeChildren(QSGNode* node) {
        while (node != nullptr && node->firstChild() != nullptr) {
            QSGNode* child = node->firstChild();
            node->removeChildNode(child);
            delete child;
        }
    }
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

template <typename RectangleNode> class RectangleGroupNode final : public QSGNode {
  public:
    void clearRectangles() {
        while (firstChild() != nullptr) {
            QSGNode* child = firstChild();
            removeChildNode(child);
            delete child;
        }
        rectangles.clear();
    }

    QVector<RectangleNode*> rectangles;
};

using SelectionGroupNode = RectangleGroupNode<QSGSimpleRectNode>;
using EnvironmentHighlightGroupNode = EnvironmentHighlightLayer;

class CanvasRootNode final : public QSGNode {
  public:
    int page_count = -1;
    PageBackgroundGroupNode* page_background_group = nullptr;
    GlyphGroupNode* glyph_group = nullptr;
    SelectionGroupNode* selection_group = nullptr;
    EnvironmentHighlightGroupNode* environment_highlight_group = nullptr;
    QSGSimpleRectNode* caret_node = nullptr;
    quint64 glyph_revision = std::numeric_limits<quint64>::max();
    quint64 glyph_geometry_revision = std::numeric_limits<quint64>::max();
    quint64 selection_revision = std::numeric_limits<quint64>::max();
    quint64 selection_geometry_revision = std::numeric_limits<quint64>::max();
    quint64 environment_highlight_revision = std::numeric_limits<quint64>::max();
    quint64 environment_highlight_geometry_revision = std::numeric_limits<quint64>::max();
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

bool build_glyph_frame(PageGlyphNode* group, QQuickWindow* window, const QVariantMap& frame,
                       const std::function<QRectF(int)>& page_rect,
                       const std::function<QPointF(int, const QPointF&)>& document_to_content,
                       qreal logical_scale, bool create_textures) {
    if (window == nullptr || !frame.value(QStringLiteral("valid")).toBool()) {
        return false;
    }
    const int page_index = frame.value(QStringLiteral("pageIndex")).toInt();
    const QList<QVariant> image_values = frame.value(QStringLiteral("images")).toList();
    const QList<QVariant> command_values = frame.value(QStringLiteral("commands")).toList();
    const qreal dpr = window->effectiveDevicePixelRatio();
    const qreal raster_pixels_per_point = frame.value(QStringLiteral("rasterPxPerPt")).toReal();
    const QRectF page_clip_rect = page_rect(page_index);
    if (page_clip_rect.isEmpty() || !is_finite_positive(dpr) ||
        !is_finite_positive(raster_pixels_per_point)) {
        return false;
    }
    const qreal image_to_logical_scale = logical_scale / raster_pixels_per_point;
    QVector<QSGTexture*> next_textures;
    QVector<bool> reused_previous(group->textures.size(), false);
    QVector<QSGTexture*> created_textures;
    QVariantList next_images;
    if (create_textures) {
        next_textures.reserve(image_values.size());
        next_images.reserve(image_values.size());
        for (qsizetype index = 0; index < image_values.size(); ++index) {
            const QVariant& value = image_values.at(index);
            if (index < group->images.size() && index < group->textures.size() &&
                group->images.at(index) == value &&
                group->textures[static_cast<int>(index)] != nullptr) {
                const int texture_index = static_cast<int>(index);
                next_textures.push_back(group->textures[texture_index]);
                reused_previous[texture_index] = true;
                next_images.push_back(value);
                continue;
            }
            const QVariantMap image = value.toMap();
            const int width = image.value(QStringLiteral("width")).toInt();
            const int height = image.value(QStringLiteral("height")).toInt();
            const QByteArray pixels = image.value(QStringLiteral("pixels")).toByteArray();
            if (width <= 0 || height <= 0) {
                next_textures.push_back(nullptr);
                next_images.push_back(value);
                continue;
            }
            const quint64 expected_size =
                static_cast<quint64>(width) * static_cast<quint64>(height) * 4U;
            if (width > std::numeric_limits<int>::max() / 4 ||
                expected_size != static_cast<quint64>(pixels.size())) {
                qDeleteAll(created_textures);
                return false;
            }
            const QImage image_data(reinterpret_cast<const uchar*>(pixels.constData()), width,
                                    height, static_cast<qsizetype>(width) * 4,
                                    QImage::Format_RGBA8888_Premultiplied);
            QSGTexture* texture =
                window->createTextureFromImage(image_data.copy(), QQuickWindow::TextureCanUseAtlas);
            if (texture == nullptr) {
                qDeleteAll(created_textures);
                return false;
            }
            created_textures.push_back(texture);
            next_textures.push_back(texture);
            next_images.push_back(value);
        }
    }
    const QVector<QSGTexture*>& textures = create_textures ? next_textures : group->textures;
    QSGNode next_draw_nodes;
    for (const QVariant& value : command_values) {
        const QVariantMap command = value.toMap();
        const int kind = command.value(QStringLiteral("kind")).toInt();
        const bool clip_enabled = command.value(QStringLiteral("clipEnabled")).toBool();
        QRectF clip_rect = page_clip_rect;
        if (clip_enabled) {
            const qreal clip_y0 = command.value(QStringLiteral("clipY0Pt")).toReal();
            const qreal clip_y1 = command.value(QStringLiteral("clipY1Pt")).toReal();
            const QPointF clip_top = document_to_content(page_index, QPointF(0.0, clip_y0));
            clip_rect = clip_rect.intersected(QRectF(page_clip_rect.left(), clip_top.y(),
                                                     page_clip_rect.width(),
                                                     (clip_y1 - clip_y0) * logical_scale));
        }
        QColor color(command.value(QStringLiteral("colorRed")).toInt(),
                     command.value(QStringLiteral("colorGreen")).toInt(),
                     command.value(QStringLiteral("colorBlue")).toInt(),
                     command.value(QStringLiteral("colorAlpha")).toInt());
        if (kind == 0) {
            const quint32 image_index = command.value(QStringLiteral("imageIndex")).toUInt();
            if (image_index >= static_cast<quint32>(textures.size())) {
                qDeleteAll(created_textures);
                return false;
            }
            if (textures[static_cast<int>(image_index)] == nullptr) {
                continue;
            }
            const QVariantMap image = image_values.at(static_cast<qsizetype>(image_index)).toMap();
            const qreal x_pt = command.value(QStringLiteral("xPt")).toReal();
            const qreal baseline_y_pt = command.value(QStringLiteral("yPt")).toReal();
            const qreal phase_x = command.value(QStringLiteral("pixelPhaseX")).toReal();
            const qreal phase_y = command.value(QStringLiteral("pixelPhaseY")).toReal();
            const QPointF baseline = document_to_content(page_index, QPointF(x_pt, baseline_y_pt));
            const qreal phase_x_adjustment =
                command.value(QStringLiteral("pixelPhaseApplied")).toBool() ? phase_x : 0.0;
            const qreal phase_y_adjustment =
                command.value(QStringLiteral("pixelPhaseApplied")).toBool() ? phase_y : 0.0;
            const qreal image_left = image.value(QStringLiteral("left")).toReal();
            const qreal image_top = image.value(QStringLiteral("top")).toReal();
            const qreal width_px = image.value(QStringLiteral("width")).toReal();
            const qreal height_px = image.value(QStringLiteral("height")).toReal();
            const QRectF glyph_rect(
                baseline.x() + (image_left - phase_x_adjustment) * image_to_logical_scale,
                baseline.y() + (-image_top - phase_y_adjustment) * image_to_logical_scale,
                width_px * image_to_logical_scale, height_px * image_to_logical_scale);
            const QRectF visible_rect = glyph_rect.intersected(clip_rect);
            if (visible_rect.isEmpty()) {
                continue;
            }
            const QRectF source_rect(
                (visible_rect.left() - glyph_rect.left()) / glyph_rect.width() * width_px,
                (visible_rect.top() - glyph_rect.top()) / glyph_rect.height() * height_px,
                visible_rect.width() / glyph_rect.width() * width_px,
                visible_rect.height() / glyph_rect.height() * height_px);
            QSGImageNode* node = make_glyph_node(window, textures[static_cast<int>(image_index)],
                                                 GlyphRectangles{visible_rect, source_rect});
            if (node == nullptr) {
                qDeleteAll(created_textures);
                return false;
            }
            next_draw_nodes.appendChildNode(node);
        } else if (kind == 1) {
            const QPointF top_left = document_to_content(
                page_index, QPointF(command.value(QStringLiteral("xPt")).toReal(),
                                    command.value(QStringLiteral("yPt")).toReal()));
            const QRectF rect(top_left.x(), top_left.y(),
                              command.value(QStringLiteral("widthPt")).toReal() * logical_scale,
                              command.value(QStringLiteral("heightPt")).toReal() * logical_scale);
            const QRectF visible = rect.intersected(clip_rect);
            if (!visible.isEmpty()) {
                next_draw_nodes.appendChildNode(new QSGSimpleRectNode(visible, color));
            }
        } else if (kind == 2) {
            const QPointF first = document_to_content(
                page_index, QPointF(command.value(QStringLiteral("xPt")).toReal(),
                                    command.value(QStringLiteral("yPt")).toReal()));
            const QPointF second = document_to_content(
                page_index, QPointF(command.value(QStringLiteral("x2Pt")).toReal(),
                                    command.value(QStringLiteral("y2Pt")).toReal()));
            const qreal thickness =
                command.value(QStringLiteral("thicknessPt")).toReal() * logical_scale;
            const QPointF delta = second - first;
            const qreal length = std::hypot(delta.x(), delta.y());
            if (length <= 0.0 || thickness <= 0.0) {
                continue;
            }
            const float first_x = static_cast<float>(first.x());
            const float first_y = static_cast<float>(first.y());
            const float angle =
                static_cast<float>(qRadiansToDegrees(std::atan2(delta.y(), delta.x())));
            if (!std::isfinite(first_x) || !std::isfinite(first_y) || !std::isfinite(angle)) {
                qDeleteAll(created_textures);
                return false;
            }
            QMatrix4x4 line_transform;
            line_transform.translate(first_x, first_y);
            line_transform.rotate(angle, 0.0F, 0.0F, 1.0F);
            auto* clip_node = new QSGClipNode();
            auto* clip_geometry = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(), 4);
            clip_geometry->setDrawingMode(QSGGeometry::DrawTriangleFan);
            const QPointF corners[] = {clip_rect.topLeft(), clip_rect.topRight(),
                                       clip_rect.bottomRight(), clip_rect.bottomLeft()};
            for (int index = 0; index < 4; ++index) {
                const float x = static_cast<float>(corners[index].x());
                const float y = static_cast<float>(corners[index].y());
                if (!std::isfinite(x) || !std::isfinite(y)) {
                    delete clip_geometry;
                    delete clip_node;
                    qDeleteAll(created_textures);
                    return false;
                }
                clip_geometry->vertexDataAsPoint2D()[index].set(x, y);
            }
            clip_node->setGeometry(clip_geometry);
            clip_node->setFlag(QSGNode::OwnsGeometry);
            clip_node->setIsRectangular(true);
            clip_node->setClipRect(clip_rect);
            auto* transform_node = new QSGTransformNode();
            transform_node->setMatrix(line_transform);
            transform_node->appendChildNode(
                new QSGSimpleRectNode(QRectF(0.0, -thickness / 2.0, length, thickness), color));
            clip_node->appendChildNode(transform_node);
            next_draw_nodes.appendChildNode(clip_node);
        }
    }
    group->clearGlyphNodes();
    if (create_textures) {
        for (qsizetype index = 0; index < group->textures.size(); ++index) {
            if (!reused_previous[index]) {
                delete group->textures[static_cast<int>(index)];
            }
        }
        group->textures = std::move(next_textures);
        group->images = std::move(next_images);
    }
    while (next_draw_nodes.firstChild() != nullptr) {
        QSGNode* child = next_draw_nodes.firstChild();
        next_draw_nodes.removeChildNode(child);
        group->draw_nodes->appendChildNode(child);
    }
    group->logical_scale = logical_scale;
    group->page_clip_rect = page_clip_rect;
    group->has_committed_frame = true;
    return true;
}

bool build_glyph_frames(GlyphGroupNode* group, QQuickItem* item, QQuickWindow* window,
                        const QVariantList& frames, const std::function<QRectF(int)>& page_rect,
                        const std::function<QPointF(int, const QPointF&)>& document_to_content,
                        const std::function<void(int, const QVariantMap&, bool)>& report_result,
                        qreal logical_scale, bool frames_changed, bool geometry_changed) {
    bool all_frames_built = true;
    const auto schedule_retry = [item, window](PageGlyphNode* page_node,
                                               const QVariantMap& frame_identity) {
        if (page_node->retry_frame != frame_identity) {
            page_node->retry_frame = frame_identity;
            page_node->texture_retry_count = 0;
        }
        if (window != nullptr && page_node->texture_retry_count == 0) {
            ++page_node->texture_retry_count;
            if (item != nullptr) {
                QMetaObject::invokeMethod(item, "update", Qt::QueuedConnection);
            }
            window->update();
        }
    };
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
            group->pages.insert(page_index, page_node);
            group->appendChildNode(page_node);
            const bool succeeded = build_glyph_frame(page_node, window, frame, page_rect,
                                                     document_to_content, logical_scale, true);
            report_result(page_index, frame_identity, succeeded);
            if (succeeded) {
                page_node->frame = frame_identity;
                page_node->retry_frame.clear();
                page_node->texture_retry_count = 0;
            } else {
                all_frames_built = false;
                schedule_retry(page_node, frame_identity);
            }
            continue;
        }

        const bool frame_changed = page_node->frame != frame_identity;
        const bool missing_texture = has_retryable_missing_texture(page_node);
        if (!frame_changed && frames_changed && !geometry_changed && !missing_texture) {
            continue;
        }
        const bool images_changed =
            page_node->images != frame.value(QStringLiteral("images")).toList();
        const bool succeeded =
            build_glyph_frame(page_node, window, frame, page_rect, document_to_content,
                              logical_scale, images_changed || missing_texture);
        report_result(page_index, frame_identity, succeeded);
        if (succeeded) {
            page_node->frame = frame_identity;
            page_node->retry_frame.clear();
            page_node->texture_retry_count = 0;
        } else {
            all_frames_built = false;
            const QRectF requested_clip = page_rect(page_index);
            if (!page_node->has_committed_frame || page_node->frame != frame_identity ||
                page_node->logical_scale != logical_scale ||
                page_node->page_clip_rect != requested_clip) {
                page_node->clearGlyphNodes();
                page_node->frame.clear();
                page_node->has_committed_frame = false;
            }
            schedule_retry(page_node, frame_identity);
        }
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
    return all_frames_built;
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
        EnvironmentHighlightGroupNode* environment_highlight_group =
            root != nullptr ? root->environment_highlight_group : nullptr;
        QSGSimpleRectNode* caret_node = root != nullptr ? root->caret_node : nullptr;
        if (root != nullptr) {
            root->removeChildNode(page_background_group);
            root->removeChildNode(glyph_group);
            root->removeChildNode(selection_group);
            root->removeChildNode(environment_highlight_group);
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
        root->environment_highlight_group = environment_highlight_group != nullptr
                                                ? environment_highlight_group
                                                : new EnvironmentHighlightGroupNode();
        root->caret_node = caret_node != nullptr ? caret_node : new QSGSimpleRectNode();
        root->appendChildNode(root->page_background_group);
        root->appendChildNode(root->environment_highlight_group);
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
    const auto to_content = [this](int page_index, const QPointF& point) {
        return documentToContent(page_index, point);
    };
    const auto page_rect = [this](int page_index) { return pageRect(page_index); };
    if (root->selection_revision != m_selection_revision ||
        root->selection_geometry_revision != m_glyph_geometry_revision) {
        root->selection_group->clearRectangles();
        QVariantList selection_rectangles = m_selection_rects;
        selection_rectangles.append(m_selection_preview_rects);
        for (const QVariant& value : selection_rectangles) {
            const QVariantMap selection = value.toMap();
            const int page_index = selection.value(QStringLiteral("pageIndex")).toInt();
            const QRectF visible_rect =
                canvas_detail::visible_canvas_rect(page_index, selection, to_content, page_rect);
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
    if (root->environment_highlight_revision != m_environment_highlight_revision ||
        root->environment_highlight_geometry_revision != m_glyph_geometry_revision) {
        root->environment_highlight_group->update(
            m_environment_highlight_rects, m_first_visible_page,
            m_page_end_visible < 0 ? page_count : m_page_end_visible,
            {m_environment_highlight_color, m_environment_highlight_radius,
             m_environment_highlight_padding_px},
            to_content, page_rect);
        root->environment_highlight_revision = m_environment_highlight_revision;
        root->environment_highlight_geometry_revision = m_glyph_geometry_revision;
    } else {
        root->environment_highlight_group->setColor(m_environment_highlight_color);
    }
    auto* glyph_group = root->glyph_group;
    const bool missing_glyph_texture = has_retryable_missing_texture(glyph_group);
    if (root->glyph_revision != m_glyph_frame_revision ||
        root->glyph_geometry_revision != m_glyph_geometry_revision || missing_glyph_texture) {
        const bool glyph_frames_changed = root->glyph_revision != m_glyph_frame_revision;
        const quint64 geometry_revision = m_glyph_geometry_revision;
        const quint64 scene_graph_generation = m_scene_graph_generation;
        const quintptr window_identity = reinterpret_cast<quintptr>(window());
        const auto report_result =
            [this, geometry_revision, scene_graph_generation,
             window_identity](int page_index, const QVariantMap& frame_identity, bool succeeded) {
                QMetaObject::invokeMethod(
                    this,
                    [this, page_index, frame_identity, succeeded, geometry_revision,
                     scene_graph_generation, window_identity] {
                        reportGlyphFrameBuildResult(page_index, frame_identity, succeeded,
                                                    geometry_revision, scene_graph_generation,
                                                    window_identity);
                    },
                    Qt::QueuedConnection);
            };
        QVariantList render_frames = m_glyph_frames;
        for (QVariant& value : render_frames) {
            QVariantMap frame = value.toMap();
            const int page_index = frame.value(QStringLiteral("pageIndex")).toInt();
            frame.insert(QStringLiteral("pageSubmissionGeneration"),
                         QString::number(m_glyph_page_submission_generations.value(page_index)));
            value = frame;
        }
        const bool frames_built = build_glyph_frames(
            glyph_group, this, window(), render_frames,
            [this](int page_index) { return pageRect(page_index); },
            [this](int page_index, const QPointF& point) {
                return documentToContent(page_index, point);
            },
            report_result, scale(), glyph_frames_changed,
            root->glyph_geometry_revision != m_glyph_geometry_revision);
        if (frames_built) {
            root->glyph_revision = m_glyph_frame_revision;
            root->glyph_geometry_revision = m_glyph_geometry_revision;
        }
    }
    auto* node = root->caret_node;
    const bool preview_caret_visible = hasActiveFocus() && !m_caret_preview_geometry.isEmpty();
    if (!m_command_mode && (preview_caret_visible ||
                            (m_caret_visible && hasActiveFocus() && !m_caret_geometry.isEmpty()))) {
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
