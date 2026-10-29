// 管理 Qt 翻译器生命周期，资源路径由 Rust 语言目录映射到构建产物。
#include "application_translator.hpp"

#include <QCoreApplication>
#include <QPointer>
#include <QQmlEngine>
#include <QTranslator>
#include <memory>
#include <utility>

namespace caret {
namespace {

std::unique_ptr<QTranslator>& active_translator() {
    static std::unique_ptr<QTranslator> value;
    return value;
}

QPointer<QQmlEngine>& application_qml_engine() {
    static QPointer<QQmlEngine> value;
    return value;
}

} // namespace

std::unique_ptr<QTranslator> prepare_application_translator(const QString& locale) {
    auto translator = std::make_unique<QTranslator>();
    QString resource_locale = locale;
    resource_locale.replace(QLatin1Char('-'), QLatin1Char('_'));
    if (!translator->load(QStringLiteral(":/i18n/caret_%1.qm").arg(resource_locale))) {
        return {};
    }
    return translator;
}

void set_application_qml_engine(QQmlEngine* engine) { application_qml_engine() = engine; }

bool install_application_translator(std::unique_ptr<QTranslator> translator) {
    if (!translator || QCoreApplication::instance() == nullptr) {
        return false;
    }
    auto& active = active_translator();
    QTranslator* candidate = translator.get();
    if (!QCoreApplication::installTranslator(candidate)) {
        return false;
    }
    if (active) {
        QCoreApplication::removeTranslator(active.get());
    }
    active = std::move(translator);
    if (application_qml_engine()) {
        application_qml_engine()->retranslate();
    }
    return true;
}

} // namespace caret
