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
class QInputMethodEvent;

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
    Q_PROPERTY(QVariantMap inputMethodContext READ inputMethodContext WRITE setInputMethodContext
                   NOTIFY inputMethodContextChanged)
    Q_PROPERTY(QString preeditText READ preeditText NOTIFY preeditTextChanged)
    Q_PROPERTY(int preeditCursorPosition READ preeditCursorPosition NOTIFY preeditTextChanged)
    Q_PROPERTY(bool caretBlinkEnabled READ caretBlinkEnabled WRITE setCaretBlinkEnabled NOTIFY
                   caretBlinkEnabledChanged)
    Q_PROPERTY(
        QVariantList glyphFrames READ glyphFrames WRITE setGlyphFrames NOTIFY glyphFramesChanged)
    Q_PROPERTY(QVariantList visiblePageRange READ visiblePageRange WRITE setVisiblePageRange NOTIFY
                   visiblePageRangeChanged)
    Q_PROPERTY(QVariantList selectionRects READ selectionRects WRITE setSelectionRects NOTIFY
                   selectionRectsChanged)
    Q_PROPERTY(QVariantList selectionPreviewRects READ selectionPreviewRects WRITE
                   setSelectionPreviewRects NOTIFY selectionPreviewRectsChanged)
    Q_PROPERTY(
        QColor selectionColor READ selectionColor WRITE setSelectionColor NOTIFY appearanceChanged)
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
    [[nodiscard]] QVariantMap inputMethodContext() const;
    void setInputMethodContext(const QVariantMap& context);
    [[nodiscard]] QString preeditText() const;
    [[nodiscard]] int preeditCursorPosition() const;
    [[nodiscard]] bool caretBlinkEnabled() const;
    void setCaretBlinkEnabled(bool enabled);
    [[nodiscard]] QVariantList glyphFrames() const;
    void setGlyphFrames(const QVariantList& frames);
    /// 页面背景保留半开可视区间，避免为整篇文档创建场景图节点。
    [[nodiscard]] QVariantList visiblePageRange() const;
    void setVisiblePageRange(const QVariantList& range);
    [[nodiscard]] QVariantList selectionRects() const;
    void setSelectionRects(const QVariantList& rectangles);
    [[nodiscard]] QVariantList selectionPreviewRects() const;
    void setSelectionPreviewRects(const QVariantList& rectangles);
    [[nodiscard]] QColor selectionColor() const;
    void setSelectionColor(const QColor& color);
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
    Q_INVOKABLE void cancelPreedit();
    Q_INVOKABLE void updateInputMethodCursorRectangle();
    [[nodiscard]] QVariant inputMethodQuery(Qt::InputMethodQuery query) const override;

  signals:
    void textInputRequested(const QString& text);
    void editingCommandRequested(const QString& command_id);
    void visualNavigationRequested(bool forward, bool extend);
    void verticalNavigationRequested(int line_delta, bool extend);
    void lineEdgeNavigationRequested(bool end, bool extend);
    void clipboardCommandRequested(const QString& command);
    void inputMethodEventRequested(const QString& preedit, const QString& commit,
                                   int replacement_start, int replacement_length,
                                   int cursor_position_utf16);
    void preeditTextChanged();
    void pageSizesPtChanged();
    void pageMarginsPtChanged();
    void zoomChanged();
    void pageGapPtChanged();
    void pageMarginPtChanged();
    void caretGeometryChanged();
    void inputMethodContextChanged();
    void caretBlinkEnabledChanged();
    void glyphFramesChanged();
    void visiblePageRangeChanged();
    void selectionRectsChanged();
    void selectionPreviewRectsChanged();
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
    void inputMethodEvent(QInputMethodEvent* event) override;
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
    QVariantMap m_input_method_context;
    QString m_preedit_text;
    int m_preedit_cursor_position = 0;
    QVariantList m_glyph_frames;
    int m_first_visible_page = 0;
    int m_page_end_visible = -1;
    QVariantList m_selection_rects;
    QVariantList m_selection_preview_rects;
    quint64 m_glyph_frame_revision = 0;
    quint64 m_glyph_geometry_revision = 0;
    QColor m_caret_color;
    QColor m_selection_color;
    quint64 m_selection_revision = 0;
    int m_caret_width_centipixels = 0;
    bool m_caret_blink_enabled = true;
    QTimer m_caret_blink_timer;
    bool m_caret_visible = false;
    qreal m_widest_page_pt = 0.0;
    qreal m_pages_height_pt = 0.0;
    QVector<qreal> m_page_y_origins_pt;
};

} // namespace caret
