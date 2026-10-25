/// 不链接 launcher、FFI 或应用服务。注入快照后确认 Caret.Shell 可导入，
/// 验证主题切换绑定、未知图标拒绝着色，以及新建文档窗口的字段边界。
#include "icon_provider.hpp"
#include "theme_view_model.hpp"
#include <QByteArray>
#include <QColor>
#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QHash>
#include <QObject>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QQuickImageProvider>
#include <QRegularExpression>
#include <QString>
#include <QVariantMap>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickControls2/qquickstyle.h>
#include <QtTest/qtest.h>
#include <qcontainerfwd.h>
#include <qtestcase.h>
#include <qtmetamacros.h>

Q_IMPORT_QML_PLUGIN(Caret_ShellPlugin)

namespace {

QVariantMap snapshot_from_theme_facade(const QString& path, const QColor& background, int body) {
    QFile file(path);
    QVariantMap tokens;
    if (!file.open(QIODevice::ReadOnly)) {
        return tokens;
    }
    const QString source = QString::fromUtf8(file.readAll());
    static const QRegularExpression declaration(
        QStringLiteral("readonly property (color|int|real|string) (\\w+)"));
    auto match = declaration.globalMatch(source);
    while (match.hasNext()) {
        const auto found = match.next();
        const QString kind = found.captured(1);
        const QString name = found.captured(2);
        if (kind == QStringLiteral("color")) {
            tokens.insert(name, QColor(QStringLiteral("#808080")));
        } else if (kind == QStringLiteral("real")) {
            tokens.insert(name, 0.5);
        } else if (kind == QStringLiteral("string")) {
            tokens.insert(name, QStringLiteral("caret"));
        } else {
            tokens.insert(name, 16);
        }
    }
    tokens.insert(QStringLiteral("colorBackground"), background);
    tokens.insert(QStringLiteral("fontBody"), body);
    return tokens;
}

} // namespace

class ShellLoadTest : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase();
    void moduleImportsAndThemeBindingTracksSnapshot();
    void unknownIconRefusesColoring();
    void newDocumentDialogHasNoProcessFields();
};

void ShellLoadTest::initTestCase() { QQuickStyle::setStyle(QStringLiteral("Basic")); }

void ShellLoadTest::moduleImportsAndThemeBindingTracksSnapshot() {
    const QString theme =
        QString::fromUtf8(CARET_SOURCE_DIR) + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"),
                  snapshot_from_theme_facade(theme, QColor(QStringLiteral("#ffffff")), 16));
    themes.insert(QStringLiteral("dark"),
                  snapshot_from_theme_facade(theme, QColor(QStringLiteral("#111111")), 18));
    QVERIFY(themes.value(QStringLiteral("light")).contains(QStringLiteral("colorBackground")));
    caret::install_theme_snapshots(themes, QStringLiteral("light"));

    QQmlApplicationEngine engine;
    caret::install_icon_provider(engine);
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Shell\n"
                          "Item {\n"
                          "    objectName: \"probe\"\n"
                          "    property color background: Theme.colorBackground\n"
                          "    property int body: Theme.fontBody\n"
                          "    ThemedLabel { objectName: \"label\"; text: \"Caret\" }\n"
                          "}\n"),
        QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* probe = component.create();
    QVERIFY(probe != nullptr);
    QCOMPARE(probe->property("background").value<QColor>(), QColor(QStringLiteral("#ffffff")));
    QCOMPARE(probe->property("body").toInt(), 16);

    auto* view_model =
        engine.singletonInstance<caret::ThemeViewModel*>("Caret.Shell", "ThemeViewModel");
    QVERIFY(view_model != nullptr);
    QVERIFY(view_model->selectTheme(QStringLiteral("dark")));
    QCOMPARE(probe->property("background").value<QColor>(), QColor(QStringLiteral("#111111")));
    QCOMPARE(probe->property("body").toInt(), 18);
    QVERIFY(view_model->selectTheme(QStringLiteral("light")));
    QCOMPARE(probe->property("background").value<QColor>(), QColor(QStringLiteral("#ffffff")));
    QVERIFY(!view_model->selectTheme(QStringLiteral("missing")));
    QCOMPARE(probe->property("background").value<QColor>(), QColor(QStringLiteral("#ffffff")));
    delete probe;
}

void ShellLoadTest::unknownIconRefusesColoring() {
    QQmlEngine engine;
    caret::install_icon_provider(engine);
    auto* provider = dynamic_cast<QQuickImageProvider*>(engine.imageProvider("caret-icons"));
    QVERIFY(provider != nullptr);
    QSize size;
    const QImage known =
        provider->requestImage(QStringLiteral("close/ff112233"), &size, QSize(16, 16));
    QVERIFY(!known.isNull());
    const QImage unknown =
        provider->requestImage(QStringLiteral("mesh-gate/ff112233"), &size, QSize(16, 16));
    QVERIFY(unknown.isNull());
}

void ShellLoadTest::newDocumentDialogHasNoProcessFields() {
    const QString root = QString::fromUtf8(CARET_SOURCE_DIR);
    QDir dialogs(root + QStringLiteral("/qml/Dialogs"));
    const QStringList names =
        dialogs.entryList(QStringList{QStringLiteral("*.qml")}, QDir::Files, QDir::Name);
    QCOMPARE(names, QStringList({QStringLiteral("LanguageSettingsPage.qml"),
                                 QStringLiteral("NewProjectDialog.qml"),
                                 QStringLiteral("SettingsDialog.qml")}));
    QFile source(dialogs.filePath(QStringLiteral("NewProjectDialog.qml")));
    QVERIFY(source.open(QIODevice::ReadOnly));
    const QString text = QString::fromUtf8(source.readAll());
    QVERIFY(!text.contains(QStringLiteral("mold")));
    QVERIFY(!text.contains(QStringLiteral("gate")));
    QVERIFY(!text.contains(QStringLiteral("material")));
    QVERIFY(!text.contains(QStringLiteral("panta")));
    QVERIFY(!text.contains(QStringLiteral("Bridge")));
    QVERIFY(text.contains(QStringLiteral("document.empty_name")));

    const QString theme = root + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"),
                  snapshot_from_theme_facade(theme, QColor(QStringLiteral("#ffffff")), 16));
    caret::install_theme_snapshots(themes, QStringLiteral("light"));
    QQmlApplicationEngine engine;
    caret::install_icon_provider(engine);
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(QByteArrayLiteral("import QtQuick\n"
                                        "import Caret.Shell\n"
                                        "NewProjectDialog { objectName: \"newDocument\" }\n"),
                      QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* dialog = component.create();
    QVERIFY(dialog != nullptr);
    QVERIFY(!dialog->property("moldTemperature").isValid());
    QVERIFY(QMetaObject::invokeMethod(dialog, "open"));
    QVERIFY(QMetaObject::invokeMethod(dialog, "submit"));
    QCOMPARE(dialog->property("errorCode").toString(), QStringLiteral("document.empty_name"));
    delete dialog;
}

int main(int argc, char* argv[]) {
    const QGuiApplication app(argc, argv);
    ShellLoadTest test;
    return QTest::qExec(&test, argc, argv);
}

#include "shell_load_test.moc"
