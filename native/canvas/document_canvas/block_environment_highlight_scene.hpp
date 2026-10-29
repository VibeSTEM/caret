// 管理当前块环境高亮的跨页场景图节点。
#pragma once

#include <QColor>
#include <QList>
#include <QPointF>
#include <QRectF>
#include <QSGNode>
#include <QVariant>
#include <functional>

namespace caret {

struct BlockEnvironmentHighlightStyle {
    QColor color;
    qreal radius = 0.0;
    qreal padding_px = 0.0;
};

class BlockEnvironmentHighlightLayer final : public QSGNode {
  public:
    using DocumentToContent = std::function<QPointF(int, const QPointF&)>;
    using PageRect = std::function<QRectF(int)>;

    void update(const QList<QVariant>& rectangles, int first_page, int page_end,
                const BlockEnvironmentHighlightStyle& style,
                const DocumentToContent& document_to_content, const PageRect& page_rect);
    void setColor(const QColor& color);

  private:
    QList<QSGNode*> m_rectangles;
};

} // namespace caret
