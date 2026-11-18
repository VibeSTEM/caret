// 在场景图线程维护纸张、字形纹理和插入点节点，并复用未变化页面。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "document_canvas.hpp"
#include "environment_highlight_scene.hpp"
#include "glyph_frame_wire.hpp"
#include "scene_graph_geometry.hpp"
#include "value_validation.hpp"
#include <QByteArray>
#include <QColor>
#include <QHash>
#include <QImage>
#include <QList>
#include <QMap>
#include <QMatrix4x4>
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
#include <cstdint>
#include <functional>
#include <limits>
#include <optional>
#include <qcontainerfwd.h>
#include <qhashfunctions.h>
#include <qnamespace.h>
#include <qpoint.h>
#include <utility>

namespace caret {
namespace {
using canvas_detail::is_finite_positive;

enum class PaintItemKind : std::uint8_t { Glyph, Decoration, Shape };

struct PaintItem {
    quint64 order = 0;
    QSGNode* node = nullptr;
    PaintItemKind kind = PaintItemKind::Shape;
};

class PageGlyphNode final : public QSGNode {
  public:
    PageGlyphNode() {
        glyph_nodes = new QSGNode();
        decoration_nodes = new QSGNode();
        shape_nodes = new QSGNode();
        paint_nodes = new QSGNode();
        appendChildNode(glyph_nodes);
        appendChildNode(decoration_nodes);
        appendChildNode(shape_nodes);
        appendChildNode(paint_nodes);
    }

    ~PageGlyphNode() override { clearGlyphResources(); }

    void clearGlyphNodes() {
        clearNodeChildren(glyph_nodes);
        clearNodeChildren(decoration_nodes);
        clearNodeChildren(shape_nodes);
        clearNodeChildren(paint_nodes);
        paint_items.clear();
    }

    void clearGlyphResources() {
        clearGlyphNodes();
        qDeleteAll(textures);
        textures.clear();
        images.clear();
    }

    QMap<QString, QVariant> frame;
    std::optional<glyph_frame_wire::GlyphFrame> parsed_frame;
    QVariantList images;
    QVector<QSGTexture*> textures;
    QSGNode* glyph_nodes = nullptr;
    QSGNode* decoration_nodes = nullptr;
    QSGNode* shape_nodes = nullptr;
    QSGNode* paint_nodes = nullptr;
    QVector<PaintItem> paint_items;

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
    if (!page_node->parsed_frame) {
        return false;
    }
    const auto& images = page_node->parsed_frame->images;
    for (qsizetype index = 0; index < images.size(); ++index) {
        if (index < static_cast<qsizetype>(page_node->textures.size()) &&
            page_node->textures.at(static_cast<int>(index)) != nullptr) {
            continue;
        }
        const glyph_frame_wire::GlyphImage& image = images.at(index);
        if (image.width > 0 && image.height > 0) {
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
    EnvironmentHighlightGroupNode* block_highlight_group = nullptr;
    EnvironmentHighlightGroupNode* inline_highlight_group = nullptr;
    QSGSimpleRectNode* caret_node = nullptr;
    quint64 glyph_revision = std::numeric_limits<quint64>::max();
    quint64 glyph_geometry_revision = std::numeric_limits<quint64>::max();
    quint64 selection_revision = std::numeric_limits<quint64>::max();
    quint64 selection_geometry_revision = std::numeric_limits<quint64>::max();
    quint64 highlight_revision = std::numeric_limits<quint64>::max();
    quint64 highlight_geometry_revision = std::numeric_limits<quint64>::max();
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

void append_shape_lines(QSGNode* parent, const QVector<QPointF>& points, QColor color,
                        qreal width) {
    if (points.isEmpty() || width <= 0.0) {
        return;
    }
    const qreal half_width = width / 2.0;
    for (qsizetype index = 0; index + 1 < points.size(); index += 2) {
        const QPointF first = points.at(index);
        const QPointF second = points.at(index + 1);
        const QPointF delta = second - first;
        const qreal length = std::hypot(delta.x(), delta.y());
        if (!std::isfinite(length) || length <= std::numeric_limits<qreal>::epsilon()) {
            continue;
        }
        auto* transform = new QSGTransformNode();
        QMatrix4x4 matrix;
        matrix.translate(static_cast<float>(first.x()), static_cast<float>(first.y()));
        matrix.rotate(static_cast<float>(qRadiansToDegrees(std::atan2(delta.y(), delta.x()))), 0.0F,
                      0.0F, 1.0F);
        transform->setMatrix(matrix);
        transform->appendChildNode(
            new QSGSimpleRectNode(QRectF(0.0, -half_width, length, width), color));
        parent->appendChildNode(transform);
    }
}

QSGClipNode* make_rect_clip_node(const QRectF& rect) {
    auto* geometry = new QSGGeometry(QSGGeometry::defaultAttributes_Point2D(), 4);
    geometry->setDrawingMode(QSGGeometry::DrawTriangleStrip);
    auto* vertices = geometry->vertexDataAsPoint2D();
    vertices[0].set(static_cast<float>(rect.left()), static_cast<float>(rect.top()));
    vertices[1].set(static_cast<float>(rect.right()), static_cast<float>(rect.top()));
    vertices[2].set(static_cast<float>(rect.left()), static_cast<float>(rect.bottom()));
    vertices[3].set(static_cast<float>(rect.right()), static_cast<float>(rect.bottom()));
    auto* clip = new QSGClipNode();
    clip->setGeometry(geometry);
    clip->setFlag(QSGNode::OwnsGeometry);
    clip->setClipRect(rect);
    clip->setIsRectangular(true);
    return clip;
}

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

/// 把行级裁剪范围（clipY0Pt..clipY1Pt）换算成画布裁剪区；未启用时返回页裁剪区。
/// 字段取值由 glyph_frame_wire 校验，这里只做几何换算。
QRectF glyph_frame_clip_rect(const QRectF& page_clip_rect, int page_index, bool clip_enabled,
                             qreal clip_y0_pt, qreal clip_y1_pt,
                             const std::function<QPointF(int, const QPointF&)>& document_to_content,
                             qreal logical_scale) {
    if (!clip_enabled) {
        return page_clip_rect;
    }
    const QPointF clip_top = document_to_content(page_index, QPointF(0.0, clip_y0_pt));
    return page_clip_rect.intersected(QRectF(page_clip_rect.left(), clip_top.y(),
                                             page_clip_rect.width(),
                                             (clip_y1_pt - clip_y0_pt) * logical_scale));
}

void build_glyph_frame(PageGlyphNode* group, QQuickWindow* window, const QVariantMap& frame,
                       const std::function<QRectF(int)>& page_rect,
                       const std::function<QPointF(int, const QPointF&)>& document_to_content,
                       qreal logical_scale, bool create_textures) {
    if (window == nullptr) {
        group->clearGlyphNodes();
        return;
    }
    // 线格式只解析一次；缓存随帧身份变化更新。
    if (!group->parsed_frame) {
        group->clearGlyphNodes();
        return;
    }
    const glyph_frame_wire::GlyphFrame& glyph_frame = *group->parsed_frame;
    const int page_index = glyph_frame.page_index;
    const QVariantList image_values = frame.value(QStringLiteral("images")).toList();
    const qreal dpr = window->effectiveDevicePixelRatio();
    const qreal raster_pixels_per_point = glyph_frame.raster_px_per_pt;
    const QRectF page_clip_rect = page_rect(page_index);
    if (page_clip_rect.isEmpty() || !is_finite_positive(dpr) ||
        !is_finite_positive(raster_pixels_per_point)) {
        group->clearGlyphNodes();
        return;
    }
    const qreal image_to_logical_scale = logical_scale / raster_pixels_per_point;
    for (const PaintItem& item : group->paint_items) {
        if (item.node == nullptr || item.node->parent() != group->paint_nodes) {
            continue;
        }
        group->paint_nodes->removeChildNode(item.node);
        switch (item.kind) {
        case PaintItemKind::Glyph:
            group->glyph_nodes->appendChildNode(item.node);
            break;
        case PaintItemKind::Decoration:
            group->decoration_nodes->appendChildNode(item.node);
            break;
        case PaintItemKind::Shape:
            group->shape_nodes->appendChildNode(item.node);
            break;
        }
    }
    group->paint_items.clear();
    QVector<PaintItem> ordered_paint_nodes;

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
            const glyph_frame_wire::GlyphImage& image = glyph_frame.images.at(index);
            if (image.width <= 0 || image.height <= 0) {
                group->textures.push_back(nullptr);
                group->images.push_back(value);
                continue;
            }
            // parse 已保证 width*height*4 等于 pixels 字节数，QImage 可安全引用像素缓冲。
            const QImage image_data(
                reinterpret_cast<const uchar*>(image.pixels.constData()), image.width, image.height,
                static_cast<qsizetype>(image.width) * 4, QImage::Format_RGBA8888_Premultiplied);
            QSGTexture* texture =
                window->createTextureFromImage(image_data.copy(), QQuickWindow::TextureCanUseAtlas);
            group->textures.push_back(texture);
            group->images.push_back(value);
        }
        qDeleteAll(previous_textures);
    }

    QSGNode* reusable_node = group->glyph_nodes->firstChild();
    for (const glyph_frame_wire::GlyphPlacement& placement : glyph_frame.placements) {
        const qsizetype image_index = placement.image_index;
        if (image_index >= static_cast<qsizetype>(group->textures.size()) ||
            group->textures[static_cast<int>(image_index)] == nullptr) {
            continue;
        }
        const glyph_frame_wire::GlyphImage& image = glyph_frame.images.at(image_index);
        const QPointF baseline =
            document_to_content(page_index, QPointF(placement.x_pt, placement.baseline_y_pt));
        const QRectF glyph_rect = canvas_detail::glyph_image_rect(
            baseline, QPointF(image.left, image.top), QSizeF(image.width, image.height),
            image_to_logical_scale, QPointF(placement.pixel_phase_x, placement.pixel_phase_y),
            placement.pixel_phase_applied);
        const QRectF clip_rect = glyph_frame_clip_rect(
            page_clip_rect, page_index, placement.clip_enabled, placement.clip_y0_pt,
            placement.clip_y1_pt, document_to_content, logical_scale);
        const QRectF visible_rect = glyph_rect.intersected(clip_rect);
        if (visible_rect.isEmpty() || glyph_rect.width() <= 0.0 || glyph_rect.height() <= 0.0) {
            continue;
        }
        const qreal u0 = (visible_rect.left() - glyph_rect.left()) / glyph_rect.width();
        const qreal v0 = (visible_rect.top() - glyph_rect.top()) / glyph_rect.height();
        const qreal u1 = (visible_rect.right() - glyph_rect.left()) / glyph_rect.width();
        const qreal v1 = (visible_rect.bottom() - glyph_rect.top()) / glyph_rect.height();
        const QRectF source_rect(u0 * image.width, v0 * image.height, (u1 - u0) * image.width,
                                 (v1 - v0) * image.height);
        const GlyphRectangles rectangles{visible_rect, source_rect};
        auto* glyph_node = static_cast<QSGImageNode*>(reusable_node);
        if (reusable_node != nullptr) {
            reusable_node = reusable_node->nextSibling();
        }
        if (glyph_node == nullptr) {
            glyph_node =
                make_glyph_node(window, group->textures[static_cast<int>(image_index)], rectangles);
            if (glyph_node != nullptr) {
                group->glyph_nodes->appendChildNode(glyph_node);
            }
        } else {
            update_glyph_node(glyph_node, group->textures[static_cast<int>(image_index)],
                              rectangles);
        }
        if (glyph_node != nullptr) {
            ordered_paint_nodes.push_back(
                {placement.paint_order, glyph_node, PaintItemKind::Glyph});
        }
    }
    while (reusable_node != nullptr) {
        QSGNode* next_node = reusable_node->nextSibling();
        group->glyph_nodes->removeChildNode(reusable_node);
        delete reusable_node;
        reusable_node = next_node;
    }

    reusable_node = group->decoration_nodes->firstChild();
    for (const glyph_frame_wire::GlyphDecoration& decoration : glyph_frame.decorations) {
        const QPointF top_left =
            document_to_content(page_index, QPointF(decoration.x_pt, decoration.y_pt));
        const QRectF decoration_rect(top_left.x(), top_left.y(),
                                     decoration.width_pt * logical_scale,
                                     decoration.height_pt * logical_scale);
        const QRectF clip_rect = glyph_frame_clip_rect(
            page_clip_rect, page_index, decoration.clip_enabled, decoration.clip_y0_pt,
            decoration.clip_y1_pt, document_to_content, logical_scale);
        const QRectF visible_rect = decoration_rect.intersected(clip_rect);
        if (visible_rect.isEmpty()) {
            continue;
        }
        QColor color(decoration.color_red, decoration.color_green, decoration.color_blue,
                     decoration.color_alpha);
        auto* rect_node = static_cast<QSGSimpleRectNode*>(reusable_node);
        if (reusable_node != nullptr) {
            reusable_node = reusable_node->nextSibling();
        }
        if (rect_node == nullptr) {
            rect_node = new QSGSimpleRectNode(visible_rect, color);
            group->decoration_nodes->appendChildNode(rect_node);
        } else {
            rect_node->setRect(visible_rect);
            rect_node->setColor(color);
        }
        if (rect_node != nullptr) {
            ordered_paint_nodes.push_back(
                {decoration.paint_order, rect_node, PaintItemKind::Decoration});
        }
    }
    while (reusable_node != nullptr) {
        QSGNode* next_node = reusable_node->nextSibling();
        group->decoration_nodes->removeChildNode(reusable_node);
        delete reusable_node;
        reusable_node = next_node;
    }

    while (group->shape_nodes->firstChild() != nullptr) {
        QSGNode* child = group->shape_nodes->firstChild();
        group->shape_nodes->removeChildNode(child);
        delete child;
    }
    for (const glyph_frame_wire::GlyphShape& shape : glyph_frame.shapes) {
        const QRectF clip_rect =
            glyph_frame_clip_rect(page_clip_rect, page_index, shape.clip_enabled, shape.clip_y0_pt,
                                  shape.clip_y1_pt, document_to_content, logical_scale);
        const QPointF origin = document_to_content(page_index, QPointF(shape.x_pt, shape.y_pt));
        auto* shape_item = new QSGNode();
        group->shape_nodes->appendChildNode(shape_item);
        if (shape.kind == 0) {
            const QPointF first = origin;
            const QPointF second = document_to_content(
                page_index, QPointF(shape.x_pt + shape.dx_pt, shape.y_pt + shape.dy_pt));
            const qreal line_width = shape.stroke_width_pt * logical_scale;
            const QColor color(shape.stroke_red, shape.stroke_green, shape.stroke_blue,
                               shape.stroke_alpha);
            const qreal half_width = line_width / 2.0;
            const QRectF stroke_bounds =
                QRectF(first, second)
                    .normalized()
                    .adjusted(-half_width, -half_width, half_width, half_width);
            const bool needs_clip = shape.clip_enabled || !clip_rect.contains(stroke_bounds);
            QSGNode* line_parent = shape_item;
            if (needs_clip) {
                auto* clipped = make_rect_clip_node(clip_rect);
                shape_item->appendChildNode(clipped);
                line_parent = clipped;
            }
            append_shape_lines(line_parent, {first, second}, color, line_width);
            ordered_paint_nodes.push_back({shape.paint_order, shape_item, PaintItemKind::Shape});
            continue;
        }

        const QRectF rect(origin.x(), origin.y(), shape.width_pt * logical_scale,
                          shape.height_pt * logical_scale);
        if (shape.fill_enabled) {
            const QRectF visible = rect.intersected(clip_rect);
            if (!visible.isEmpty()) {
                const QColor fill(shape.fill_red, shape.fill_green, shape.fill_blue,
                                  shape.fill_alpha);
                shape_item->appendChildNode(new QSGSimpleRectNode(visible, fill));
            }
        }
        if (shape.stroke_enabled && shape.stroke_width_pt > 0.0) {
            const qreal line_width = shape.stroke_width_pt * logical_scale;
            const QColor stroke(shape.stroke_red, shape.stroke_green, shape.stroke_blue,
                                shape.stroke_alpha);
            QSGNode* stroke_parent = shape_item;
            if (!clip_rect.contains(rect.adjusted(-line_width / 2.0, -line_width / 2.0,
                                                  line_width / 2.0, line_width / 2.0))) {
                auto* clipped = make_rect_clip_node(clip_rect);
                shape_item->appendChildNode(clipped);
                stroke_parent = clipped;
            }
            const qreal half_width = line_width / 2.0;
            if (line_width >= std::min(rect.width(), rect.height())) {
                stroke_parent->appendChildNode(new QSGSimpleRectNode(
                    rect.adjusted(-half_width, -half_width, half_width, half_width), stroke));
            } else {
                stroke_parent->appendChildNode(
                    new QSGSimpleRectNode(QRectF(rect.left() - half_width, rect.top() - half_width,
                                                 rect.width() + line_width, line_width),
                                          stroke));
                stroke_parent->appendChildNode(new QSGSimpleRectNode(
                    QRectF(rect.left() - half_width, rect.bottom() - half_width,
                           rect.width() + line_width, line_width),
                    stroke));
                const qreal side_height = rect.height() - line_width;
                if (side_height > 0.0) {
                    stroke_parent->appendChildNode(new QSGSimpleRectNode(
                        QRectF(rect.left() - half_width, rect.top() + half_width, line_width,
                               side_height),
                        stroke));
                    stroke_parent->appendChildNode(new QSGSimpleRectNode(
                        QRectF(rect.right() - half_width, rect.top() + half_width, line_width,
                               side_height),
                        stroke));
                }
            }
        }
        ordered_paint_nodes.push_back({shape.paint_order, shape_item, PaintItemKind::Shape});
    }
    std::stable_sort(
        ordered_paint_nodes.begin(), ordered_paint_nodes.end(),
        [](const auto& first, const auto& second) { return first.order < second.order; });
    group->paint_items = ordered_paint_nodes;
    for (const PaintItem& paint_node : ordered_paint_nodes) {
        QSGNode* node = paint_node.node;
        if (node->parent() != nullptr) {
            node->parent()->removeChildNode(node);
        }
        group->paint_nodes->appendChildNode(node);
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
            page_node->parsed_frame = glyph_frame_wire::parse(frame);
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
            page_node->parsed_frame = glyph_frame_wire::parse(frame);
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
        EnvironmentHighlightGroupNode* block_highlight_group =
            root != nullptr ? root->block_highlight_group : nullptr;
        EnvironmentHighlightGroupNode* inline_highlight_group =
            root != nullptr ? root->inline_highlight_group : nullptr;
        QSGSimpleRectNode* caret_node = root != nullptr ? root->caret_node : nullptr;
        if (root != nullptr) {
            root->removeChildNode(page_background_group);
            root->removeChildNode(glyph_group);
            root->removeChildNode(selection_group);
            root->removeChildNode(block_highlight_group);
            root->removeChildNode(inline_highlight_group);
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
        root->block_highlight_group = block_highlight_group != nullptr
                                          ? block_highlight_group
                                          : new EnvironmentHighlightGroupNode();
        root->inline_highlight_group = inline_highlight_group != nullptr
                                           ? inline_highlight_group
                                           : new EnvironmentHighlightGroupNode();
        root->caret_node = caret_node != nullptr ? caret_node : new QSGSimpleRectNode();
        root->appendChildNode(root->page_background_group);
        root->appendChildNode(root->block_highlight_group);
        root->appendChildNode(root->inline_highlight_group);
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
    if (root->highlight_revision != m_highlight_revision ||
        root->highlight_geometry_revision != m_glyph_geometry_revision) {
        root->block_highlight_group->update(
            m_block_highlight_rects, m_first_visible_page,
            m_page_end_visible < 0 ? page_count : m_page_end_visible,
            {m_block_highlight_color, m_environment_highlight_radius,
             m_environment_highlight_padding_px},
            to_content, page_rect);
        root->inline_highlight_group->update(
            m_inline_highlight_rects, m_first_visible_page,
            m_page_end_visible < 0 ? page_count : m_page_end_visible,
            {m_inline_highlight_color, m_environment_highlight_radius,
             m_environment_highlight_padding_px},
            to_content, page_rect);
        root->highlight_revision = m_highlight_revision;
        root->highlight_geometry_revision = m_glyph_geometry_revision;
    } else {
        root->block_highlight_group->setColor(m_block_highlight_color);
        root->inline_highlight_group->setColor(m_inline_highlight_color);
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
