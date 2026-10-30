// 绘制行内与块环境共用的圆角跨页高亮。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "environment_highlight_scene.hpp"

#include "scene_graph_geometry.hpp"
#include <QColor>
#include <QList>
#include <QMap>
#include <QSGNode>
#include <QSGSimpleRectNode>
#include <QString>
#include <QVariant>
#include <algorithm>
#include <cmath>

namespace caret {
namespace {

class RoundedRectSlices final : public QSGNode {
  public:
    void setRect(const QRectF& rect, qreal radius, const QColor& color) {
        const qreal safe_radius =
            std::clamp(radius, 0.0, std::min(rect.width(), rect.height()) / 2.0);
        QList<QRectF> slices;
        if (safe_radius <= 0.0) {
            slices.push_back(rect);
        } else {
            const int segment_count =
                static_cast<int>(std::clamp(std::ceil(safe_radius * 2.0), 1.0, 32.0));
            const qreal segment_height = safe_radius / static_cast<qreal>(segment_count);
            for (int segment = 0; segment < segment_count; ++segment) {
                const qreal top = static_cast<qreal>(segment) * segment_height;
                const qreal bottom = static_cast<qreal>(segment + 1) * segment_height;
                const qreal inset = rounded_corner_inset(safe_radius, top);
                slices.push_back(QRectF(rect.left() + inset, rect.top() + top,
                                        rect.width() - inset * 2.0, bottom - top));
            }
            const qreal middle_top = rect.top() + safe_radius;
            const qreal middle_bottom = rect.bottom() - safe_radius;
            if (middle_bottom > middle_top) {
                slices.push_back(
                    QRectF(rect.left(), middle_top, rect.width(), middle_bottom - middle_top));
            }
            for (int segment = segment_count - 1; segment >= 0; --segment) {
                const qreal top = static_cast<qreal>(segment) * segment_height;
                const qreal bottom = static_cast<qreal>(segment + 1) * segment_height;
                const qreal inset = rounded_corner_inset(safe_radius, top);
                slices.push_back(QRectF(rect.left() + inset, rect.bottom() - bottom,
                                        rect.width() - inset * 2.0, bottom - top));
            }
        }

        while (m_rectangles.size() < slices.size()) {
            auto* rectangle = new QSGSimpleRectNode();
            m_rectangles.push_back(rectangle);
            appendChildNode(rectangle);
        }
        while (m_rectangles.size() > slices.size()) {
            QSGSimpleRectNode* rectangle = m_rectangles.takeLast();
            removeChildNode(rectangle);
            delete rectangle;
        }
        for (qsizetype index = 0; index < slices.size(); ++index) {
            m_rectangles[index]->setRect(slices[index]);
            m_rectangles[index]->setColor(color);
        }
    }

    void setColor(const QColor& color) {
        for (QSGSimpleRectNode* rectangle : m_rectangles) {
            rectangle->setColor(color);
        }
    }

  private:
    static qreal rounded_corner_inset(qreal radius, qreal distance_from_edge) {
        const qreal center_distance = radius - distance_from_edge;
        const qreal radius_squared = radius * radius;
        return radius -
               std::sqrt(std::max(0.0, radius_squared - center_distance * center_distance));
    }

    QVector<QSGSimpleRectNode*> m_rectangles;
};

} // namespace

void EnvironmentHighlightLayer::update(const QList<QVariant>& rectangles, int first_page,
                                       int page_end, const EnvironmentHighlightStyle& style,
                                       const DocumentToContent& document_to_content,
                                       const PageRect& page_rect) {
    qsizetype next_rectangle = 0;
    if (style.color.isValid()) {
        for (const QVariant& value : rectangles) {
            const QMap<QString, QVariant> rectangle = value.toMap();
            const int page_index = rectangle.value(QStringLiteral("pageIndex")).toInt();
            if (page_index < first_page || (page_end >= 0 && page_index >= page_end)) {
                continue;
            }
            const QRectF visible_rect = canvas_detail::visible_canvas_rect(
                page_index, rectangle, document_to_content, page_rect);
            if (visible_rect.isEmpty()) {
                continue;
            }
            const QRectF padded_rect = visible_rect
                                           .adjusted(-style.padding_px, -style.padding_px,
                                                     style.padding_px, style.padding_px)
                                           .intersected(page_rect(page_index));
            if (padded_rect.isEmpty()) {
                continue;
            }
            auto* node = next_rectangle < m_rectangles.size()
                             ? static_cast<RoundedRectSlices*>(m_rectangles[next_rectangle])
                             : new RoundedRectSlices();
            if (next_rectangle == m_rectangles.size()) {
                m_rectangles.push_back(node);
                appendChildNode(node);
            }
            node->setRect(padded_rect, style.radius, style.color);
            ++next_rectangle;
        }
    }
    while (m_rectangles.size() > next_rectangle) {
        QSGNode* node = m_rectangles.takeLast();
        removeChildNode(node);
        delete node;
    }
}

void EnvironmentHighlightLayer::setColor(const QColor& color) {
    for (QSGNode* node : m_rectangles) {
        static_cast<RoundedRectSlices*>(node)->setColor(color);
    }
}

} // namespace caret
