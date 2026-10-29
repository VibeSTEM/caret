// 验证主题目录的类型化 token 安装、无效输入拒绝及失败原子性。
#include "shell_load_test.hpp"

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "theme_catalog.hpp"
#include "theme_view_model.hpp"
#include <QByteArray>
#include <QColor>
#include <QTest>
#include <QtCore/qlist.h>
#include <QtCore/qstring.h>
#include <QtCore/qvariant.h>
#include <QtTest/qtestcase.h>

namespace {

class BuiltinThemeRestorer final {
  public:
    BuiltinThemeRestorer() {
        const rust::String catalog = caret::ffi::builtin_theme_catalog();
        m_catalog = QByteArray(catalog.data(), static_cast<qsizetype>(catalog.size()));
    }

    ~BuiltinThemeRestorer() {
        if (!m_catalog.isEmpty()) {
            (void)caret::install_theme_catalog(m_catalog);
        }
    }

  private:
    QByteArray m_catalog;
};

} // namespace

void ShellLoadTest::themeCatalogInstallsTypedTokensAndRejectsInvalidSnapshotsAtomically() {
    const BuiltinThemeRestorer restore_builtin_theme;
    const QByteArray valid = R"({
        "active":"custom",
        "themes":{"custom":{
            "accent":{"kind":"color","value":"#123456"},
            "count":{"kind":"int","value":3},
            "ratio":{"kind":"real","value":1.5},
            "enabled":{"kind":"bool","value":true}
        }}
    })";
    QVERIFY(caret::install_theme_catalog(valid));

    const caret::ThemeViewModel view_model;
    const auto& tokens = view_model.tokens();
    QCOMPARE(view_model.activeTheme(), QStringLiteral("custom"));
    QVERIFY(tokens.value(QStringLiteral("accent")).value<QColor>().isValid());
    QCOMPARE(tokens.value(QStringLiteral("accent")).value<QColor>(),
             QColor(QStringLiteral("#123456")));
    QCOMPARE(tokens.value(QStringLiteral("count")).toInt(), 3);
    QCOMPARE(tokens.value(QStringLiteral("ratio")).toDouble(), 1.5);
    QVERIFY(tokens.value(QStringLiteral("enabled")).toBool());

    const QList<QByteArray> invalid_catalogs = {
        QByteArrayLiteral("error:theme unavailable"),
        QByteArrayLiteral("not json"),
        QByteArrayLiteral("[]"),
        QByteArrayLiteral(R"({"themes":{}})"),
        QByteArrayLiteral(R"({"active":"missing","themes":{}})"),
        QByteArrayLiteral(R"({"active":"custom","themes":{"custom":null}})"),
        QByteArrayLiteral(R"({"active":"custom","themes":{"custom":{}}})"),
        QByteArrayLiteral(
            R"({"active":"custom","themes":{"custom":{"x":{"kind":"unknown","value":1}}}})"),
        QByteArrayLiteral(
            R"({"active":"custom","themes":{"custom":{"x":{"kind":"color","value":"bad"}}}})"),
        QByteArrayLiteral(
            R"({"active":"custom","themes":{"custom":{"x":{"kind":"int","value":"3"}}}})"),
        QByteArrayLiteral(
            R"({"active":"custom","themes":{"custom":{"x":{"kind":"bool","value":1}}}})")};
    for (const QByteArray& invalid : invalid_catalogs) {
        QVERIFY(!caret::install_theme_catalog(invalid));
        const caret::ThemeViewModel current_theme;
        QCOMPARE(current_theme.activeTheme(), QStringLiteral("custom"));
        QCOMPARE(current_theme.tokens().value(QStringLiteral("count")).toInt(), 3);
        QCOMPARE(current_theme.tokens().value(QStringLiteral("ratio")).toDouble(), 1.5);
        QVERIFY(current_theme.tokens().value(QStringLiteral("enabled")).toBool());
    }
}
#endif
