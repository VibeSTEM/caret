// 验证内置代码字体资源不完整时会回滚已注册字形并返回空族名。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "code.hpp"
#include <QFontDatabase>
#include <QGuiApplication>
#include <QObject>
#include <QRegularExpression>
#include <QtCore/qlogging.h>
#include <QtTest/qtestcase.h>

class CodeFontFailureTest final : public QObject {
    Q_OBJECT

  private slots:
    void missingStyleRollsBackPreviouslyRegisteredFonts() {
        const auto families_before = QFontDatabase::families();
        const auto styles_before = QFontDatabase::styles(QStringLiteral("JetBrains Mono"));
        QTest::ignoreMessage(QtCriticalMsg,
                             QRegularExpression(QStringLiteral(
                                 ".*无法完整注册内置代码字体资源：.*JetBrainsMono-Bold.ttf.*")));

        QVERIFY(caret::fonts::load_code_font().isEmpty());

        QCOMPARE(QFontDatabase::families(), families_before);
        QCOMPARE(QFontDatabase::styles(QStringLiteral("JetBrains Mono")), styles_before);
    }
};

auto main(int argc, char** argv) -> int {
    QGuiApplication application(argc, argv);
    CodeFontFailureTest test;
    return QTest::qExec(&test, argc, argv);
}

#include "code_font_failure_test.moc"
