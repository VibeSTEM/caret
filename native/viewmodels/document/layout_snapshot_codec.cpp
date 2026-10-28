// 校验 Rust 布局快照中的页面、文本范围、几何与代际。
#include "layout_snapshot_codec.hpp"

#include <QByteArray>
#include <QHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QSizeF>
#include <QVariant>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <limits>
#include <qcontainerfwd.h>
#include <utility>

namespace caret::layout_snapshot {
namespace {
qulonglong next_page_glyph_revision() {
    static std::atomic<qulonglong> revision{1};
    return revision.fetch_add(1, std::memory_order_relaxed);
}

bool nonnegative_index(const QJsonValue& value, qsizetype& result) {
    if (!value.isDouble()) {
        return false;
    }
    const qint64 number = value.toInteger(-1);
    if (number < 0 || number > std::numeric_limits<qsizetype>::max()) {
        return false;
    }
    result = static_cast<qsizetype>(number);
    return true;
}

bool nonnegative_generation(const QJsonValue& value, qulonglong& result) {
    if (!value.isString()) {
        return false;
    }
    bool valid = false;
    const qulonglong number = value.toString().toULongLong(&valid);
    if (!valid) {
        return false;
    }
    result = number;
    return true;
}

bool finite_number(const QJsonValue& value, double& result) {
    if (!value.isDouble()) {
        return false;
    }
    result = value.toDouble();
    return std::isfinite(result);
}

bool parse_geometry_rectangles(const QJsonValue& value, const QVariantList& page_sizes,
                               const QString& field, QVariantList& result, QString& error) {
    const QString array_error = QStringLiteral("layout.invalid_%1_rects").arg(field);
    const QString item_error = QStringLiteral("layout.invalid_%1_rect").arg(field);
    if (!value.isUndefined() && !value.isArray()) {
        error = array_error;
        return false;
    }
    const QJsonArray rectangles = value.toArray();
    result.reserve(rectangles.size());
    for (const QJsonValue rectangle_value : rectangles) {
        if (!rectangle_value.isObject()) {
            error = item_error;
            return false;
        }
        const QJsonObject rectangle = rectangle_value.toObject();
        qsizetype page_index = 0;
        double x0 = 0.0;
        double y0 = 0.0;
        double x1 = 0.0;
        double y1 = 0.0;
        if (!nonnegative_index(rectangle.value(QStringLiteral("pageIndex")), page_index) ||
            page_index >= page_sizes.size() ||
            !finite_number(rectangle.value(QStringLiteral("x0Pt")), x0) ||
            !finite_number(rectangle.value(QStringLiteral("y0Pt")), y0) ||
            !finite_number(rectangle.value(QStringLiteral("x1Pt")), x1) ||
            !finite_number(rectangle.value(QStringLiteral("y1Pt")), y1) || x0 < 0.0 || y0 < 0.0 ||
            x1 <= x0 || y1 <= y0 || x1 > page_sizes.at(page_index).toSizeF().width() ||
            y1 > page_sizes.at(page_index).toSizeF().height()) {
            error = item_error;
            return false;
        }
        result.push_back(rectangle.toVariantMap());
    }
    return true;
}

bool is_utf8_boundary(const QByteArray& text, qsizetype offset) {
    if (offset < 0 || offset > text.size()) {
        return false;
    }
    if (offset == 0 || offset == text.size()) {
        return true;
    }
    return (static_cast<unsigned char>(text.at(offset)) & 0xC0U) != 0x80U;
}

} // namespace

bool parse(const QString& snapshot_json, ParsedLayoutSnapshot& parsed_snapshot, QString& error,
           const ParsedLayoutSnapshot* previous_snapshot) {
    return parse(snapshot_json.toUtf8(), parsed_snapshot, error, previous_snapshot);
}

bool parse(const QByteArray& snapshot_json, ParsedLayoutSnapshot& parsed_snapshot, QString& error,
           const ParsedLayoutSnapshot* previous_snapshot) {
    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(snapshot_json, &parse_error);
    if (parse_error.error != QJsonParseError::NoError) {
        error = QStringLiteral("layout.invalid_snapshot");
        return false;
    }
    return parse(document, parsed_snapshot, error, previous_snapshot);
}

bool parse(const QJsonDocument& document, ParsedLayoutSnapshot& parsed_snapshot, QString& error,
           const ParsedLayoutSnapshot* previous_snapshot) {
    if (!document.isObject()) {
        error = QStringLiteral("layout.invalid_snapshot");
        return false;
    }
    const QJsonObject root = document.object();
    const QJsonValue partial_value = root.value(QStringLiteral("partial"));
    const bool is_partial = partial_value.toBool(false);
    if (!partial_value.isUndefined() && !partial_value.isBool()) {
        error = QStringLiteral("layout.invalid_partial_snapshot");
        return false;
    }
    const QJsonValue composition_preview_value = root.value(QStringLiteral("compositionPreview"));
    const bool is_composition_preview = composition_preview_value.toBool(false);
    if ((!composition_preview_value.isUndefined() && !composition_preview_value.isBool()) ||
        (is_composition_preview && !is_partial)) {
        error = QStringLiteral("layout.invalid_composition_preview");
        return false;
    }
    qulonglong parsed_document_generation = 0;
    qulonglong parsed_font_generation = 0;
    if (!nonnegative_generation(root.value(QStringLiteral("documentGeneration")),
                                parsed_document_generation) ||
        !nonnegative_generation(root.value(QStringLiteral("fontGeneration")),
                                parsed_font_generation)) {
        error = QStringLiteral("layout.invalid_generation");
        return false;
    }
    const QJsonValue session_value = root.value(QStringLiteral("sessionId"));
    const QString parsed_session_id =
        session_value.isString() ? session_value.toString() : QString();
    qulonglong base_document_generation = 0;
    if (is_partial &&
        (!nonnegative_generation(root.value(QStringLiteral("baseDocumentGeneration")),
                                 base_document_generation) ||
         previous_snapshot == nullptr ||
         base_document_generation != previous_snapshot->document_generation ||
         parsed_session_id.isEmpty() || parsed_session_id != previous_snapshot->session_id ||
         (is_composition_preview
              ? parsed_document_generation != previous_snapshot->document_generation
              : parsed_document_generation <= previous_snapshot->document_generation) ||
         parsed_font_generation != previous_snapshot->font_generation)) {
        error = QStringLiteral("layout.stale_partial_snapshot");
        return false;
    }

    const QJsonValue margins_value = root.value(QStringLiteral("marginsPt"));
    if (!margins_value.isObject()) {
        error = QStringLiteral("layout.invalid_page_margins");
        return false;
    }
    const QJsonObject margins = margins_value.toObject();
    QVariantMap parsed_margins;
    for (const QString& name : {QStringLiteral("top"), QStringLiteral("right"),
                                QStringLiteral("bottom"), QStringLiteral("left")}) {
        double value = -1.0;
        if (!finite_number(margins.value(name), value) || value < 0.0) {
            error = QStringLiteral("layout.invalid_page_margins");
            return false;
        }
        parsed_margins.insert(name, value);
    }

    const QJsonValue paragraphs_value = root.value(QStringLiteral("paragraphs"));
    if (!paragraphs_value.isArray()) {
        error = QStringLiteral("layout.invalid_paragraphs");
        return false;
    }
    const QJsonArray paragraphs = paragraphs_value.toArray();
    QJsonArray parsed_paragraphs = is_partial ? previous_snapshot->paragraph_layouts : paragraphs;
    QHash<QString, qsizetype> paragraph_line_counts;
    QHash<QString, bool> paragraph_ids;
    for (const auto& paragraph_value : paragraphs) {
        if (!paragraph_value.isObject()) {
            error = QStringLiteral("layout.invalid_paragraph");
            return false;
        }
        const QJsonObject paragraph = paragraph_value.toObject();
        const QJsonValue id_value = paragraph.value(QStringLiteral("paragraphId"));
        const QJsonValue text_value = paragraph.value(QStringLiteral("text"));
        const QJsonValue lines_value = paragraph.value(QStringLiteral("lines"));
        if (!id_value.isString() || id_value.toString().isEmpty() || !text_value.isString() ||
            !lines_value.isArray()) {
            error = QStringLiteral("layout.invalid_paragraph");
            return false;
        }
        const QString paragraph_id = id_value.toString();
        if (paragraph_ids.contains(paragraph_id)) {
            error = QStringLiteral("layout.duplicate_paragraph_id");
            return false;
        }
        paragraph_ids.insert(paragraph_id, true);
        const QByteArray utf8_text = text_value.toString().toUtf8();
        const qsizetype utf8_size = utf8_text.size();
        const QJsonArray lines = lines_value.toArray();
        if (lines.isEmpty()) {
            error = QStringLiteral("layout.invalid_paragraph");
            return false;
        }
        qsizetype consumed_cursor = 0;
        double expected_top_pt = 0.0;
        for (const auto& line_value : lines) {
            if (!line_value.isObject()) {
                error = QStringLiteral("layout.invalid_line");
                return false;
            }
            const QJsonObject line = line_value.toObject();
            qsizetype source_start = 0;
            qsizetype source_end = 0;
            qsizetype consumed_start = 0;
            qsizetype consumed_end = 0;
            double top_pt = 0.0;
            double baseline_pt = 0.0;
            double height_pt = 0.0;
            double advance_pt = 0.0;
            if (!nonnegative_index(line.value(QStringLiteral("sourceStart")), source_start) ||
                !nonnegative_index(line.value(QStringLiteral("sourceEnd")), source_end) ||
                !nonnegative_index(line.value(QStringLiteral("consumedStart")), consumed_start) ||
                !nonnegative_index(line.value(QStringLiteral("consumedEnd")), consumed_end) ||
                source_start > source_end || source_end > utf8_size ||
                consumed_start > consumed_end || consumed_end > utf8_size ||
                source_start != consumed_cursor || source_start != consumed_start ||
                source_end > consumed_end || !is_utf8_boundary(utf8_text, source_start) ||
                !is_utf8_boundary(utf8_text, source_end) ||
                !is_utf8_boundary(utf8_text, consumed_end) ||
                !finite_number(line.value(QStringLiteral("topPt")), top_pt) || top_pt < 0.0 ||
                !finite_number(line.value(QStringLiteral("baselinePt")), baseline_pt) ||
                !finite_number(line.value(QStringLiteral("heightPt")), height_pt) ||
                height_pt <= 0.0 ||
                !finite_number(line.value(QStringLiteral("advancePt")), advance_pt) ||
                advance_pt < 0.0 || !line.value(QStringLiteral("overflowsWidth")).isBool() ||
                !std::isfinite(top_pt + height_pt)) {
                error = QStringLiteral("layout.invalid_line");
                return false;
            }
            const double geometry_tolerance =
                std::max(0.0001, static_cast<double>(std::numeric_limits<float>::epsilon()) *
                                     std::max(1.0, std::abs(expected_top_pt)) * 2.0);
            if (std::abs(top_pt - expected_top_pt) > geometry_tolerance) {
                error = QStringLiteral("layout.invalid_line_geometry");
                return false;
            }
            consumed_cursor = consumed_end;
            expected_top_pt = top_pt + height_pt;
        }
        if (consumed_cursor != utf8_size) {
            error = QStringLiteral("layout.invalid_line_ranges");
            return false;
        }
        paragraph_line_counts.insert(paragraph_id, lines.size());
    }

    if (is_partial) {
        QHash<QString, qsizetype> merged_line_counts = previous_snapshot->paragraph_line_counts;
        QHash<QString, qsizetype> paragraph_indices;
        for (qsizetype index = 0; index < parsed_paragraphs.size(); ++index) {
            const QString id = parsed_paragraphs.at(index)
                                   .toObject()
                                   .value(QStringLiteral("paragraphId"))
                                   .toString();
            paragraph_indices.insert(id, index);
        }
        for (auto iterator = paragraph_line_counts.cbegin();
             iterator != paragraph_line_counts.cend(); ++iterator) {
            const auto existing = paragraph_indices.constFind(iterator.key());
            if (existing == paragraph_indices.cend() ||
                !merged_line_counts.contains(iterator.key())) {
                error = QStringLiteral("layout.unknown_partial_paragraph");
                return false;
            }
            merged_line_counts.insert(iterator.key(), iterator.value());
        }
        paragraph_line_counts = std::move(merged_line_counts);
        for (const QJsonValue& updated : paragraphs) {
            const QString id = updated.toObject().value(QStringLiteral("paragraphId")).toString();
            parsed_paragraphs.replace(paragraph_indices.value(id), updated);
        }
    }

    const QJsonValue pages_value = root.value(QStringLiteral("pages"));
    if (!pages_value.isArray()) {
        error = QStringLiteral("layout.invalid_pages");
        return false;
    }
    QVariantList parsed_sizes;
    QVariantList parsed_page_glyph_revisions;
    QJsonArray parsed_page_layouts;
    const QJsonArray pages = pages_value.toArray();
    qsizetype page_start_index = 0;
    if (is_partial) {
        if (!nonnegative_index(root.value(QStringLiteral("pageStartIndex")), page_start_index) ||
            page_start_index > previous_snapshot->page_sizes.size() ||
            previous_snapshot->page_sizes.size() != previous_snapshot->page_layouts.size() ||
            previous_snapshot->page_sizes.size() !=
                previous_snapshot->page_glyph_revisions.size() ||
            previous_snapshot->page_margins != parsed_margins) {
            error = QStringLiteral("layout.invalid_partial_pages");
            return false;
        }
        parsed_sizes.reserve(page_start_index + pages.size());
        for (qsizetype index = 0; index < page_start_index; ++index) {
            parsed_sizes.push_back(previous_snapshot->page_sizes.at(index));
            parsed_page_layouts.push_back(previous_snapshot->page_layouts.at(index));
            parsed_page_glyph_revisions.push_back(
                previous_snapshot->page_glyph_revisions.at(index));
        }
    } else {
        parsed_sizes.reserve(pages.size());
    }
    parsed_page_glyph_revisions.reserve(parsed_sizes.size() + pages.size());
    for (const auto& page_value : pages) {
        if (!page_value.isObject()) {
            error = QStringLiteral("layout.invalid_page");
            return false;
        }
        const QJsonObject page = page_value.toObject();
        double width_pt = -1.0;
        double height_pt = -1.0;
        const QJsonValue fragments_value = page.value(QStringLiteral("fragments"));
        if (!finite_number(page.value(QStringLiteral("widthPt")), width_pt) ||
            !finite_number(page.value(QStringLiteral("heightPt")), height_pt) || width_pt <= 0.0 ||
            height_pt <= 0.0 || !fragments_value.isArray()) {
            error = QStringLiteral("layout.invalid_page_geometry");
            return false;
        }
        if (parsed_margins.value(QStringLiteral("left")).toDouble() +
                    parsed_margins.value(QStringLiteral("right")).toDouble() >=
                width_pt ||
            parsed_margins.value(QStringLiteral("top")).toDouble() +
                    parsed_margins.value(QStringLiteral("bottom")).toDouble() >=
                height_pt) {
            error = QStringLiteral("layout.invalid_page_margins");
            return false;
        }
        const double content_height = height_pt -
                                      parsed_margins.value(QStringLiteral("top")).toDouble() -
                                      parsed_margins.value(QStringLiteral("bottom")).toDouble();
        const QJsonArray fragments = fragments_value.toArray();
        for (const auto& fragment_value : fragments) {
            if (!fragment_value.isObject()) {
                error = QStringLiteral("layout.invalid_fragment");
                return false;
            }
            const QJsonObject fragment = fragment_value.toObject();
            const QString paragraph_id = fragment.value(QStringLiteral("paragraphId")).toString();
            qsizetype first_line = 0;
            qsizetype line_end = 0;
            double y_offset_pt = -1.0;
            const auto paragraph_line_count = paragraph_line_counts.constFind(paragraph_id);
            if (paragraph_id.isEmpty() || paragraph_line_count == paragraph_line_counts.cend() ||
                !nonnegative_index(fragment.value(QStringLiteral("firstLine")), first_line) ||
                !nonnegative_index(fragment.value(QStringLiteral("lineEnd")), line_end) ||
                first_line >= line_end || line_end > *paragraph_line_count ||
                !finite_number(fragment.value(QStringLiteral("yOffsetPt")), y_offset_pt) ||
                y_offset_pt < 0.0 || y_offset_pt > content_height) {
                error = QStringLiteral("layout.invalid_fragment");
                return false;
            }
        }
        parsed_sizes.push_back(QVariant::fromValue(QSizeF(width_pt, height_pt)));
        parsed_page_layouts.push_back(page_value);
        parsed_page_glyph_revisions.push_back(QString::number(next_page_glyph_revision()));
    }
    QVariantList parsed_selection_rects;
    if (!parse_geometry_rectangles(root.value(QStringLiteral("selectionRects")), parsed_sizes,
                                   QStringLiteral("selection"), parsed_selection_rects, error)) {
        return false;
    }
    QVariantList parsed_composition_rects;
    if (!parse_geometry_rectangles(root.value(QStringLiteral("compositionRects")), parsed_sizes,
                                   QStringLiteral("composition"), parsed_composition_rects,
                                   error)) {
        return false;
    }
    parsed_snapshot.page_sizes = std::move(parsed_sizes);
    parsed_snapshot.page_layouts = std::move(parsed_page_layouts);
    parsed_snapshot.page_glyph_revisions = std::move(parsed_page_glyph_revisions);
    parsed_snapshot.page_margins = std::move(parsed_margins);
    parsed_snapshot.paragraph_layouts = std::move(parsed_paragraphs);
    parsed_snapshot.paragraph_line_counts = std::move(paragraph_line_counts);
    parsed_snapshot.selection_rects = std::move(parsed_selection_rects);
    parsed_snapshot.composition_rects = std::move(parsed_composition_rects);
    const QJsonValue input_context_value = root.value(QStringLiteral("inputMethodContext"));
    if (!input_context_value.isUndefined()) {
        if (!input_context_value.isObject()) {
            error = QStringLiteral("layout.invalid_input_method_context");
            return false;
        }
        const QJsonObject context = input_context_value.toObject();
        const QString surrounding_text =
            context.value(QStringLiteral("surroundingText")).toString();
        qsizetype cursor_position = 0;
        qsizetype anchor_position = 0;
        if (!context.value(QStringLiteral("surroundingText")).isString() ||
            !nonnegative_index(context.value(QStringLiteral("cursorPosition")), cursor_position) ||
            !nonnegative_index(context.value(QStringLiteral("anchorPosition")), anchor_position) ||
            cursor_position > surrounding_text.size() ||
            anchor_position > surrounding_text.size()) {
            error = QStringLiteral("layout.invalid_input_method_context");
            return false;
        }
        parsed_snapshot.input_method_context = context.toVariantMap();
    }
    parsed_snapshot.document_generation = parsed_document_generation;
    parsed_snapshot.font_generation = parsed_font_generation;
    parsed_snapshot.session_id = parsed_session_id;
    error.clear();
    return true;
}

} // namespace caret::layout_snapshot
