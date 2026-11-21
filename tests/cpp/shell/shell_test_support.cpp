// Shell 测试公共基类、主题投影与文档窗口夹具的实现。
#include "shell_test_support.hpp"

#include <QtCore/qcontainerfwd.h>
#include <QtCore/qfile.h>
#include <QtCore/qiodevice.h>
#include <QtCore/qnamespace.h>
#include <QtCore/qobject.h>
#include <QtCore/qregularexpression.h>
#include <QtCore/qstring.h>
#include <QtCore/qtestsupport_core.h>
#include <QtGui/qcolor.h>
#include <QtGui/qtestsupport_gui.h>
#include <QtQuick/qquickwindow.h>
#include <QtQuickControls2/qquickstyle.h>
#include <QtTest/qsignalspy.h>
#include <QtTest/qtest.h>
#include <memory>

#ifdef CARET_ENABLE_APP
#include "application_translator.hpp"
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "icon_provider.hpp"
#include "layout_view_model.hpp"
#include "settings_runtime_bridge.hpp"
#include "theme_catalog.hpp"
#include <QQmlApplicationEngine>
#include <cstddef>
#include <string_view>
#include <utility>
#endif

namespace caret::shell_test {

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
    tokens.insert(QStringLiteral("colorPanel"), background);
    tokens.insert(QStringLiteral("colorPanelLine"), QColor(QStringLiteral("#c9c9c9")));
    tokens.insert(QStringLiteral("colorTransparent"), QColor(Qt::transparent));
    tokens.insert(QStringLiteral("colorCaret"), QColor(QStringLiteral("#1c1c1c")));
    tokens.insert(QStringLiteral("caretWidthCentipixels"), 100);
    tokens.insert(QStringLiteral("fontBody"), body);
    return tokens;
}

} // namespace caret::shell_test

void ShellTestBase::initTestCase() {
    if (QQuickStyle::name() != QStringLiteral("Basic"))
        QQuickStyle::setStyle(QStringLiteral("Basic"));
}

#ifdef CARET_ENABLE_APP
bool ShellTestBase::configureDocumentRecovery(const QString& recovery_root) {
    const QByteArray root = recovery_root.toUtf8();
    return caret::ffi::configure_document_recovery(
               rust::String(root.constData(), static_cast<std::size_t>(root.size())))
        .empty();
}

bool ShellTestBase::installBlankDocumentRuntime() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*runtime);
    if (std::string_view(snapshot.data(), snapshot.size()).starts_with("error:")) {
        return false;
    }
    if (!caret::install_layout_snapshot(
            QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size())))) {
        return false;
    }
    return caret::install_document_runtime(std::move(runtime));
}

bool AppFixture::initialize(const QString& recovery_root, QString* error) {
    const QByteArray settings_path = directory.filePath(QStringLiteral("settings.json")).toUtf8();
    auto settings = caret::ffi::new_settings_runtime(
        rust::String(settings_path.constData(), static_cast<std::size_t>(settings_path.size())));
    if (!caret::install_settings_runtime(std::move(settings))) {
        *error = QStringLiteral("settings runtime install failed");
        return false;
    }
    const rust::String theme_catalog =
        caret::ffi::settings_theme_catalog(*caret::settings_runtime());
    const QByteArray theme_bytes(theme_catalog.data(),
                                 static_cast<qsizetype>(theme_catalog.size()));
    if (!caret::install_theme_catalog(theme_bytes) ||
        !caret::install_application_translator(
            caret::prepare_application_translator(QStringLiteral("en")))) {
        *error = QStringLiteral("theme or translator setup failed");
        return false;
    }
    const QByteArray root = recovery_root.toUtf8();
    if (!caret::ffi::configure_document_recovery(
             rust::String(root.constData(), static_cast<std::size_t>(root.size())))
             .empty()) {
        *error = QStringLiteral("recovery storage setup failed");
        return false;
    }
    auto document = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*document);
    if (std::string_view(snapshot.data(), snapshot.size()).starts_with("error:") ||
        !caret::install_layout_snapshot(
            QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size()))) ||
        !caret::install_document_runtime(std::move(document))) {
        *error = QStringLiteral("document runtime setup failed");
        return false;
    }

    engine = std::make_unique<QQmlApplicationEngine>();
    caret::set_application_qml_engine(engine.get());
    caret::install_icon_provider(*engine);
    engine->addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    engine->loadFromModule("Caret.Shell", "App");
    if (engine->rootObjects().size() != 1) {
        *error = engine->rootObjects().isEmpty() ? QStringLiteral("App.qml did not load")
                                                 : QStringLiteral("unexpected app window count");
        return false;
    }
    window = qobject_cast<QQuickWindow*>(engine->rootObjects().first());
    if (window == nullptr) {
        *error = QStringLiteral("App.qml root is not a window");
        return false;
    }
    view_model =
        engine->singletonInstance<caret::LayoutViewModel*>("Caret.ViewModels", "LayoutViewModel");
    if (view_model == nullptr) {
        *error = QStringLiteral("LayoutViewModel is unavailable");
        return false;
    }
    if (!QTest::qWaitForWindowExposed(window)) {
        *error = QStringLiteral("App window was not exposed");
        return false;
    }
    return true;
}

bool AppFixture::saveTo(const QString& path) {
    QSignalSpy operations(view_model, &caret::LayoutViewModel::documentFileOperationFinished);
    const auto request = view_model->saveDocument(path);
    if (!request.value(QStringLiteral("valid")).toBool())
        return false;
    if (operations.size() != 0 || !operations.wait(10000))
        return false;
    return operations.takeFirst().at(0).toBool();
}

bool AppFixture::insert(const QString& text) {
    return view_model->insertText(text).value(QStringLiteral("valid")).toBool();
}

QObject* AppFixture::child(const QString& name) const { return window->findChild<QObject*>(name); }

bool AppFixture::click(const QString& name) const {
    QObject* button = child(name);
    return button != nullptr && QMetaObject::invokeMethod(button, "clicked", Qt::DirectConnection);
}

bool waitForStartupScan(const AppFixture& fixture) {
    for (int elapsed = 0; elapsed < 10000; elapsed += 10) {
        if (fixture.window != nullptr && fixture.view_model != nullptr &&
            !fixture.window->property("pendingRecoveryScan").toBool() &&
            !fixture.view_model->documentFileBusy()) {
            return true;
        }
        QTest::qWait(10);
    }
    return false;
}
#endif
