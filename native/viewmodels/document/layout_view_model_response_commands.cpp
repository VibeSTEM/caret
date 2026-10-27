// 安装 Rust 编辑与交互命令返回的布局或交互状态。
#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#endif
#include <QByteArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QString>
#include <QVariant>
#include <QVariantMap>
#include <qcontainerfwd.h>

namespace caret {
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

QVariantMap LayoutViewModel::installInteractionResponse(const QByteArray& response_bytes) {
#ifdef CARET_ENABLE_APP
    if (response_bytes.startsWith("error:")) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"),
                 QString::fromUtf8(response_bytes.constData(), response_bytes.size())}};
    }
    const QString response = QString::fromUtf8(response_bytes.constData(), response_bytes.size());
    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(response_bytes, &parse_error);
    if (parse_error.error != QJsonParseError::NoError || !document.isObject()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.invalid_interaction")}};
    }
    if (document.object().contains(QStringLiteral("pages"))) {
        return installEditResponse(response_bytes);
    }
    if (!installInteraction(response)) {
        return {{QStringLiteral("valid"), false}, {QStringLiteral("error"), m_error}};
    }
    return {{QStringLiteral("valid"), true}};
#else
    Q_UNUSED(response_bytes)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
