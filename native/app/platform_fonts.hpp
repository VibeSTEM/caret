#pragma once

#include <QObject>
#include <QString>
#include <QtQml/qqmlregistration.h>

namespace caret {

/// GUI 线程查询的平台字体；每个 QML 引擎持有自己的只读实例。
class PlatformFonts final : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(QString fixedFamily READ fixedFamily CONSTANT)

  public:
    explicit PlatformFonts(QObject* parent = nullptr);
    [[nodiscard]] auto fixedFamily() const -> const QString&;

  private:
    const QString m_fixedFamily;
};

} // namespace caret
