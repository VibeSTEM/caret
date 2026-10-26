// 多页文档画布及页面坐标映射。
#pragma once

#include <QColor>
#include <QPointF>
#include <QQuickItem>
#include <QRectF>
#include <QSizeF>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>
#include <QVector>
#include <QtQml/qqmlregistration.h>

class QSGNode;
class QKeyEvent;

namespace caret {

/// 将页面物理点坐标映射到画布逻辑像素；DPR 只由 Qt 绘制后端处理。
class DocumentCanvas : public QQuickItem {
    Q_OBJECT
    QML_ELEMENT
    Q_PROPERTY(
        QVariantList pageSizesPt READ pageSizesPt WRITE setPageSizesPt NOTIFY pageSizesPtChanged)
    Q_PROPERTY(QVariantMap pageMarginsPt READ pageMarginsPt WRITE setPageMarginsPt NOTIFY
                   pageMarginsPtChanged)
    Q_PROPERTY(qreal zoom READ zoom WRITE setZoom NOTIFY zoomChanged)
    Q_PROPERTY(QColor pageColor READ pageColor WRITE setPageColor NOTIFY appearanceChanged)
    Q_PROPERTY(QColor pageBorderColor READ pageBorderColor WRITE setPageBorderColor NOTIFY
                   appearanceChanged)
    Q_PROPERTY(QVariantMap caretGeometry READ caretGeometry WRITE setCaretGeometry NOTIFY
                   caretGeometryChanged)
    Q_PROPERTY(bool caretBlinkEnabled READ caretBlinkEnabled WRITE setCaretBlinkEnabled NOTIFY
                   caretBlinkEnabledChanged)
    Q_PROPERTY(
        QVariantList glyphFrames READ glyphFrames WRITE setGlyphFrames NOTIFY glyphFramesChanged)
    Q_PROPERTY(QColor caretColor READ caretColor WRITE setCaretColor NOTIFY appearanceChanged)
    Q_PROPERTY(int caretWidthCentipixels READ caretWidthCentipixels WRITE setCaretWidthCentipixels
                   NOTIFY caretWidthCentipixelsChanged)
    Q_PROPERTY(qreal pageGapPt READ pageGapPt WRITE setPageGapPt NOTIFY pageGapPtChanged)
    Q_PROPERTY(
        qreal pageMarginPt READ pageMarginPt WRITE setPageMarginPt NOTIFY pageMarginPtChanged)
    Q_PROPERTY(QSizeF naturalContentSize READ naturalContentSize NOTIFY geometryChanged)

  public:
    explicit DocumentCanvas(QQuickItem* parent = nullptr);

    [[nodiscard]] QVariantList pageSizesPt() const;
    void setPageSizesPt(const QVariantList& page_sizes);
    [[nodiscard]] QVariantMap pageMarginsPt() const;
    void setPageMarginsPt(const QVariantMap& margins);
    [[nodiscard]] qreal zoom() const;
    void setZoom(qreal zoom);
    [[nodiscard]] QColor pageColor() const;
    void setPageColor(const QColor& color);
    [[nodiscard]] QColor pageBorderColor() const;
    void setPageBorderColor(const QColor& color);
    [[nodiscard]] QVariantMap caretGeometry() const;
    void setCaretGeometry(const QVariantMap& geometry);
    [[nodiscard]] bool caretBlinkEnabled() const;
    void setCaretBlinkEnabled(bool enabled);
    [[nodiscard]] QVariantList glyphFrames() const;
    void setGlyphFrames(const QVariantList& frames);
    [[nodiscard]] QColor caretColor() const;
    void setCaretColor(const QColor& color);
    [[nodiscard]] int caretWidthCentipixels() const;
    void setCaretWidthCentipixels(int width_centipixels);
    [[nodiscard]] qreal pageGapPt() const;
    void setPageGapPt(qreal gap_pt);
    [[nodiscard]] qreal pageMarginPt() const;
    void setPageMarginPt(qreal margin_pt);
    [[nodiscard]] QSizeF naturalContentSize() const;

    /// 返回页面在画布内容坐标中的原点；无效页索引返回空点。
    Q_INVOKABLE QPointF pageOrigin(int page_index) const;
    /// 将页面 pt 坐标映射到画布内容逻辑像素。
    Q_INVOKABLE QPointF documentToContent(int page_index, const QPointF& point_pt) const;
    /// 将画布内容逻辑像素映射到指定页面的 pt 坐标。
    Q_INVOKABLE QPointF contentToDocument(int page_index, const QPointF& point_px) const;
    /// 页面矩形内返回页索引及 pt 坐标；页面外返回 pageIndex = -1。
    Q_INVOKABLE QVariantMap hitTestPage(const QPointF& point_px) const;
    /// 返回视图变换和 DPR 后的页面 raster 请求参数。
    Q_INVOKABLE QVariantMap glyphRasterRequest(int page_index) const;

  signals:
    void textInputRequested(const QString& text);
    void editingCommandRequested(const QString& command_id);
    void pageSizesPtChanged();
    void pageMarginsPtChanged();
    void zoomChanged();
    void pageGapPtChanged();
    void pageMarginPtChanged();
    void caretGeometryChanged();
    void caretBlinkEnabledChanged();
    void glyphFramesChanged();
    void glyphRasterMetricsChanged();
    void caretWidthCentipixelsChanged();
    void appearanceChanged();
    void geometryChanged();

  private:
    [[nodiscard]] qreal scale() const;
    [[nodiscard]] QSizeF pageSizePt(int page_index) const;
    [[nodiscard]] QRectF pageRect(int page_index) const;
    void updateGeometry();
    void initializeGeometryNotifications();
    void initializeCaretBlink();
    [[nodiscard]] QRectF caretRect() const;
    void refreshCaretBlink();
    void toggleCaretVisibility();
    void keyPressEvent(QKeyEvent* event) override;
    QSGNode* updatePaintNode(QSGNode* old_node, UpdatePaintNodeData* data) override;
    void itemChange(ItemChange change, const ItemChangeData& data) override;

    QVariantList m_page_sizes_pt;
    QVariantMap m_page_margins_pt;
    qreal m_zoom = 1.0;
    qreal m_page_gap_pt = 24.0;
    qreal m_page_margin_pt = 24.0;
    QColor m_page_color;
    QColor m_page_border_color;
    QVariantMap m_caret_geometry;
    QVariantList m_glyph_frames;
    quint64 m_glyph_frame_revision = 0;
    quint64 m_glyph_geometry_revision = 0;
    QColor m_caret_color;
    int m_caret_width_centipixels = 0;
    bool m_caret_blink_enabled = true;
    QTimer m_caret_blink_timer;
    bool m_caret_visible = false;
    qreal m_widest_page_pt = 0.0;
    qreal m_pages_height_pt = 0.0;
    QVector<qreal> m_page_y_origins_pt;
};

} // namespace caret
