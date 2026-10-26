/// 不链接 launcher、FFI 或应用服务。注入快照后确认 Caret.Shell 可导入，
/// 验证主题切换绑定、未知图标拒绝着色，以及新建文档窗口的字段边界。
#include "document_canvas.hpp"
#include "icon_provider.hpp"
#include "layout_view_model.hpp"
#include "theme_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime.hpp"
#endif
#include <QByteArray>
#include <QChar>
#include <QColor>
#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QHash>
#include <QImage>
#include <QKeyEvent>
#include <QList>
#include <QObject>
#include <QPointF>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QQuickImageProvider>
#include <QQuickItem>
#include <QQuickWindow>
#include <QRect>
#include <QRectF>
#include <QRegularExpression>
#include <QScopeGuard>
#include <QSignalSpy>
#include <QSize>
#include <QString>
#include <QStyleHints>
#include <QVariantList>
#include <QVariantMap>
#include <QWindow>
#include <Qt>
#include <QtCore/qglobal.h>
#include <QtCore/qobject.h>
#include <QtCore/qobjectdefs.h>
#include <QtCore/qvariant.h>
#include <QtMath>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickControls2/qquickstyle.h>
#include <QtTest/QTest>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <QtTest/qtestkeyboard.h>
#include <QtTest/qtestmouse.h>
#include <algorithm>
#include <limits>
#include <qcontainerfwd.h>
#include <qtestcase.h>
#include <qtestsupport_core.h>
#include <qtmetamacros.h>
#include <string_view>
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
    tokens.insert(QStringLiteral("colorCaret"), QColor(QStringLiteral("#1c1c1c")));
    tokens.insert(QStringLiteral("caretWidthCentipixels"), 100);
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
#ifdef CARET_ENABLE_APP
    void documentHitTestReturnsRustPosition();
#endif
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
    constexpr int kTestCursorFlashPeriodMs = 100;
    constexpr int kTestBlinkTransitionTimeoutMs = 500;
    constexpr int kDisabledBlinkObservationMs = 250;

    const QString theme_path =
        QString::fromUtf8(CARET_SOURCE_DIR) + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"),
                  snapshot_from_theme_facade(theme_path, QColor(Qt::white), 16));
    caret::install_theme_snapshots(std::move(themes), QStringLiteral("light"));
    const QString empty_snapshot = QStringLiteral(
        R"({"documentGeneration":"0","fontGeneration":"1","marginsPt":{"top":72,"right":72,"bottom":72,"left":72},"pages":[{"widthPt":600,"heightPt":800,"fragments":[{"paragraphId":"p1","firstLine":0,"lineEnd":1,"yOffsetPt":0}]}],"paragraphs":[{"paragraphId":"p1","text":"","lines":[{"sourceStart":0,"sourceEnd":0,"consumedStart":0,"consumedEnd":0,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":0,"overflowsWidth":false}]}]})");
    QString max_generation_snapshot = empty_snapshot;
    max_generation_snapshot.replace(
        QStringLiteral("\"documentGeneration\":\"0\""),
        QStringLiteral("\"documentGeneration\":\"18446744073709551615\""));
    QVERIFY(caret::install_layout_snapshot(max_generation_snapshot));
    const caret::LayoutViewModel max_generation_model;
    const QString max_generation = QString::number(std::numeric_limits<qulonglong>::max());
    QCOMPARE(max_generation_model.documentGeneration(), max_generation);
    QQmlEngine generation_engine;
    generation_engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent generation_component(&generation_engine);
    generation_component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.ViewModels\n"
                          "Item { property var generation: LayoutViewModel.documentGeneration }\n"),
        QUrl());
    QVERIFY2(generation_component.isReady(), qPrintable(generation_component.errorString()));
    QObject* generation_object = generation_component.create();
    QVERIFY(generation_object != nullptr);
    QCOMPARE(generation_object->property("generation").toString(), max_generation);
    delete generation_object;
    QVERIFY(caret::install_layout_snapshot(empty_snapshot));

    QVERIFY(!caret::install_layout_snapshot(QStringLiteral("{invalid")));
    QString unsafe_index_snapshot = empty_snapshot;
    unsafe_index_snapshot.replace(QStringLiteral("\"sourceStart\":0"),
                                  QStringLiteral("\"sourceStart\":9223372036854775808"));
    QVERIFY(!caret::install_layout_snapshot(unsafe_index_snapshot));
    const QString split_utf8_snapshot = QStringLiteral(
        R"({"documentGeneration":"0","fontGeneration":"1","marginsPt":{"top":72,"right":72,"bottom":72,"left":72},"pages":[],"paragraphs":[{"paragraphId":"p1","text":"éx","lines":[{"sourceStart":0,"sourceEnd":1,"consumedStart":0,"consumedEnd":1,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":6,"overflowsWidth":false},{"sourceStart":1,"sourceEnd":3,"consumedStart":1,"consumedEnd":3,"topPt":12,"baselinePt":21,"heightPt":12,"advancePt":6,"overflowsWidth":false}]}]})");
    QVERIFY(!caret::install_layout_snapshot(split_utf8_snapshot));
    const QString overlapping_lines_snapshot = QStringLiteral(
        R"({"documentGeneration":"0","fontGeneration":"1","marginsPt":{"top":72,"right":72,"bottom":72,"left":72},"pages":[],"paragraphs":[{"paragraphId":"p1","text":"ab","lines":[{"sourceStart":0,"sourceEnd":1,"consumedStart":0,"consumedEnd":1,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":6,"overflowsWidth":false},{"sourceStart":1,"sourceEnd":2,"consumedStart":1,"consumedEnd":2,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":6,"overflowsWidth":false}]}]})");
    QVERIFY(!caret::install_layout_snapshot(overlapping_lines_snapshot));
    QString negative_line_top = empty_snapshot;
    negative_line_top.replace(QStringLiteral("\"topPt\":0"), QStringLiteral("\"topPt\":-1"));
    QVERIFY(!caret::install_layout_snapshot(negative_line_top));
    QString outside_page_fragment = empty_snapshot;
    outside_page_fragment.replace(QStringLiteral("\"yOffsetPt\":0"),
                                  QStringLiteral("\"yOffsetPt\":1e300"));
    QVERIFY(!caret::install_layout_snapshot(outside_page_fragment));
    const caret::LayoutViewModel retained_snapshot_model;
    QCOMPARE(retained_snapshot_model.documentGeneration(), QStringLiteral("0"));
    QCOMPARE(retained_snapshot_model.fontGeneration(), QStringLiteral("1"));
    QQmlEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent view_model_component(&engine);
    view_model_component.setData(
        QByteArrayLiteral(
            "import QtQuick\n"
            "import Caret.ViewModels\n"
            "Item {\n"
            "    property var pageSizes: LayoutViewModel.pageSizesPt\n"
            "    property var pages: LayoutViewModel.pageLayouts\n"
            "    property var paragraphs: LayoutViewModel.paragraphLayouts\n"
            "    property string documentGeneration: LayoutViewModel.documentGeneration\n"
            "    property string fontGeneration: LayoutViewModel.fontGeneration\n"
            "}\n"),
        QUrl());
    QVERIFY2(view_model_component.isReady(), qPrintable(view_model_component.errorString()));
    QObject* view_model = view_model_component.create();
    QVERIFY(view_model != nullptr);
    const QVariantList page_sizes = view_model->property("pageSizes").toList();
    QCOMPARE(page_sizes.size(), 1);
    QCOMPARE(page_sizes.front().toSizeF(), QSizeF(600.0, 800.0));
    QCOMPARE(view_model->property("pages").toList().size(), 1);
    QCOMPARE(view_model->property("paragraphs").toList().size(), 1);
    QCOMPARE(view_model->property("documentGeneration").toString(), QStringLiteral("0"));
    QCOMPARE(view_model->property("fontGeneration").toString(), QStringLiteral("1"));
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
    QSignalSpy text_input_spy(canvas, &caret::DocumentCanvas::textInputRequested);
    QVERIFY(text_input_spy.isValid());
    QSignalSpy editing_command_spy(canvas, &caret::DocumentCanvas::editingCommandRequested);
    QVERIFY(editing_command_spy.isValid());
    QKeyEvent altgr_input(QEvent::KeyPress, Qt::Key_Q,
                          Qt::ControlModifier | Qt::AltModifier | Qt::GroupSwitchModifier,
                          QStringLiteral("@"));
    QCoreApplication::sendEvent(canvas, &altgr_input);
    QCOMPARE(text_input_spy.size(), 1);
    QCOMPARE(text_input_spy.takeFirst().at(0).toString(), QStringLiteral("@"));
    QKeyEvent altgr_without_group_switch(
        QEvent::KeyPress, Qt::Key_Q, Qt::ControlModifier | Qt::AltModifier, QStringLiteral("@"));
    QCoreApplication::sendEvent(canvas, &altgr_without_group_switch);
    QCOMPARE(text_input_spy.size(), 1);
    QCOMPARE(text_input_spy.takeFirst().at(0).toString(), QStringLiteral("@"));
    QKeyEvent shortcut_input(QEvent::KeyPress, Qt::Key_X, Qt::ControlModifier, QStringLiteral("x"));
    QCoreApplication::sendEvent(canvas, &shortcut_input);
    QCOMPARE(text_input_spy.size(), 0);
    QKeyEvent keypad_enter(QEvent::KeyPress, Qt::Key_Enter, Qt::KeypadModifier, QString());
    QCoreApplication::sendEvent(canvas, &keypad_enter);
    QCOMPARE(editing_command_spy.size(), 1);
    QCOMPARE(editing_command_spy.takeFirst().at(0).toString(),
             QStringLiteral("text.split_paragraph"));

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

    const QVariantMap caret_geometry{{QStringLiteral("pageIndex"), 1},
                                     {QStringLiteral("xPt"), 12.0},
                                     {QStringLiteral("y0Pt"), 20.0},
                                     {QStringLiteral("y1Pt"), 30.0}};
    canvas->setCaretGeometry(caret_geometry);
    const QVariantMap installed_caret = canvas->caretGeometry();
    QCOMPARE(installed_caret.value(QStringLiteral("pageIndex")).toInt(), 1);
    QVariantMap invalid_caret = caret_geometry;
    invalid_caret.insert(QStringLiteral("pageIndex"), 0.5);
    canvas->setCaretGeometry(invalid_caret);
    QCOMPARE(canvas->caretGeometry(), installed_caret);
    invalid_caret.insert(QStringLiteral("pageIndex"),
                         QVariant::fromValue<qulonglong>(4294967296ULL));
    canvas->setCaretGeometry(invalid_caret);
    QCOMPARE(canvas->caretGeometry(), installed_caret);

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
    QCOMPARE(bound_canvas->caretColor(), QColor(QStringLiteral("#1c1c1c")));
    QCOMPARE(bound_canvas->caretWidthCentipixels(), 100);

    QQmlComponent render_component(&engine);
    render_component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Canvas\n"
                          "DocumentCanvas {\n"
                          "    width: 200; height: 200\n"
                          "    pageColor: \"#ffffff\"\n"
                          "    pageBorderColor: \"#c8c8c8\"\n"
                          "    caretColor: \"#ff0000\"\n"
                          "    caretWidthCentipixels: 125\n"
                          "    pageSizesPt: [Qt.size(60, 60), Qt.size(60, 60)]\n"
                          "    caretGeometry: ({ pageIndex: 0, xPt: 30, y0Pt: 20, y1Pt: 80 })\n"
                          "    pageMarginPt: 12; pageGapPt: 0\n"
                          "}\n"),
        QUrl());
    QVERIFY2(render_component.isReady(), qPrintable(render_component.errorString()));
    auto* rendered_canvas = qobject_cast<QQuickItem*>(render_component.create());
    QVERIFY(rendered_canvas != nullptr);
    QCOMPARE(rendered_canvas->property("caretWidthCentipixels").toInt(), 125);
    constexpr qsizetype glyph_pixel_count = qsizetype{8} * 8;
    QByteArray glyph_pixels(glyph_pixel_count * 4, '\0');
    for (qsizetype pixel = 0; pixel < glyph_pixel_count; ++pixel) {
        const qsizetype x = pixel % 8;
        glyph_pixels[pixel * 4 + (x < 4 ? 0 : 1)] = static_cast<char>(255);
        glyph_pixels[pixel * 4 + 3] = static_cast<char>(255);
    }
    const QVariantMap glyph_frame{
        {QStringLiteral("valid"), true},
        {QStringLiteral("rasterPxPerPt"), 96.0 / 72.0},
        {QStringLiteral("pageIndex"), 0},
        {QStringLiteral("images"),
         QVariantList{QVariantMap{{QStringLiteral("width"), 8},
                                  {QStringLiteral("height"), 8},
                                  {QStringLiteral("left"), 0},
                                  {QStringLiteral("top"), 8},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("pixels"), glyph_pixels}},
                      QVariantMap{{QStringLiteral("width"), 0},
                                  {QStringLiteral("height"), 1},
                                  {QStringLiteral("left"), 0},
                                  {QStringLiteral("top"), 0},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("pixels"), QByteArray{}}}}},
        {QStringLiteral("placements"),
         QVariantList{QVariantMap{{QStringLiteral("imageIndex"), 0},
                                  {QStringLiteral("xPt"), 20.0},
                                  {QStringLiteral("baselineYPt"), 32.0},
                                  {QStringLiteral("pixelPhaseX"), 0.0},
                                  {QStringLiteral("pixelPhaseY"), 0.0},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("clipEnabled"), true},
                                  {QStringLiteral("clipY0Pt"), 28.0},
                                  {QStringLiteral("clipY1Pt"), 31.0}},
                      QVariantMap{{QStringLiteral("imageIndex"), 1},
                                  {QStringLiteral("xPt"), 30.0},
                                  {QStringLiteral("baselineYPt"), 32.0},
                                  {QStringLiteral("pixelPhaseX"), 0.0},
                                  {QStringLiteral("pixelPhaseY"), 0.0},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("clipEnabled"), false}}}},
    };
    QVariantMap second_glyph_frame = glyph_frame;
    second_glyph_frame.insert(QStringLiteral("pageIndex"), 1);
    QVERIFY(
        rendered_canvas->setProperty("glyphFrames", QVariantList{glyph_frame, second_glyph_frame}));
    QQuickWindow render_window;
    render_window.setColor(Qt::green);
    render_window.resize(200, 200);
    rendered_canvas->setParent(&render_window);
    rendered_canvas->setParentItem(render_window.contentItem());
    render_window.show();
    QTRY_VERIFY_WITH_TIMEOUT(render_window.isExposed(), 3000);
    rendered_canvas->forceActiveFocus(Qt::OtherFocusReason);
    QTRY_VERIFY(rendered_canvas->hasActiveFocus());
    const QImage frame = render_window.grabWindow();
    QVERIFY(!frame.isNull());
    const auto pixel_at_logical_point = [&frame, &render_window](qreal x, qreal y) {
        const qreal scale_x = static_cast<qreal>(frame.width()) / render_window.width();
        const qreal scale_y = static_cast<qreal>(frame.height()) / render_window.height();
        return frame.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    QCOMPARE(pixel_at_logical_point(60.5, 50.0), QColor(QStringLiteral("#c8c8c8")));
    QCOMPARE(pixel_at_logical_point(90.0, 50.0), QColor(Qt::white));
    QCOMPARE(pixel_at_logical_point(88.5, 55.0), QColor(Qt::red));
    QCOMPARE(pixel_at_logical_point(92.5, 55.0), QColor(Qt::green));
    QCOMPARE(pixel_at_logical_point(92.0, 51.0), QColor(Qt::white));
    QVERIFY(pixel_at_logical_point(101.0, 50.0) != QColor(Qt::white));
    QCOMPARE(pixel_at_logical_point(102.5, 50.0), QColor(Qt::white));
    QCOMPARE(pixel_at_logical_point(100.5, 95.5), QColor(QStringLiteral("#ff0000")));
    QCOMPARE(pixel_at_logical_point(101.0, 180.0), QColor(Qt::green));
    QCOMPARE(pixel_at_logical_point(88.5, 134.5), QColor(Qt::red));

    const auto current_caret_pixel = [&render_window]() {
        const QImage current_frame = render_window.grabWindow();
        return current_frame.pixelColor(
            qFloor(100.5 * current_frame.width() / render_window.width()),
            qFloor(50.0 * current_frame.height() / render_window.height()));
    };
    const int cursor_flash_time = QGuiApplication::styleHints()->cursorFlashTime();
    if (cursor_flash_time > 0) {
        QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                                 cursor_flash_time + 500);
        QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                                  cursor_flash_time + 500);
    }

    QStyleHints* style_hints = QGuiApplication::styleHints();
    const int original_cursor_flash_time = style_hints->cursorFlashTime();
    [[maybe_unused]] const auto restore_cursor_flash_time =
        qScopeGuard([style_hints, original_cursor_flash_time]() {
            style_hints->setCursorFlashTime(original_cursor_flash_time);
        });
    style_hints->setCursorFlashTime(kTestCursorFlashPeriodMs);
    QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                             kTestBlinkTransitionTimeoutMs);
    QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                              kTestBlinkTransitionTimeoutMs);
    rendered_canvas->setProperty("caretBlinkEnabled", false);
    QTRY_COMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    QTest::qWait(kDisabledBlinkObservationMs);
    QCOMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    rendered_canvas->setProperty("caretBlinkEnabled", true);
    QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                             kTestBlinkTransitionTimeoutMs);
    QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                              kTestBlinkTransitionTimeoutMs);
    style_hints->setCursorFlashTime(0);
    QTRY_COMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    QTest::qWait(kDisabledBlinkObservationMs);
    QCOMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    style_hints->setCursorFlashTime(kTestCursorFlashPeriodMs);
    QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                             kTestBlinkTransitionTimeoutMs);
    QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                              kTestBlinkTransitionTimeoutMs);

    rendered_canvas->setFocus(false);
    QTRY_VERIFY(!rendered_canvas->hasActiveFocus());
    const QImage unfocused_frame = render_window.grabWindow();
    QVERIFY(!unfocused_frame.isNull());
    QCOMPARE(unfocused_frame.pixelColor(qFloor(101.0 * frame.width() / render_window.width()),
                                        qFloor(50.0 * frame.height() / render_window.height())),
             QColor(Qt::white));

    QByteArray changed_glyph_pixels = glyph_pixels;
    for (qsizetype pixel = 0; pixel < glyph_pixel_count; ++pixel) {
        const qsizetype x = pixel % 8;
        if (x < 4) {
            changed_glyph_pixels[pixel * 4 + 0] = '\0';
            changed_glyph_pixels[pixel * 4 + 2] = static_cast<char>(255);
        }
    }
    QVariantMap changed_second_image =
        second_glyph_frame.value(QStringLiteral("images")).toList().front().toMap();
    changed_second_image.insert(QStringLiteral("pixels"), changed_glyph_pixels);
    QVariantMap changed_second_frame = second_glyph_frame;
    const QVariantList changed_images{
        changed_second_image,
        second_glyph_frame.value(QStringLiteral("images")).toList().at(1),
    };
    changed_second_frame.insert(QStringLiteral("images"), changed_images);
    const QVariantList partially_changed_frames{glyph_frame, changed_second_frame};
    QVERIFY(rendered_canvas->setProperty("glyphFrames", partially_changed_frames));
    QCOMPARE(rendered_canvas->property("glyphFrames").toList(), partially_changed_frames);
    QVariantMap fractional_width_image = changed_second_image;
    fractional_width_image.insert(QStringLiteral("width"), 8.5);
    QVariantMap malformed_second_frame = changed_second_frame;
    QVariantList malformed_images = changed_images;
    malformed_images[0] = fractional_width_image;
    malformed_second_frame.insert(QStringLiteral("images"), malformed_images);
    QVERIFY(rendered_canvas->setProperty("glyphFrames",
                                         QVariantList{glyph_frame, malformed_second_frame}));
    QCOMPARE(rendered_canvas->property("glyphFrames").toList(), partially_changed_frames);
    QImage partial_update_frame;
    const auto partial_update_pixel = [&partial_update_frame, &render_window](qreal x, qreal y) {
        const qreal scale_x =
            static_cast<qreal>(partial_update_frame.width()) / render_window.width();
        const qreal scale_y =
            static_cast<qreal>(partial_update_frame.height()) / render_window.height();
        return partial_update_frame.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    QTRY_VERIFY_WITH_TIMEOUT(([&]() {
                                 partial_update_frame = render_window.grabWindow();
                                 return !partial_update_frame.isNull() &&
                                        partial_update_pixel(88.5, 134.5) == QColor(Qt::blue);
                             }()),
                             3000);
    QCOMPARE(partial_update_pixel(88.5, 55.0), QColor(Qt::red));
    QCOMPARE(partial_update_pixel(88.5, 134.5), QColor(Qt::blue));

    rendered_canvas->setWidth(240);
    QTRY_COMPARE_WITH_TIMEOUT(rendered_canvas->width(), 240.0, 3000);
    const QImage recentered_frame = render_window.grabWindow();
    QVERIFY(!recentered_frame.isNull());
    const auto recentered_pixel = [&recentered_frame, &render_window](qreal x, qreal y) {
        const qreal scale_x = static_cast<qreal>(recentered_frame.width()) / render_window.width();
        const qreal scale_y =
            static_cast<qreal>(recentered_frame.height()) / render_window.height();
        return recentered_frame.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    QCOMPARE(recentered_pixel(88.5, 55.0), QColor(Qt::white));
    QCOMPARE(recentered_pixel(108.5, 55.0), QColor(Qt::red));
    QCOMPARE(recentered_pixel(88.5, 134.5), QColor(Qt::white));
    QCOMPARE(recentered_pixel(108.5, 134.5), QColor(Qt::blue));

    rendered_canvas->setWidth(300);
    rendered_canvas->setProperty("zoom", 2.0);
    render_window.resize(300, 200);
    QTRY_COMPARE_WITH_TIMEOUT(rendered_canvas->width(), 300.0, 3000);
    const QImage zoomed_frame = render_window.grabWindow();
    QVERIFY(!zoomed_frame.isNull());
    const auto zoomed_pixel = [&zoomed_frame, &render_window](qreal x, qreal y) {
        const qreal scale_x = static_cast<qreal>(zoomed_frame.width()) / render_window.width();
        const qreal scale_y = static_cast<qreal>(zoomed_frame.height()) / render_window.height();
        return zoomed_frame.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    QCOMPARE(zoomed_pixel(126.0, 109.0), QColor(Qt::red));
    QCOMPARE(zoomed_pixel(134.0, 109.0), QColor(Qt::green));
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

#ifdef CARET_ENABLE_APP
void ShellLoadTest::documentHitTestReturnsRustPosition() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(snapshot.data(), snapshot.size()).starts_with("error:"));
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size()))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));

    QQmlEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent component(&engine);
    component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.ViewModels\n"
                          "Item {\n"
                          "    property var hit: LayoutViewModel.hitTestDocument(0, 73, 73)\n"
                          "    property var miss: LayoutViewModel.hitTestDocument(-1, 73, 73)\n"
                          "}\n"),
        QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* probe = component.create();
    QVERIFY(probe != nullptr);
    const QVariantMap hit = probe->property("hit").toMap();
    QVERIFY(hit.value(QStringLiteral("valid")).toBool());
    QVERIFY(!probe->property("miss").toMap().value(QStringLiteral("valid")).toBool());
    QCOMPARE(hit.value(QStringLiteral("documentGeneration")).toString(), QStringLiteral("0"));
    const QVariantMap position = hit.value(QStringLiteral("position")).toMap();
    QCOMPARE(position.value(QStringLiteral("paragraphId")).toString().isEmpty(), false);
    QCOMPARE(position.value(QStringLiteral("offsetUtf8")).toString(), QStringLiteral("0"));
    QCOMPARE(position.value(QStringLiteral("affinity")).toString(), QStringLiteral("downstream"));
    QCOMPARE(
        position.value(QStringLiteral("story")).toMap().value(QStringLiteral("kind")).toString(),
        QStringLiteral("body"));
    delete probe;

    QQmlComponent surface_component(&engine);
    surface_component.setData(QByteArrayLiteral("import QtQuick\n"
                                                "import Caret.Shell\n"
                                                "PageSurface { width: 500; height: 500 }\n"),
                              QUrl());
    QVERIFY2(surface_component.isReady(), qPrintable(surface_component.errorString()));
    auto* surface = qobject_cast<QQuickItem*>(surface_component.create());
    QVERIFY(surface != nullptr);
    auto* canvas = surface->findChild<caret::DocumentCanvas*>(QStringLiteral("documentCanvas"));
    QVERIFY(canvas != nullptr);

    QQuickWindow window;
    window.resize(500, 500);
    surface->setParentItem(window.contentItem());
    QSignalSpy position_spy(surface, SIGNAL(documentPositionHit(QVariant)));
    QVERIFY(position_spy.isValid());
    window.show();
    QTRY_VERIFY_WITH_TIMEOUT(window.isExposed(), 3000);
    const QSizeF page_size = canvas->pageSizesPt().front().toSizeF();
    const QPointF page_center(std::min(100.0, page_size.width()), 140.0);
    const QPointF content_point = canvas->documentToContent(0, page_center);
    const QVariantMap page_hit = canvas->hitTestPage(content_point);
    QCOMPARE(page_hit.value(QStringLiteral("pageIndex")).toInt(), 0);
    auto* layout_view_model =
        engine.singletonInstance<caret::LayoutViewModel*>("Caret.ViewModels", "LayoutViewModel");
    QVERIFY(layout_view_model != nullptr);
    const QVariantMap rust_hit =
        layout_view_model->hitTestDocument(page_hit.value(QStringLiteral("pageIndex")).toInt(),
                                           page_hit.value(QStringLiteral("xPt")).toReal(),
                                           page_hit.value(QStringLiteral("yPt")).toReal());
    QVERIFY2(rust_hit.value(QStringLiteral("valid")).toBool(),
             qPrintable(rust_hit.value(QStringLiteral("error")).toString()));
    const QPointF window_point = canvas->mapToItem(window.contentItem(), content_point);
    QTest::mouseClick(&window, Qt::LeftButton, Qt::NoModifier, window_point.toPoint());
    QTRY_COMPARE_WITH_TIMEOUT(position_spy.size(), 1, 3000);
    QTRY_VERIFY_WITH_TIMEOUT(canvas->hasActiveFocus(), 3000);
    const QVariantMap clicked_hit = position_spy.takeFirst().at(0).toMap();
    QVERIFY(clicked_hit.value(QStringLiteral("valid")).toBool());
    QCOMPARE(clicked_hit.value(QStringLiteral("documentGeneration")).toString(),
             QStringLiteral("0"));
    const qreal initial_caret_x = canvas->caretGeometry().value(QStringLiteral("xPt")).toReal();
    QSignalSpy text_input_spy(canvas, &caret::DocumentCanvas::textInputRequested);
    QVERIFY(text_input_spy.isValid());
    QSignalSpy editing_command_spy(canvas, &caret::DocumentCanvas::editingCommandRequested);
    QVERIFY(editing_command_spy.isValid());
    QTest::keyClick(&window, Qt::Key_X);
    QTRY_COMPARE_WITH_TIMEOUT(text_input_spy.size(), 1, 3000);
    QCOMPARE(text_input_spy.takeFirst().at(0).toString(), QStringLiteral("x"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("1"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x"));
    QTRY_VERIFY_WITH_TIMEOUT(
        canvas->caretGeometry().value(QStringLiteral("xPt")).toReal() > initial_caret_x, 3000);
    QTRY_VERIFY_WITH_TIMEOUT(!canvas->glyphFrames().isEmpty() &&
                                 canvas->glyphFrames()
                                         .first()
                                         .toMap()
                                         .value(QStringLiteral("documentGeneration"))
                                         .toString() == QStringLiteral("1") &&
                                 !canvas->glyphFrames()
                                      .first()
                                      .toMap()
                                      .value(QStringLiteral("placements"))
                                      .toList()
                                      .isEmpty(),
                             3000);
    const QVariantMap updated_glyph_frame = canvas->glyphFrames().first().toMap();
    QCOMPARE(updated_glyph_frame.value(QStringLiteral("documentGeneration")).toString(),
             QStringLiteral("1"));
    QVERIFY(!updated_glyph_frame.value(QStringLiteral("placements")).toList().isEmpty());

    const auto rendered_glyph_has_ink = [&](qsizetype placement_index) {
        const QImage frame = window.grabWindow();
        if (frame.isNull()) {
            return false;
        }
        const QVariantMap current_frame = canvas->glyphFrames().first().toMap();
        const QVariantList images = current_frame.value(QStringLiteral("images")).toList();
        const QVariantList placements = current_frame.value(QStringLiteral("placements")).toList();
        if (images.isEmpty() || placement_index < 0 || placement_index >= placements.size()) {
            return false;
        }
        const QVariantMap placement = placements.at(placement_index).toMap();
        const quint32 image_index = placement.value(QStringLiteral("imageIndex")).toUInt();
        if (image_index >= static_cast<quint32>(images.size())) {
            return false;
        }
        const QVariantMap image = images.at(static_cast<qsizetype>(image_index)).toMap();
        const bool phase_applied = placement.value(QStringLiteral("pixelPhaseApplied")).toBool();
        const qreal phase_x =
            phase_applied ? placement.value(QStringLiteral("pixelPhaseX")).toReal() : 0.0;
        const qreal phase_y =
            phase_applied ? placement.value(QStringLiteral("pixelPhaseY")).toReal() : 0.0;
        const qreal image_scale = canvas->zoom() * 96.0 / 72.0 /
                                  current_frame.value(QStringLiteral("rasterPxPerPt")).toReal();
        const QPointF baseline = canvas->mapToItem(
            window.contentItem(),
            canvas->documentToContent(
                0, QPointF(placement.value(QStringLiteral("xPt")).toReal(),
                           placement.value(QStringLiteral("baselineYPt")).toReal())));
        const QRectF glyph_rect(
            baseline.x() + (image.value(QStringLiteral("left")).toReal() - phase_x) * image_scale,
            baseline.y() + (-image.value(QStringLiteral("top")).toReal() - phase_y) * image_scale,
            image.value(QStringLiteral("width")).toReal() * image_scale,
            image.value(QStringLiteral("height")).toReal() * image_scale);
        const qreal pixels_per_x = static_cast<qreal>(frame.width()) / window.width();
        const qreal pixels_per_y = static_cast<qreal>(frame.height()) / window.height();
        const QRect pixel_rect(
            qFloor(glyph_rect.left() * pixels_per_x), qFloor(glyph_rect.top() * pixels_per_y),
            qCeil(glyph_rect.width() * pixels_per_x), qCeil(glyph_rect.height() * pixels_per_y));
        const QRect clipped = pixel_rect.intersected(frame.rect());
        if (clipped.isEmpty()) {
            return false;
        }
        for (int y = clipped.top(); y <= clipped.bottom(); ++y) {
            for (int x = clipped.left(); x <= clipped.right(); ++x) {
                if (frame.pixelColor(x, y).lightness() < 240) {
                    return true;
                }
            }
        }
        return false;
    };
    QTRY_VERIFY_WITH_TIMEOUT(rendered_glyph_has_ink(0), 3000);

    const qreal latin_baseline_before_fallback =
        updated_glyph_frame.value(QStringLiteral("placements"))
            .toList()
            .first()
            .toMap()
            .value(QStringLiteral("baselineYPt"))
            .toReal();
    const QVariantMap fullwidth_insert = layout_view_model->insertText(QStringLiteral("！"));
    QVERIFY2(fullwidth_insert.value(QStringLiteral("valid")).toBool(),
             qPrintable(fullwidth_insert.value(QStringLiteral("error")).toString()));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("2"), 3000);
    QTRY_VERIFY_WITH_TIMEOUT(!canvas->glyphFrames().isEmpty() &&
                                 canvas->glyphFrames()
                                         .first()
                                         .toMap()
                                         .value(QStringLiteral("documentGeneration"))
                                         .toString() == QStringLiteral("2") &&
                                 canvas->glyphFrames()
                                         .first()
                                         .toMap()
                                         .value(QStringLiteral("placements"))
                                         .toList()
                                         .size() >= 2,
                             3000);
    const QVariantMap mixed_glyph_frame = canvas->glyphFrames().first().toMap();
    const qreal latin_baseline_after_fallback =
        mixed_glyph_frame.value(QStringLiteral("placements"))
            .toList()
            .first()
            .toMap()
            .value(QStringLiteral("baselineYPt"))
            .toReal();
    QCOMPARE(latin_baseline_after_fallback, latin_baseline_before_fallback);
    QTRY_VERIFY_WITH_TIMEOUT(rendered_glyph_has_ink(1), 3000);

    QTest::keyClick(&window, Qt::Key_Return);
    QTRY_COMPARE_WITH_TIMEOUT(editing_command_spy.size(), 1, 3000);
    QCOMPARE(editing_command_spy.takeFirst().at(0).toString(),
             QStringLiteral("text.split_paragraph"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("3"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts().size(), 2);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .at(0)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x！"));
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .at(1)
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QString());

    QTest::keyClick(&window, Qt::Key_Backspace);
    QTRY_COMPARE_WITH_TIMEOUT(editing_command_spy.size(), 1, 3000);
    QCOMPARE(editing_command_spy.takeFirst().at(0).toString(),
             QStringLiteral("text.delete_backward"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("4"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts().size(), 1);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x！"));

    QTest::keyClick(&window, Qt::Key_Backspace);
    QTRY_COMPARE_WITH_TIMEOUT(editing_command_spy.size(), 1, 3000);
    QCOMPARE(editing_command_spy.takeFirst().at(0).toString(),
             QStringLiteral("text.delete_backward"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("5"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QStringLiteral("x"));

    QTest::keyClick(&window, Qt::Key_Backspace);
    QTRY_COMPARE_WITH_TIMEOUT(editing_command_spy.size(), 1, 3000);
    QCOMPARE(editing_command_spy.takeFirst().at(0).toString(),
             QStringLiteral("text.delete_backward"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("6"), 3000);
    QCOMPARE(layout_view_model->paragraphLayouts()
                 .first()
                 .toMap()
                 .value(QStringLiteral("text"))
                 .toString(),
             QString());

    QTest::keyClick(&window, Qt::Key_Return, Qt::ShiftModifier);
    QTRY_COMPARE_WITH_TIMEOUT(editing_command_spy.size(), 1, 3000);
    QCOMPARE(editing_command_spy.takeFirst().at(0).toString(), QStringLiteral("text.soft_break"));
    QTRY_COMPARE_WITH_TIMEOUT(layout_view_model->documentGeneration(), QStringLiteral("7"), 3000);
    delete surface;
}
#endif

int main(int argc, char* argv[]) {
    QQuickWindow::setSceneGraphBackend(QStringLiteral("software"));
    const QGuiApplication app(argc, argv);
    ShellLoadTest test;
    return QTest::qExec(&test, argc, argv);
}

#include "shell_load_test.moc"
