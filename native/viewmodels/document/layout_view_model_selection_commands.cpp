// 把画布命中与拖选命令适配到 Rust Session。
#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "layout_view_model_support.hpp"
#endif
#include <QByteArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QMap>
#include <QString>
#include <QVariant>
#include <QVariantMap>

namespace caret {
#ifdef CARET_ENABLE_APP
namespace {
QMap<QString, QVariant> hit_result_from_json(const rust::String& response) {
    const QByteArray json = layout_view_model_support::response_bytes(response);
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
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("layout.hit_test_wrong_thread"))) {
        return *error;
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
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("layout.hit_test_wrong_thread"))) {
        return *error;
    }
    QVariantMap result = hit_result_from_json(ffi::place_caret_at_page_json(
        *runtime, page_index, static_cast<double>(x_pt), static_cast<double>(y_pt)));
    if (result.value(QStringLiteral("valid")).toBool()) {
        const rust::String snapshot_response = ffi::interaction_state_json(*runtime);
        const QByteArray snapshot_bytes =
            layout_view_model_support::response_bytes(snapshot_response);
        if (snapshot_bytes.startsWith("error:")) {
            return {{QStringLiteral("valid"), false},
                    {QStringLiteral("error"), QString::fromUtf8(snapshot_bytes.sliced(6))}};
        }
        if (!installInteraction(QString::fromUtf8(snapshot_bytes))) {
            return {{QStringLiteral("valid"), false}, {QStringLiteral("error"), m_error}};
        }
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

QVariantMap LayoutViewModel::dragHitMap(int page_index) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("selection.wrong_thread"))) {
        return *error;
    }
    return hit_result_from_json(ffi::drag_hit_map_json(*runtime, page_index));
#else
    Q_UNUSED(page_index)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::setSelection(const QVariantMap& anchor, const QVariantMap& focus) {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("selection.wrong_thread"))) {
        return *error;
    }
    const auto position_object = [](const QVariantMap& stop) {
        const QVariantMap position = stop.value(QStringLiteral("position")).toMap();
        return QJsonObject{
            {QStringLiteral("paragraphId"),
             position.value(QStringLiteral("paragraphId")).toString()},
            {QStringLiteral("offsetUtf8"), position.value(QStringLiteral("offsetUtf8")).toString()},
            {QStringLiteral("affinity"), position.value(QStringLiteral("affinity")).toString()},
            {QStringLiteral("visualAffinity"),
             stop.value(QStringLiteral("visualAffinity")).toString()}};
    };
    const QJsonObject selection{{QStringLiteral("anchor"), position_object(anchor)},
                                {QStringLiteral("focus"), position_object(focus)}};
    const QByteArray payload = QJsonDocument(selection).toJson(QJsonDocument::Compact);
    const rust::String response =
        ffi::set_selection_json(*runtime, layout_view_model_support::to_rust_string(payload));
    return installInteractionResponse(layout_view_model_support::response_bytes(response));
#else
    Q_UNUSED(anchor)
    Q_UNUSED(focus)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::clearSelection() {
#ifdef CARET_ENABLE_APP
    ffi::DocumentRuntime* runtime = nullptr;
    if (auto error = layout_view_model_support::runtime_error(
            runtime, QStringLiteral("selection.wrong_thread"))) {
        return *error;
    }
    const rust::String response = ffi::clear_selection_json(*runtime);
    return installInteractionResponse(layout_view_model_support::response_bytes(response));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
