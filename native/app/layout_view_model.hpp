/// 把 Rust 生成的只读页面快照转换为 QML 可绑定的值。
#pragma once

#include <QObject>
#include <QString>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

namespace caret {

/// 在创建 QML 引擎前安装 Rust 返回的页面快照 JSON。
bool install_layout_snapshot(QString snapshot_json);

class LayoutViewModel final : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(QVariantList pageSizesPt READ pageSizesPt CONSTANT)
    Q_PROPERTY(QVariantList pageLayouts READ pageLayouts CONSTANT)
    Q_PROPERTY(QVariantMap pageMarginsPt READ pageMarginsPt CONSTANT)
    Q_PROPERTY(QVariantList paragraphLayouts READ paragraphLayouts CONSTANT)
    Q_PROPERTY(QString documentGeneration READ documentGeneration CONSTANT)
    Q_PROPERTY(QString fontGeneration READ fontGeneration CONSTANT)
    Q_PROPERTY(QString error READ error CONSTANT)

  public:
    explicit LayoutViewModel(QObject* parent = nullptr);

    [[nodiscard]] const QVariantList& pageSizesPt() const { return m_page_sizes_pt; }
    [[nodiscard]] const QVariantList& pageLayouts() const { return m_page_layouts; }
    [[nodiscard]] const QVariantMap& pageMarginsPt() const { return m_page_margins_pt; }
    [[nodiscard]] const QVariantList& paragraphLayouts() const { return m_paragraph_layouts; }
    [[nodiscard]] QString documentGeneration() const {
        return QString::number(m_document_generation);
    }
    [[nodiscard]] QString fontGeneration() const { return QString::number(m_font_generation); }
    [[nodiscard]] const QString& error() const { return m_error; }

    /// 将页面局部 pt 坐标交给 Rust 布局快照，返回带文档位置的命中结果。
    Q_INVOKABLE QVariantMap hitTestDocument(int page_index, qreal x_pt, qreal y_pt);
    /// 在 Rust 会话中将页面命中位置设为当前插入点。
    Q_INVOKABLE QVariantMap placeCaretAtPage(int page_index, qreal x_pt, qreal y_pt);
    /// 只请求当前可见页的 glyph 栅格；图像像素以 QByteArray 二进制返回。
    Q_INVOKABLE QVariantMap rasterizePageGlyphs(int page_index, qreal px_per_pt,
                                                qreal page_origin_device_x_px,
                                                qreal page_origin_device_y_px, int foreground_red,
                                                int foreground_green, int foreground_blue,
                                                int foreground_alpha);

  private:
    QVariantList m_page_sizes_pt;
    QVariantList m_page_layouts;
    QVariantMap m_page_margins_pt;
    QVariantList m_paragraph_layouts;
    qulonglong m_document_generation = 0;
    qulonglong m_font_generation = 0;
    QString m_error;
};

} // namespace caret
