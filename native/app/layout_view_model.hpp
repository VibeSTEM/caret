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
