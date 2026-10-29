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
    void everySupportedDictionaryLoads();
    void zhHansDictionaryTranslates();
    void zhHantDictionaryTranslates();
    void japaneseDictionaryTranslates();
    void enBaselineDictionaryLoads();
};

void QmLoadTest::everySupportedDictionaryLoads() {
    for (const QString& locale : {QStringLiteral("en"), QStringLiteral("zh_Hans"),
                                  QStringLiteral("zh_Hant"), QStringLiteral("ja")}) {
        const QString resource = QStringLiteral(":/i18n/caret_%1.qm").arg(locale);
        QVERIFY2(QFile::exists(resource), qPrintable(resource));
        QTranslator translator;
        QVERIFY2(translator.load(resource), qPrintable(resource));
    }
}

void QmLoadTest::zhHansDictionaryTranslates() {
    QTranslator translator;
    QVERIFY(translator.load(QStringLiteral(":/i18n/caret_zh_Hans.qm")));
    QCOMPARE(translator.translate("DialogAction", "Cancel"), QStringLiteral("取消"));
    QCOMPARE(translator.translate("DialogAction", "OK"), QStringLiteral("确定"));
    QCOMPARE(translator.translate("WindowTitleBar", "Untitled"), QStringLiteral("未命名"));
    QCOMPARE(translator.translate("WindowTitleBar", "Saving"), QStringLiteral("保存中"));
    QCOMPARE(translator.translate("WindowTitleBar", "Edited"), QStringLiteral("已编辑"));
    QCOMPARE(translator.translate("WindowTitleBar", "Saved"), QStringLiteral("已保存"));
}

void QmLoadTest::zhHantDictionaryTranslates() {
    QTranslator translator;
    QVERIFY(translator.load(QStringLiteral(":/i18n/caret_zh_Hant.qm")));
    QCOMPARE(translator.translate("DialogAction", "Cancel"), QStringLiteral("取消"));
    QCOMPARE(translator.translate("DialogAction", "OK"), QStringLiteral("確定"));
    QCOMPARE(translator.translate("WindowTitleBar", "Untitled"), QStringLiteral("未命名"));
    QCOMPARE(translator.translate("WindowTitleBar", "Saving"), QStringLiteral("儲存中"));
    QCOMPARE(translator.translate("WindowTitleBar", "Edited"), QStringLiteral("已編輯"));
    QCOMPARE(translator.translate("WindowTitleBar", "Saved"), QStringLiteral("已儲存"));
}

void QmLoadTest::japaneseDictionaryTranslates() {
    QTranslator translator;
    QVERIFY(translator.load(QStringLiteral(":/i18n/caret_ja.qm")));
    QCOMPARE(translator.translate("DialogAction", "Cancel"), QStringLiteral("キャンセル"));
    QCOMPARE(translator.translate("DialogAction", "OK"), QStringLiteral("OK"));
    QCOMPARE(translator.translate("WindowTitleBar", "Untitled"), QStringLiteral("無題"));
    QCOMPARE(translator.translate("WindowTitleBar", "Saving"), QStringLiteral("保存中"));
    QCOMPARE(translator.translate("WindowTitleBar", "Edited"), QStringLiteral("編集済み"));
    QCOMPARE(translator.translate("WindowTitleBar", "Saved"), QStringLiteral("保存済み"));
}

void QmLoadTest::enBaselineDictionaryLoads() {
    QVERIFY2(QFile::exists(QStringLiteral(":/i18n/caret_en.qm")), "英文 QM 未注册");
    QTranslator translator;
    QVERIFY(translator.load(QStringLiteral(":/i18n/caret_en.qm")));
    QCOMPARE(translator.translate("DialogAction", "Cancel"), QStringLiteral("Cancel"));
    QCOMPARE(translator.translate("DialogAction", "OK"), QStringLiteral("OK"));
}

QTEST_GUILESS_MAIN(QmLoadTest)
#include "qm_load_test.moc"
