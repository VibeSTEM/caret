// 安装 Rust 布局快照，并发布只读页面与插入点属性。
#include "layout_view_model.hpp"

#include "layout_snapshot_codec.hpp"
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QObject>
#include <QString>
#include <qtmetamacros.h>
#include <utility>

namespace caret {
namespace {
layout_snapshot::ParsedLayoutSnapshot& installed_snapshot() {
    static layout_snapshot::ParsedLayoutSnapshot value;
    return value;
}

bool& has_installed_snapshot() {
    static bool value = false;
    return value;
}
} // namespace

bool install_layout_snapshot(const QString& snapshot_json) {
    layout_snapshot::ParsedLayoutSnapshot parsed_snapshot;
    QString error;
    if (!layout_snapshot::parse(snapshot_json, parsed_snapshot, error)) {
        return false;
    }
    installed_snapshot() = std::move(parsed_snapshot);
    has_installed_snapshot() = true;
    return true;
}

LayoutViewModel::LayoutViewModel(QObject* parent) : QObject(parent) {
    m_document_io_pool.setMaxThreadCount(1);
    m_auto_save_timer.setSingleShot(true);
    connect(&m_auto_save_timer, &QTimer::timeout, this, &LayoutViewModel::runAutoSaveDeadline);
    refreshDocumentFileStatus();
    if (!has_installed_snapshot()) {
        m_error = QStringLiteral("layout.snapshot_unavailable");
        return;
    }
    const auto& parsed_snapshot = installed_snapshot();
    m_page_sizes_pt = parsed_snapshot.page_sizes;
    m_page_glyph_revisions = parsed_snapshot.page_glyph_revisions;
    m_page_layouts = parsed_snapshot.page_layouts;
    m_page_margins_pt = parsed_snapshot.page_margins;
    m_selection_rects = parsed_snapshot.selection_rects;
    m_composition_rects = parsed_snapshot.composition_rects;
    m_paragraph_layouts = parsed_snapshot.paragraph_layouts;
    m_input_method_context = parsed_snapshot.input_method_context;
    m_command_line = {{QStringLiteral("active"), false},
                      {QStringLiteral("query"), QString{}},
                      {QStringLiteral("selected"), 0},
                      {QStringLiteral("candidates"), QVariantList{}}};
    m_current_structure = parsed_snapshot.current_structure;
    m_block_environment_highlight_rects = parsed_snapshot.block_environment_highlight_rects;
    m_inline_environment_highlight_rects = parsed_snapshot.inline_environment_highlight_rects;
    m_document_generation = parsed_snapshot.document_generation;
    m_font_generation = parsed_snapshot.font_generation;
}

bool LayoutViewModel::installSnapshot(const QString& snapshot_json, bool replace_document) {
    layout_snapshot::ParsedLayoutSnapshot parsed_snapshot;
    QString error;
    const auto* previous_snapshot =
        !replace_document && has_installed_snapshot() ? &installed_snapshot() : nullptr;
    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(snapshot_json.toUtf8(), &parse_error);
    if (parse_error.error != QJsonParseError::NoError ||
        !layout_snapshot::parse(document, parsed_snapshot, error, previous_snapshot)) {
        if (parse_error.error != QJsonParseError::NoError) {
            error = QStringLiteral("layout.invalid_snapshot");
        }
        m_error = std::move(error);
        if (m_error != QStringLiteral("layout.stale_partial_snapshot")) {
            clearEnvironmentHighlightRects();
        }
        return false;
    }
    return installSnapshot(std::move(parsed_snapshot));
}

bool LayoutViewModel::installSnapshot(const QJsonDocument& snapshot_document,
                                      bool replace_document) {
    layout_snapshot::ParsedLayoutSnapshot parsed_snapshot;
    QString error;
    const auto* previous_snapshot =
        !replace_document && has_installed_snapshot() ? &installed_snapshot() : nullptr;
    if (!layout_snapshot::parse(snapshot_document, parsed_snapshot, error, previous_snapshot)) {
        m_error = std::move(error);
        if (m_error != QStringLiteral("layout.stale_partial_snapshot")) {
            clearEnvironmentHighlightRects();
        }
        return false;
    }
    return installSnapshot(std::move(parsed_snapshot));
}

bool LayoutViewModel::installSnapshot(layout_snapshot::ParsedLayoutSnapshot parsed_snapshot) {
    const bool interaction_changed =
        m_selection_rects != parsed_snapshot.selection_rects ||
        m_input_method_context != parsed_snapshot.input_method_context ||
        m_command_line != parsed_snapshot.command_line ||
        m_current_structure != parsed_snapshot.current_structure ||
        m_block_environment_highlight_rects != parsed_snapshot.block_environment_highlight_rects ||
        m_inline_environment_highlight_rects != parsed_snapshot.inline_environment_highlight_rects;
    installed_snapshot() = parsed_snapshot;
    has_installed_snapshot() = true;
    m_page_sizes_pt = std::move(parsed_snapshot.page_sizes);
    m_page_glyph_revisions = std::move(parsed_snapshot.page_glyph_revisions);
    m_page_layouts = std::move(parsed_snapshot.page_layouts);
    m_page_margins_pt = std::move(parsed_snapshot.page_margins);
    m_selection_rects = std::move(parsed_snapshot.selection_rects);
    m_composition_rects = std::move(parsed_snapshot.composition_rects);
    m_paragraph_layouts = std::move(parsed_snapshot.paragraph_layouts);
    m_command_line = std::move(parsed_snapshot.command_line);
    m_current_structure = std::move(parsed_snapshot.current_structure);
    m_block_environment_highlight_rects =
        std::move(parsed_snapshot.block_environment_highlight_rects);
    m_inline_environment_highlight_rects =
        std::move(parsed_snapshot.inline_environment_highlight_rects);
    m_input_method_context = std::move(parsed_snapshot.input_method_context);
    m_document_generation = parsed_snapshot.document_generation;
    m_font_generation = parsed_snapshot.font_generation;
    ++m_layout_revision;
    m_error.clear();
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
        clearEnvironmentHighlightRects();
        return false;
    }
    return installInteraction(document.object());
}

bool LayoutViewModel::installInteraction(const QJsonObject& root) {
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
    if (!document_generation_ok || !font_generation_ok) {
        m_error = QStringLiteral("layout.stale_interaction");
        clearEnvironmentHighlightRects();
        return false;
    }
    // 丢弃迟到的旧交互结果；不能让旧 generation 清除当前有效状态。
    if (document_generation_value != m_document_generation ||
        font_generation_value != m_font_generation) {
        m_error = QStringLiteral("layout.stale_interaction");
        return false;
    }
    if (!rectangles_value.isArray() || !input_context_value.isObject() || !caret_value.isObject()) {
        m_error = QStringLiteral("layout.stale_interaction");
        clearEnvironmentHighlightRects();
        return false;
    }

    const QVariantList selection_rectangles = rectangles_value.toArray().toVariantList();
    QVariantMap input_context = input_context_value.toObject().toVariantMap();
    if (!input_context.contains(QStringLiteral("surroundingText"))) {
        if (!m_input_method_context.contains(QStringLiteral("surroundingText"))) {
            m_error = QStringLiteral("layout.interaction_context_unavailable");
            clearEnvironmentHighlightRects();
            return false;
        }
        input_context.insert(QStringLiteral("surroundingText"),
                             m_input_method_context.value(QStringLiteral("surroundingText")));
    }
    const QVariantMap command_line =
        root.value(QStringLiteral("commandLine")).toObject().toVariantMap();
    QVariantList block_environment_highlight_rects = m_block_environment_highlight_rects;
    QVariantList inline_environment_highlight_rects = m_inline_environment_highlight_rects;
    const QJsonValue block_environment_highlight_value =
        root.value(QStringLiteral("blockEnvironmentHighlightRects"));
    if (!block_environment_highlight_value.isUndefined()) {
        QVariantList parsed_block_environment_highlight_rects;
        if (!layout_snapshot::parse_block_environment_highlight_rects(
                block_environment_highlight_value, m_page_sizes_pt,
                parsed_block_environment_highlight_rects, m_error)) {
            clearEnvironmentHighlightRects();
            return false;
        }
        block_environment_highlight_rects = std::move(parsed_block_environment_highlight_rects);
    }
    const QJsonValue inline_environment_highlight_value =
        root.value(QStringLiteral("inlineEnvironmentHighlightRects"));
    if (!inline_environment_highlight_value.isUndefined()) {
        QVariantList parsed_inline_environment_highlight_rects;
        if (!layout_snapshot::parse_inline_environment_highlight_rects(
                inline_environment_highlight_value, m_page_sizes_pt,
                parsed_inline_environment_highlight_rects, m_error)) {
            clearEnvironmentHighlightRects();
            return false;
        }
        inline_environment_highlight_rects = std::move(parsed_inline_environment_highlight_rects);
    }
    QVariantList current_structure = m_current_structure;
    const QJsonValue current_structure_value = root.value(QStringLiteral("currentStructure"));
    if (!current_structure_value.isUndefined() &&
        !layout_snapshot::parse_current_structure(current_structure_value, current_structure,
                                                  m_error)) {
        clearEnvironmentHighlightRects();
        return false;
    }
    if (m_selection_rects != selection_rectangles || m_input_method_context != input_context ||
        m_command_line != command_line || m_current_structure != current_structure ||
        m_block_environment_highlight_rects != block_environment_highlight_rects ||
        m_inline_environment_highlight_rects != inline_environment_highlight_rects) {
        m_selection_rects = selection_rectangles;
        m_input_method_context = input_context;
        m_command_line = command_line;
        m_current_structure = std::move(current_structure);
        m_block_environment_highlight_rects = std::move(block_environment_highlight_rects);
        m_inline_environment_highlight_rects = std::move(inline_environment_highlight_rects);
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

void LayoutViewModel::clearEnvironmentHighlightRects() {
    if (m_block_environment_highlight_rects.isEmpty() &&
        m_inline_environment_highlight_rects.isEmpty()) {
        return;
    }
    m_block_environment_highlight_rects.clear();
    m_inline_environment_highlight_rects.clear();
    emit interactionChanged();
}

} // namespace caret
