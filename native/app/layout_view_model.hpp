/// 把 Rust 生成的只读页面快照转换为 QML 可绑定的值。
#pragma once

#include <QObject>
#include <QString>
#include <QVariantList>
#include <QtQml/qqmlregistration.h>

namespace caret {

/// 在创建 QML 引擎前安装 Rust 返回的页面快照 JSON。
bool install_layout_snapshot(QString snapshot_json);

class LayoutViewModel final : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(QVariantList pageSizesPt READ pageSizesPt CONSTANT)
    Q_PROPERTY(QString error READ error CONSTANT)

  public:
    explicit LayoutViewModel(QObject* parent = nullptr);

    [[nodiscard]] const QVariantList& pageSizesPt() const { return m_page_sizes_pt; }
    [[nodiscard]] const QString& error() const { return m_error; }

  private:
    QVariantList m_page_sizes_pt;
    QString m_error;
};

} // namespace caret
