#pragma once

#include <QObject>
#include <QSet>
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
    Q_INVOKABLE [[nodiscard]] auto containsFamily(const QString& family) const -> bool;

  private:
    const QString m_fixedFamily;
    const QSet<QString> m_familyNames;
};

} // namespace caret
