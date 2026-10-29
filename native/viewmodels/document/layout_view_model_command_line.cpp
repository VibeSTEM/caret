// 把命令查询与候选选择交给 Rust Session。
#include "layout_snapshot_codec.hpp"
#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "layout_view_model_support.hpp"
#endif
#include <QByteArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QString>
#include <QVariant>
#include <cstddef>
#include <qcontainerfwd.h>
#include <string>
#include <utility>

namespace caret {
QVariantMap LayoutViewModel::commandLineCommand(const QString& action, const QString& text) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("cmdline.wrong_thread"))) {
        return *error;
    }
    const QByteArray action_utf8 = action.toUtf8();
    const QByteArray text_utf8 = text.toUtf8();
    const rust::String response = ffi::command_line_command_json(
        *runtime,
        rust::String(
            std::string(action_utf8.constData(), static_cast<std::size_t>(action_utf8.size()))),
        rust::String(
            std::string(text_utf8.constData(), static_cast<std::size_t>(text_utf8.size()))));
    const QByteArray response_bytes(response.data(), static_cast<qsizetype>(response.size()));
    if (response_bytes.startsWith("error:")) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QString::fromUtf8(response_bytes.mid(6))}};
    }
    return installCommandLineResponse(response_bytes);
#else
    Q_UNUSED(action)
    Q_UNUSED(text)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::installCommandLineResponse(const QByteArray& response_bytes) {
#ifdef CARET_ENABLE_APP
    if (response_bytes.startsWith("error:")) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"),
                 QString::fromUtf8(response_bytes.constData(), response_bytes.size())}};
    }
    QJsonParseError parse_error;
    const QJsonDocument response = QJsonDocument::fromJson(response_bytes, &parse_error);
    if (parse_error.error != QJsonParseError::NoError || !response.isObject()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.invalid_command_line")}};
    }
    if (response.object().contains(QStringLiteral("pages"))) {
        return installEditResponse(response);
    }
    QVariantMap command_line;
    QString error;
    if (!layout_snapshot::parse_command_line_state(response.object(), command_line, error)) {
        return {{QStringLiteral("valid"), false}, {QStringLiteral("error"), error}};
    }
    if (m_command_line != command_line) {
        m_command_line = std::move(command_line);
        emit interactionChanged();
    }
    return {{QStringLiteral("valid"), true}};
#else
    Q_UNUSED(response_bytes)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}
} // namespace caret
