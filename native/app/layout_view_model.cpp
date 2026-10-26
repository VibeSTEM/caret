// Rust 页面/段落快照的只读 Qt 属性适配。
#include "layout_view_model.hpp"

#ifdef CARET_ENABLE_APP
#include "document_runtime.hpp"
#endif

#include <QByteArray>
#include <QCoreApplication>
#include <QHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QSizeF>
#include <QThread>
#include <QVariant>
#include <algorithm>
#include <cmath>
#include <limits>
#include <optional>
#include <utility>

namespace caret {
namespace {

QString& installed_snapshot() {
    static QString value;
    return value;
}

#ifdef CARET_ENABLE_APP
std::optional<rust::Box<ffi::DocumentRuntime>>& installed_runtime() {
    static std::optional<rust::Box<ffi::DocumentRuntime>> value;
    return value;
}
#endif

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

bool is_utf8_boundary(const QByteArray& text, qsizetype offset) {
    if (offset < 0 || offset > text.size()) {
        return false;
    }
    if (offset == 0 || offset == text.size()) {
        return true;
    }
    return (static_cast<unsigned char>(text.at(offset)) & 0xC0U) != 0x80U;
}

bool parse_layout_snapshot(const QString& snapshot_json, QVariantList& page_sizes,
                           QVariantList& page_layouts, QVariantMap& page_margins,
                           QVariantList& paragraph_layouts, qulonglong& document_generation,
                           qulonglong& font_generation, QString& error) {
    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(snapshot_json.toUtf8(), &parse_error);
    if (parse_error.error != QJsonParseError::NoError || !document.isObject()) {
        error = QStringLiteral("layout.invalid_snapshot");
        return false;
    }
    const QJsonObject root = document.object();
    qulonglong parsed_document_generation = 0;
    qulonglong parsed_font_generation = 0;
    if (!nonnegative_generation(root.value(QStringLiteral("documentGeneration")),
                                parsed_document_generation) ||
        !nonnegative_generation(root.value(QStringLiteral("fontGeneration")),
                                parsed_font_generation)) {
        error = QStringLiteral("layout.invalid_generation");
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
    QVariantList parsed_paragraphs;
    const QJsonArray paragraphs = paragraphs_value.toArray();
    parsed_paragraphs.reserve(paragraphs.size());
    QHash<QString, qsizetype> paragraph_line_counts;
    QHash<QString, bool> paragraph_ids;
    for (const QJsonValue& paragraph_value : paragraphs) {
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
        QVariantList parsed_lines;
        parsed_lines.reserve(lines.size());
        qsizetype consumed_cursor = 0;
        double expected_top_pt = 0.0;
        for (const QJsonValue& line_value : lines) {
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
            parsed_lines.push_back(line.toVariantMap());
        }
        if (consumed_cursor != utf8_size) {
            error = QStringLiteral("layout.invalid_line_ranges");
            return false;
        }
        paragraph_line_counts.insert(paragraph_id, lines.size());
        QVariantMap parsed_paragraph = paragraph.toVariantMap();
        parsed_paragraph.insert(QStringLiteral("lines"), parsed_lines);
        parsed_paragraphs.push_back(parsed_paragraph);
    }

    const QJsonValue pages_value = root.value(QStringLiteral("pages"));
    if (!pages_value.isArray()) {
        error = QStringLiteral("layout.invalid_pages");
        return false;
    }
    QVariantList parsed_sizes;
    QVariantList parsed_page_layouts;
    const QJsonArray pages = pages_value.toArray();
    parsed_sizes.reserve(pages.size());
    parsed_page_layouts.reserve(pages.size());
    for (const QJsonValue& page_value : pages) {
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
        for (const QJsonValue& fragment_value : fragments_value.toArray()) {
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
        parsed_page_layouts.push_back(page.toVariantMap());
    }
    page_sizes = std::move(parsed_sizes);
    page_layouts = std::move(parsed_page_layouts);
    page_margins = std::move(parsed_margins);
    paragraph_layouts = std::move(parsed_paragraphs);
    document_generation = parsed_document_generation;
    font_generation = parsed_font_generation;
    error.clear();
    return true;
}

} // namespace

bool install_layout_snapshot(QString snapshot_json) {
    QVariantList ignored_pages;
    QVariantList ignored_page_layouts;
    QVariantMap ignored_margins;
    QVariantList ignored_paragraphs;
    qulonglong ignored_document_generation = 0;
    qulonglong ignored_font_generation = 0;
    QString error;
    if (!parse_layout_snapshot(snapshot_json, ignored_pages, ignored_page_layouts, ignored_margins,
                               ignored_paragraphs, ignored_document_generation,
                               ignored_font_generation, error)) {
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
    (void)parse_layout_snapshot(installed_snapshot(), m_page_sizes_pt, m_page_layouts,
                                m_page_margins_pt, m_paragraph_layouts, m_document_generation,
                                m_font_generation, m_error);
}

#ifdef CARET_ENABLE_APP
bool install_document_runtime(rust::Box<ffi::DocumentRuntime> runtime) {
    installed_runtime() = std::move(runtime);
    return true;
}
#endif

QVariantMap LayoutViewModel::hitTestDocument(int page_index, qreal x_pt, qreal y_pt) {
#ifdef CARET_ENABLE_APP
    const QCoreApplication* application = QCoreApplication::instance();
    if (application == nullptr || QThread::currentThread() != application->thread()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.hit_test_wrong_thread")}};
    }
    auto& runtime = installed_runtime();
    if (!runtime.has_value()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }

    const rust::String response = ffi::document_hit_test_json(
        **runtime, page_index, static_cast<double>(x_pt), static_cast<double>(y_pt));
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
    if (parse_error.error != QJsonParseError::NoError) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.invalid_hit_result")}};
    }
    if (!document.isObject()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.invalid_hit_result")}};
    }
    QVariantMap result = document.object().toVariantMap();
    result.insert(QStringLiteral("valid"), true);
    return result;
#else
    Q_UNUSED(page_index)
    Q_UNUSED(x_pt)
    Q_UNUSED(y_pt)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
