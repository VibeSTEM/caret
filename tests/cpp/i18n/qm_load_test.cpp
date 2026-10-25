/// 验证语言 .pa 生成的 QM 能从构建树资源加载。
#include <QFile>
#include <QObject>
#include <QString>
#include <QTranslator>
#include <QtTest/qtest.h>
#include <qtestcase.h>
#include <qtmetamacros.h>

class QmLoadTest : public QObject {
    Q_OBJECT

  private slots:
    void zhCnDictionaryTranslates();
    void enBaselineDictionaryLoads();
};

void QmLoadTest::zhCnDictionaryTranslates() {
    QVERIFY2(QFile::exists(QStringLiteral(":/i18n/caret_zh_CN.qm")),
             "qrc 资源未注册：检查 caret_i18n 是否被链接");
    QTranslator translator;
    QVERIFY(translator.load(QStringLiteral(":/i18n/caret_zh_CN.qm")));
    QCOMPARE(translator.translate("DialogAction", "Cancel"), QStringLiteral("取消"));
    QCOMPARE(translator.translate("DialogAction", "OK"), QStringLiteral("确定"));
}

void QmLoadTest::enBaselineDictionaryLoads() {
    QVERIFY2(QFile::exists(QStringLiteral(":/i18n/caret_en.qm")), "英文 QM 未注册");
    QTranslator translator;
    QVERIFY(translator.load(QStringLiteral(":/i18n/caret_en.qm")));
    QCOMPARE(translator.translate("DialogAction", "Cancel"), QStringLiteral("Cancel"));
    QCOMPARE(translator.translate("CaretWindow", "Caret"), QStringLiteral("Caret"));
}

QTEST_GUILESS_MAIN(QmLoadTest)
#include "qm_load_test.moc"
