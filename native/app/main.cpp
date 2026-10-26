/// 桌面入口：加载 Caret.Shell 的文档窗口。关闭窗口后事件循环结束。
///
/// `--version` 打印 caret-ffi 的版本字符串后退出。`--quit-after-load` 在根窗口
/// 创建后走同一条 close 路径，供无交互环境确认进程会退出。其余参数必须写在 `--` 之后，
/// 交给 QGuiApplication。

#include "caret_ffi.h"
#include "icon_provider.hpp"
#include "layout_view_model.hpp"
#include "theme_catalog.hpp"
#include <QCoreApplication>
#include <QGuiApplication>
#include <QObject>
#include <QQmlApplicationEngine>
#include <QString>
#include <QWindow>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickControls2/qquickstyle.h>
#include <cstdio>
#include <exception>
#include <qnamespace.h>
#include <string_view>
#include <vector>

Q_IMPORT_QML_PLUGIN(Caret_ShellPlugin)
Q_IMPORT_QML_PLUGIN(Caret_CanvasPlugin)
Q_IMPORT_QML_PLUGIN(Caret_ViewModelsPlugin)

namespace {

constexpr int kExitUsage = 64;
constexpr int kExitUnavailable = 69;

void print_version() {
    const rust::String value = caret::ffi::version();
    (void)std::fwrite(value.data(), 1, value.size(), stdout);
    (void)std::fputc('\n', stdout);
}

} // namespace

int main(int argc, char* argv[]) try {
    bool quit_after_load = false;
    std::vector<char*> forwarded;
    forwarded.push_back(argv[0]);
    for (int index = 1; index < argc; ++index) {
        const std::string_view argument = argv[index];
        if (argument == "--") {
            for (int rest = index + 1; rest < argc; ++rest) {
                forwarded.push_back(argv[rest]);
            }
            break;
        }
        if (argument == "--version") {
            print_version();
            return 0;
        }
        if (argument == "--quit-after-load") {
            quit_after_load = true;
            continue;
        }
        std::fprintf(stderr,
                     "caret: 未知参数 '%.*s'（用法：caret [--version] [--quit-after-load] [-- <Qt "
                     "参数>]）\n",
                     static_cast<int>(argument.size()), argument.data());
        return kExitUsage;
    }
    forwarded.push_back(nullptr);
    int forwarded_argc = static_cast<int>(forwarded.size()) - 1;

    // 引用版本字符串，保证空白窗口的链接也消费 caret-ffi，而不是只在 --version 路径上用到。
    const rust::String linked_version = caret::ffi::version();
    const QGuiApplication app(forwarded_argc, forwarded.data());
    QGuiApplication::setApplicationVersion(
        QString::fromUtf8(linked_version.data(), static_cast<qsizetype>(linked_version.size())));
#ifdef Q_OS_LINUX
    QGuiApplication::setApplicationDisplayName(QString{});
#endif
    QQuickStyle::setStyle(QStringLiteral("Basic"));
    if (!caret::install_builtin_theme_catalog()) {
        return kExitUnavailable;
    }
    auto layout_runtime = caret::ffi::new_blank_document_runtime();
    const rust::String layout_snapshot = caret::ffi::layout_snapshot_json(*layout_runtime);
    const std::string_view layout_snapshot_text(layout_snapshot.data(), layout_snapshot.size());
    if (layout_snapshot_text.starts_with("error:") ||
        !caret::install_layout_snapshot(QString::fromUtf8(
            layout_snapshot.data(), static_cast<qsizetype>(layout_snapshot.size())))) {
        return kExitUnavailable;
    }

    QQmlApplicationEngine engine;
    caret::install_icon_provider(engine);
    if (quit_after_load) {
        QObject::connect(
            &engine, &QQmlApplicationEngine::objectCreated, &app,
            [](QObject* object, const QUrl&) {
                if (auto* window = qobject_cast<QWindow*>(object)) {
                    window->close();
                }
            },
            Qt::QueuedConnection);
    }
    QObject::connect(
        &engine, &QQmlApplicationEngine::objectCreationFailed, &app,
        []() { QCoreApplication::exit(kExitUnavailable); }, Qt::QueuedConnection);
    engine.loadFromModule("Caret.Shell", "App");
    if (engine.rootObjects().isEmpty()) {
        return kExitUnavailable;
    }
    return QGuiApplication::exec();
} catch (const std::exception& error) {
    std::fprintf(stderr, "caret: 启动失败：%s\n", error.what());
    return kExitUnavailable;
} catch (...) {
    std::fprintf(stderr, "caret: 启动失败：未知异常\n");
    return kExitUnavailable;
}
