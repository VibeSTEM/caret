// 向 QML 提供内置代码字体族名和 Qt 可查询的字体目录。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

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
    Q_PROPERTY(QString codeFamily READ codeFamily CONSTANT)

  public:
    explicit PlatformFonts(QObject* parent = nullptr);
    [[nodiscard]] auto codeFamily() const -> const QString&;
    Q_INVOKABLE [[nodiscard]] auto containsFamily(const QString& family) const -> bool;

  private:
    const QString m_codeFamily;
    QSet<QString> m_familyNames;
};

} // namespace caret
