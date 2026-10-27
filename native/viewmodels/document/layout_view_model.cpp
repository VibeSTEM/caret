// 安装 Rust 布局快照，并发布只读页面与插入点属性。
#include "layout_view_model.hpp"

#include "layout_snapshot_codec.hpp"
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QObject>
#include <qtmetamacros.h>
#include <utility>

namespace caret {
namespace {
QString& installed_snapshot() {
    static QString value;
    return value;
}
} // namespace

bool install_layout_snapshot(QString snapshot_json) {
    layout_snapshot::ParsedLayoutSnapshot parsed_snapshot;
    QString error;
    if (!layout_snapshot::parse(snapshot_json, parsed_snapshot, error)) {
        return false;
    }
    installed_snapshot() = std::move(snapshot_json);
    return true;
}

LayoutViewModel::LayoutViewModel(QObject* parent) : QObject(parent) {
    if (installed_snapshot().isEmpty()) {
        m_error = QStringLiteral("layout.snapshot_unavailable");
        return;
    }
    layout_snapshot::ParsedLayoutSnapshot parsed_snapshot;
    if (layout_snapshot::parse(installed_snapshot(), parsed_snapshot, m_error)) {
        m_page_sizes_pt = std::move(parsed_snapshot.page_sizes);
        m_page_layouts = std::move(parsed_snapshot.page_layouts);
        m_page_margins_pt = std::move(parsed_snapshot.page_margins);
        m_paragraph_layouts = std::move(parsed_snapshot.paragraph_layouts);
        m_selection_rects = std::move(parsed_snapshot.selection_rects);
        m_input_method_context = std::move(parsed_snapshot.input_method_context);
        m_document_generation = parsed_snapshot.document_generation;
        m_font_generation = parsed_snapshot.font_generation;
    }
}

bool LayoutViewModel::installSnapshot(const QString& snapshot_json) {
    layout_snapshot::ParsedLayoutSnapshot parsed_snapshot;
    QString error;
    if (!layout_snapshot::parse(snapshot_json, parsed_snapshot, error)) {
        m_error = std::move(error);
        return false;
    }
    const bool interaction_changed = m_selection_rects != parsed_snapshot.selection_rects ||
                                     m_input_method_context != parsed_snapshot.input_method_context;
    m_page_sizes_pt = std::move(parsed_snapshot.page_sizes);
    m_page_layouts = std::move(parsed_snapshot.page_layouts);
    m_page_margins_pt = std::move(parsed_snapshot.page_margins);
    m_paragraph_layouts = std::move(parsed_snapshot.paragraph_layouts);
    m_selection_rects = std::move(parsed_snapshot.selection_rects);
    m_composition_rects = std::move(parsed_snapshot.composition_rects);
    m_input_method_context = std::move(parsed_snapshot.input_method_context);
    m_document_generation = parsed_snapshot.document_generation;
    m_font_generation = parsed_snapshot.font_generation;
    ++m_layout_revision;
    m_error.clear();
    installed_snapshot() = snapshot_json;
    emit layoutChanged();
    if (interaction_changed) {
        emit interactionChanged();
    }
    return true;
}

bool LayoutViewModel::installInteraction(const QString& interaction_json) {
    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(interaction_json.toUtf8(), &parse_error);
    if (parse_error.error != QJsonParseError::NoError || !document.isObject()) {
        m_error = QStringLiteral("layout.invalid_interaction");
        return false;
    }
    const QJsonObject root = document.object();
    const QString document_generation = root.value(QStringLiteral("documentGeneration")).toString();
    const QString font_generation = root.value(QStringLiteral("fontGeneration")).toString();
    bool document_generation_ok = false;
    bool font_generation_ok = false;
    const qulonglong document_generation_value =
        document_generation.toULongLong(&document_generation_ok);
    const qulonglong font_generation_value = font_generation.toULongLong(&font_generation_ok);
    const QJsonValue rectangles_value = root.value(QStringLiteral("selectionRects"));
    const QJsonValue input_context_value = root.value(QStringLiteral("inputMethodContext"));
    const QJsonValue caret_value = root.value(QStringLiteral("caretGeometry"));
    if (!document_generation_ok || !font_generation_ok || !rectangles_value.isArray() ||
        !input_context_value.isObject() || !caret_value.isObject() ||
        document_generation_value != m_document_generation ||
        font_generation_value != m_font_generation) {
        m_error = QStringLiteral("layout.stale_interaction");
        return false;
    }

    const QVariantList selection_rectangles = rectangles_value.toArray().toVariantList();
    QVariantMap input_context = input_context_value.toObject().toVariantMap();
    if (!input_context.contains(QStringLiteral("surroundingText"))) {
        if (!m_input_method_context.contains(QStringLiteral("surroundingText"))) {
            m_error = QStringLiteral("layout.interaction_context_unavailable");
            return false;
        }
        input_context.insert(QStringLiteral("surroundingText"),
                             m_input_method_context.value(QStringLiteral("surroundingText")));
    }
    if (m_selection_rects != selection_rectangles || m_input_method_context != input_context) {
        m_selection_rects = selection_rectangles;
        m_input_method_context = input_context;
        emit interactionChanged();
    }
    installCaretGeometry(caret_value.toObject().toVariantMap());
    m_error.clear();
    return true;
}

void LayoutViewModel::installCaretGeometry(const QVariantMap& geometry) {
    if (m_caret_geometry == geometry) {
        return;
    }
    m_caret_geometry = geometry;
    emit caretGeometryChanged();
}

} // namespace caret
