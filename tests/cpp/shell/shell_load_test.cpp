/// 不链接 launcher、FFI 或应用服务。注入快照后确认 Caret.Shell 可导入，
/// 验证主题切换绑定、未知图标拒绝着色，以及新建文档窗口的字段边界。
#include "document_canvas.hpp"
#include "icon_provider.hpp"
#include "layout_view_model.hpp"
#include "theme_view_model.hpp"
#include <QByteArray>
#include <QColor>
#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QHash>
#include <QImage>
#include <QObject>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QQuickImageProvider>
#include <QQuickWindow>
#include <QRegularExpression>
#include <QString>
#include <QVariantMap>
#include <QtMath>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickControls2/qquickstyle.h>
#include <QtTest/qtest.h>
#include <algorithm>
#include <qcontainerfwd.h>
#include <qtestcase.h>
#include <qtmetamacros.h>
#include <utility>

Q_IMPORT_QML_PLUGIN(Caret_ShellPlugin)
Q_IMPORT_QML_PLUGIN(Caret_CanvasPlugin)
Q_IMPORT_QML_PLUGIN(Caret_ViewModelsPlugin)

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
    tokens.insert(QStringLiteral("colorPanel"), background);
    tokens.insert(QStringLiteral("colorPanelLine"), QColor(QStringLiteral("#c9c9c9")));
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
    void documentCanvasMapsPageCoordinatesAndHitsPages();
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
    const QImage zoom_out =
        provider->requestImage(QStringLiteral("zoom-out/ff112233"), &size, QSize(16, 16));
    QVERIFY(!zoom_out.isNull());
    const QImage zoom_in =
        provider->requestImage(QStringLiteral("zoom-in/ff112233"), &size, QSize(16, 16));
    QVERIFY(!zoom_in.isNull());
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

void ShellLoadTest::documentCanvasMapsPageCoordinatesAndHitsPages() {
    const QString theme_path =
        QString::fromUtf8(CARET_SOURCE_DIR) + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"),
                  snapshot_from_theme_facade(theme_path, QColor(Qt::white), 16));
    caret::install_theme_snapshots(std::move(themes), QStringLiteral("light"));
    QVERIFY(caret::install_layout_snapshot(QStringLiteral(
        R"({"marginsPt":{"top":72,"right":72,"bottom":72,"left":72},"pages":[{"widthPt":600,"heightPt":800}]})")));
    QVERIFY(!caret::install_layout_snapshot(QStringLiteral("{invalid")));
    QQmlEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent view_model_component(&engine);
    view_model_component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.ViewModels\n"
                          "Item { property var pageSizes: LayoutViewModel.pageSizesPt }\n"),
        QUrl());
    QVERIFY2(view_model_component.isReady(), qPrintable(view_model_component.errorString()));
    QObject* view_model = view_model_component.create();
    QVERIFY(view_model != nullptr);
    const QVariantList page_sizes = view_model->property("pageSizes").toList();
    QCOMPARE(page_sizes.size(), 1);
    QCOMPARE(page_sizes.front().toSizeF(), QSizeF(600.0, 800.0));
    QQmlComponent component(&engine);
    component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Canvas\n"
                          "Item {\n"
                          "    DocumentCanvas {\n"
                          "        objectName: \"canvas\"\n"
                          "        pageSizesPt: [Qt.size(600, 800), Qt.size(300, 400)]\n"
                          "        zoom: 1.5\n"
                          "        width: implicitWidth + 100\n"
                          "    }\n"
                          "}\n"),
        QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* root = component.create();
    QVERIFY(root != nullptr);
    auto* canvas = root->findChild<caret::DocumentCanvas*>(QStringLiteral("canvas"));
    QVERIFY(canvas != nullptr);

    const QSizeF natural_size = canvas->naturalContentSize();
    QCOMPARE(natural_size.width(), 1296.0);
    QCOMPARE(natural_size.height(), 2544.0);
    const QPointF origin = canvas->pageOrigin(1);
    QCOMPARE(origin, QPointF(398.0, 1696.0));

    const QPointF document_point(12.0, 20.0);
    const QPointF content_point = canvas->documentToContent(1, document_point);
    QCOMPARE(content_point, QPointF(422.0, 1736.0));
    QCOMPARE(canvas->contentToDocument(1, content_point), document_point);
    const QVariantMap hit = canvas->hitTestPage(content_point);
    QCOMPARE(hit.value(QStringLiteral("pageIndex")).toInt(), 1);
    QCOMPARE(hit.value(QStringLiteral("xPt")).toReal(), document_point.x());
    QCOMPARE(hit.value(QStringLiteral("yPt")).toReal(), document_point.y());
    QCOMPARE(canvas->hitTestPage(QPointF(-1.0, -1.0)).value(QStringLiteral("pageIndex")).toInt(),
             -1);

    // QQuickItem 输入坐标是逻辑像素，pt 映射不接收 DPR。
    for (const qreal zoom : {0.5, 1.0, 2.0}) {
        canvas->setZoom(zoom);
        const QPointF zoomed = canvas->documentToContent(1, document_point);
        QCOMPARE(canvas->contentToDocument(1, zoomed), document_point);
    }

    QQmlComponent surface_component(&engine);
    surface_component.setData(QByteArrayLiteral("import QtQuick\n"
                                                "import Caret.Shell\n"
                                                "PageSurface { width: 500; height: 500 }\n"),
                              QUrl());
    QVERIFY2(surface_component.isReady(), qPrintable(surface_component.errorString()));
    QObject* surface = surface_component.create();
    QVERIFY(surface != nullptr);
    auto* bound_canvas =
        surface->findChild<caret::DocumentCanvas*>(QStringLiteral("documentCanvas"));
    QVERIFY(bound_canvas != nullptr);
    QCOMPARE(bound_canvas->pageSizesPt().size(), 1);
    QCOMPARE(bound_canvas->pageSizesPt().front().toSizeF(), QSizeF(600.0, 800.0));
    QCOMPARE(bound_canvas->pageColor(), QColor(Qt::white));
    QCOMPARE(bound_canvas->pageBorderColor(), QColor(QStringLiteral("#c9c9c9")));

    QQmlComponent render_component(&engine);
    render_component.setData(QByteArrayLiteral("import QtQuick\n"
                                               "import Caret.Canvas\n"
                                               "DocumentCanvas {\n"
                                               "    width: 200; height: 120\n"
                                               "    pageColor: \"#ffffff\"\n"
                                               "    pageBorderColor: \"#c8c8c8\"\n"
                                               "    pageSizesPt: [Qt.size(60, 60)]\n"
                                               "    pageMarginPt: 12; pageGapPt: 0\n"
                                               "}\n"),
                             QUrl());
    QVERIFY2(render_component.isReady(), qPrintable(render_component.errorString()));
    auto* rendered_canvas = qobject_cast<QQuickItem*>(render_component.create());
    QVERIFY(rendered_canvas != nullptr);
    QQuickWindow render_window;
    render_window.setColor(Qt::green);
    render_window.resize(200, 120);
    rendered_canvas->setParentItem(render_window.contentItem());
    render_window.show();
    QTRY_VERIFY_WITH_TIMEOUT(render_window.isExposed(), 3000);
    const QImage frame = render_window.grabWindow();
    QVERIFY(!frame.isNull());
    const auto pixel_at_logical_point = [&frame, &render_window](qreal x, qreal y) {
        const qreal scale_x = static_cast<qreal>(frame.width()) / render_window.width();
        const qreal scale_y = static_cast<qreal>(frame.height()) / render_window.height();
        return frame.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    QCOMPARE(pixel_at_logical_point(60.5, 50.0), QColor(QStringLiteral("#c8c8c8")));
    QCOMPARE(pixel_at_logical_point(100.0, 50.0), QColor(Qt::white));
    delete rendered_canvas;

    QObject* scroller = surface->findChild<QObject*>(QStringLiteral("documentScroller"));
    QVERIFY(scroller != nullptr);
    bound_canvas->setZoom(2.0);
    QCoreApplication::processEvents();
    const qreal previous_max_y =
        scroller->property("contentHeight").toReal() - scroller->property("height").toReal();
    QVERIFY(previous_max_y > 0.0);
    scroller->setProperty("contentY", previous_max_y);
    QVERIFY(QMetaObject::invokeMethod(surface, "setZoomAtViewportCenter",
                                      Q_ARG(QVariant, QVariant(0.5))));
    QCoreApplication::processEvents();
    const qreal new_max_y =
        scroller->property("contentHeight").toReal() - scroller->property("height").toReal();
    QVERIFY(scroller->property("contentY").toReal() <= std::max(0.0, new_max_y));
    delete surface;
    delete view_model;
    delete root;
}

int main(int argc, char* argv[]) {
    QQuickWindow::setSceneGraphBackend(QStringLiteral("software"));
    const QGuiApplication app(argc, argv);
    ShellLoadTest test;
    return QTest::qExec(&test, argc, argv);
}

#include "shell_load_test.moc"
