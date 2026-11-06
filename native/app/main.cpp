// 桌面入口：加载 Caret.Shell 的文档窗口。关闭窗口后事件循环结束。
//
// `--version` 打印 caret-ffi 的版本字符串后退出。`--quit-after-load` 在根窗口
// 创建后走同一条 close 路径，供无交互环境确认进程会退出。其余参数必须写在 `--` 之后，
// 交给 QGuiApplication。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "application_translator.hpp"
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "icon_provider.hpp"
#include "layout_view_model.hpp"
#include "settings_runtime_bridge.hpp"
#include "theme_catalog.hpp"
#include <QByteArray>
#include <QCoreApplication>
#include <QDebug>
#include <QDir>
#include <QGuiApplication>
#include <QObject>
#include <QQmlApplicationEngine>
#include <QStandardPaths>
#include <QString>
#include <QUrl>
#include <QWindow>
#include <QtCore/qlogging.h>
#include <QtQml/qqmlextensionplugin.h>
#include <QtQuickControls2/qquickstyle.h>
#include <cstddef>
#include <cstdio>
#include <exception>
#include <qnamespace.h>
#include <rust/cxx.h>
#include <string>
#include <string_view>
#include <utility>
#include <vector>
#ifdef _WIN32
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <limits>
#include <windows.h>
#endif

Q_IMPORT_QML_PLUGIN(Caret_AdaptersPlugin)
Q_IMPORT_QML_PLUGIN(Caret_ShellPlugin)
Q_IMPORT_QML_PLUGIN(Caret_CanvasPlugin)
Q_IMPORT_QML_PLUGIN(Caret_ViewModelsPlugin)

namespace {

constexpr int kExitUsage = 64;
constexpr int kExitUnavailable = 69;

/// 将 UTF-8 诊断写到 stderr；Windows 控制台使用 UTF-16，重定向时保留 UTF-8。
void write_stderr_utf8(std::string_view message) {
#ifdef _WIN32
    const HANDLE output = GetStdHandle(STD_ERROR_HANDLE);
    DWORD console_mode = 0;
    if (output != nullptr && output != INVALID_HANDLE_VALUE &&
        GetConsoleMode(output, &console_mode) != 0 &&
        message.size() <= static_cast<std::size_t>((std::numeric_limits<int>::max)())) {
        const int byte_count = static_cast<int>(message.size());
        const int wide_count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, message.data(),
                                                   byte_count, nullptr, 0);
        if (wide_count > 0) {
            std::wstring wide_message(static_cast<std::size_t>(wide_count), L'\0');
            const int converted = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, message.data(),
                                                      byte_count, wide_message.data(), wide_count);
            if (converted == wide_count) {
                DWORD written = 0;
                if (WriteConsoleW(output, wide_message.data(), static_cast<DWORD>(wide_count),
                                  &written, nullptr) != 0 &&
                    written == static_cast<DWORD>(wide_count)) {
                    return;
                }
            }
        }
    }
#endif
    (void)std::fwrite(message.data(), sizeof(char), message.size(), stderr);
}

void print_version() {
    const rust::String value = caret::ffi::version();
    (void)std::fwrite(value.data(), 1, value.size(), stdout);
    (void)std::fputc('\n', stdout);
}

} // namespace

int main(int argc, char* argv[]) try {
    // 原生崩溃此前控制台零输出；先于其他逻辑安装，错误直接写 stderr。
    try {
        caret::ffi::install_crash_handler(rust::String(""));
    } catch (const rust::Error& error) {
        std::string diagnostic = "caret: 崩溃日志初始化失败：";
        diagnostic.append(error.what());
        diagnostic.push_back('\n');
        write_stderr_utf8(diagnostic);
    }

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
    QCoreApplication::setOrganizationName(QStringLiteral("VibeSTEM"));
    QCoreApplication::setApplicationName(QStringLiteral("Caret"));
#ifdef Q_OS_LINUX
    QGuiApplication::setApplicationDisplayName(QString{});
#endif
    QQuickStyle::setStyle(QStringLiteral("Basic"));
    const QString settings_directory =
        QStandardPaths::writableLocation(QStandardPaths::AppConfigLocation);
    if (settings_directory.isEmpty() || !QDir().mkpath(settings_directory)) {
        return kExitUnavailable;
    }
    const QByteArray settings_path =
        QDir(settings_directory).filePath(QStringLiteral("settings.json")).toUtf8();
    const QString local_data_directory =
        QStandardPaths::writableLocation(QStandardPaths::AppLocalDataLocation);
    if (local_data_directory.isEmpty() || !QDir().mkpath(local_data_directory)) {
        qWarning() << "恢复存储不可用；本次启动仍可编辑文档。" << local_data_directory;
    } else {
        const QByteArray local_data_path = local_data_directory.toUtf8();
        const rust::String recovery_configuration =
            caret::ffi::configure_document_recovery(rust::String(
                local_data_path.constData(), static_cast<std::size_t>(local_data_path.size())));
        if (!recovery_configuration.empty()) {
            qWarning() << "恢复存储配置失败；本次启动仍可编辑文档。"
                       << QString::fromUtf8(recovery_configuration.data(),
                                            static_cast<qsizetype>(recovery_configuration.size()));
        }
    }
    auto settings_runtime = caret::ffi::new_settings_runtime(
        rust::String(settings_path.constData(), static_cast<std::size_t>(settings_path.size())));
    const caret::ffi::SettingsSnapshot settings_snapshot =
        caret::ffi::settings_snapshot(*settings_runtime);
    const rust::String theme_catalog = caret::ffi::settings_theme_catalog(*settings_runtime);
    const QByteArray theme_catalog_bytes(theme_catalog.data(),
                                         static_cast<qsizetype>(theme_catalog.size()));
    if (!settings_snapshot.error.empty() || theme_catalog_bytes.startsWith("error:") ||
        !caret::install_theme_catalog(theme_catalog_bytes)) {
        return kExitUnavailable;
    }
    const QString initial_language =
        QString::fromUtf8(settings_snapshot.language.data(),
                          static_cast<qsizetype>(settings_snapshot.language.size()));
    if (!caret::install_application_translator(
            caret::prepare_application_translator(initial_language))) {
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
    if (!caret::install_document_runtime(std::move(layout_runtime))) {
        return kExitUnavailable;
    }
    if (!caret::install_settings_runtime(std::move(settings_runtime))) {
        return kExitUnavailable;
    }

    QQmlApplicationEngine engine;
    caret::set_application_qml_engine(&engine);
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
