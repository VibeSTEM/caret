// 将编辑与命中意图交给 Rust，并安装事务返回的布局。
#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#endif

#include <QByteArray>
#include <QCoreApplication>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QMap>
#include <QString>
#include <QThread>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <cstddef>
#include <qcontainerfwd.h>
#include <string>

namespace caret {
#ifdef CARET_ENABLE_APP
namespace {
QMap<QString, QVariant> hit_result_from_json(const rust::String& response) {
    const QByteArray json(response.data(), static_cast<qsizetype>(response.size()));
    if (json.startsWith("error:")) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QString::fromUtf8(json.sliced(6))}};
    }
    if (json == "null") {
        return {{QStringLiteral("valid"), false}};
    }

    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(json, &parse_error);
    if (parse_error.error != QJsonParseError::NoError || !document.isObject()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.invalid_hit_result")}};
    }
    QMap<QString, QVariant> result = document.object().toVariantMap();
    result.insert(QStringLiteral("valid"), true);
    return result;
}
} // namespace
#endif

QVariantMap LayoutViewModel::hitTestDocument(int page_index, qreal x_pt, qreal y_pt) {
#ifdef CARET_ENABLE_APP
    const QCoreApplication* application = QCoreApplication::instance();
    if (application == nullptr || QThread::currentThread() != application->thread()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.hit_test_wrong_thread")}};
    }
    auto* runtime = document_runtime();
    if (runtime == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }

    return hit_result_from_json(ffi::document_hit_test_json(
        *runtime, page_index, static_cast<double>(x_pt), static_cast<double>(y_pt)));
#else
    Q_UNUSED(page_index)
    Q_UNUSED(x_pt)
    Q_UNUSED(y_pt)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::placeCaretAtPage(int page_index, qreal x_pt, qreal y_pt) {
#ifdef CARET_ENABLE_APP
    const QCoreApplication* application = QCoreApplication::instance();
    if (application == nullptr || QThread::currentThread() != application->thread()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.hit_test_wrong_thread")}};
    }
    auto* runtime = document_runtime();
    if (runtime == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    QVariantMap result = hit_result_from_json(ffi::place_caret_at_page_json(
        *runtime, page_index, static_cast<double>(x_pt), static_cast<double>(y_pt)));
    if (result.value(QStringLiteral("valid")).toBool()) {
        installCaretGeometry(result.value(QStringLiteral("caret")).toMap());
    }
    return result;
#else
    Q_UNUSED(page_index)
    Q_UNUSED(x_pt)
    Q_UNUSED(y_pt)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::insertText(const QString& text) {
#ifdef CARET_ENABLE_APP
    const QCoreApplication* application = QCoreApplication::instance();
    if (application == nullptr || QThread::currentThread() != application->thread()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("text.insert_wrong_thread")}};
    }
    if (text.isEmpty()) {
        return {{QStringLiteral("valid"), true}};
    }
    auto* runtime = document_runtime();
    if (runtime == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    const QByteArray utf8 = text.toUtf8();
    const rust::String input(std::string(utf8.constData(), static_cast<std::size_t>(utf8.size())));
    const rust::String response = ffi::insert_text_json(*runtime, input);
    return installEditResponse(
        QByteArray(response.data(), static_cast<qsizetype>(response.size())));
#else
    Q_UNUSED(text)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::splitParagraph() {
#ifdef CARET_ENABLE_APP
    const QCoreApplication* application = QCoreApplication::instance();
    if (application == nullptr || QThread::currentThread() != application->thread()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("text.split_paragraph_wrong_thread")}};
    }
    auto* runtime = document_runtime();
    if (runtime == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    const rust::String response = ffi::split_paragraph_json(*runtime);
    return installEditResponse(
        QByteArray(response.data(), static_cast<qsizetype>(response.size())));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::deleteBackward() {
#ifdef CARET_ENABLE_APP
    const QCoreApplication* application = QCoreApplication::instance();
    if (application == nullptr || QThread::currentThread() != application->thread()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("text.delete_wrong_thread")}};
    }
    auto* runtime = document_runtime();
    if (runtime == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    const rust::String response = ffi::delete_backward_json(*runtime);
    return installEditResponse(
        QByteArray(response.data(), static_cast<qsizetype>(response.size())));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::insertSoftBreak() {
#ifdef CARET_ENABLE_APP
    const QCoreApplication* application = QCoreApplication::instance();
    if (application == nullptr || QThread::currentThread() != application->thread()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("text.soft_break_wrong_thread")}};
    }
    auto* runtime = document_runtime();
    if (runtime == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    const rust::String response = ffi::insert_soft_break_json(*runtime);
    return installEditResponse(
        QByteArray(response.data(), static_cast<qsizetype>(response.size())));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::installEditResponse(const QByteArray& response_bytes) {
#ifdef CARET_ENABLE_APP
    auto* runtime = document_runtime();
    if (runtime == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    if (response_bytes.startsWith("error:")) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"),
                 QString::fromUtf8(response_bytes.constData(), response_bytes.size())}};
    }
    const QString snapshot = QString::fromUtf8(response_bytes.constData(), response_bytes.size());
    if (!installSnapshot(snapshot)) {
        return {{QStringLiteral("valid"), false}, {QStringLiteral("error"), m_error}};
    }
    const rust::String caret_response = ffi::current_caret_json(*runtime);
    const QByteArray caret_bytes(caret_response.data(),
                                 static_cast<qsizetype>(caret_response.size()));
    if (caret_bytes.startsWith("error:")) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"),
                 QString::fromUtf8(caret_bytes.constData(), caret_bytes.size())}};
    }
    QJsonParseError parse_error;
    const QJsonDocument caret_document = QJsonDocument::fromJson(caret_bytes, &parse_error);
    if (parse_error.error != QJsonParseError::NoError || !caret_document.isObject()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.invalid_caret_geometry")}};
    }
    installCaretGeometry(caret_document.object().toVariantMap());
    return {{QStringLiteral("valid"), true},
            {QStringLiteral("documentGeneration"), documentGeneration()}};
#else
    Q_UNUSED(response_bytes)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
