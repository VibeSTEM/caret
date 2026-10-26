// Rust 页面/段落快照的只读 Qt 属性适配。
#include "layout_view_model.hpp"

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime.hpp"
#endif

#include <QByteArray>
#include <QCoreApplication>
#include <QHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QMap>
#include <QObject>
#include <QSizeF>
#include <QString>
#include <QThread>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <QtCore/qcontainerfwd.h>
#include <QtCore/qobjectdefs.h>
#include <QtCore/qtmetamacros.h>
#include <QtCore/qvariant.h>
#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
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

struct ParsedLayoutSnapshot {
    QVariantList page_sizes;
    QVariantList page_layouts;
    QVariantMap page_margins;
    QVariantList paragraph_layouts;
    qulonglong document_generation = 0;
    qulonglong font_generation = 0;
};

bool parse_layout_snapshot(const QString& snapshot_json, ParsedLayoutSnapshot& parsed_snapshot,
                           QString& error) {
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
        QVariantList parsed_lines;
        parsed_lines.reserve(lines.size());
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
        parsed_page_layouts.push_back(page.toVariantMap());
    }
    parsed_snapshot.page_sizes = std::move(parsed_sizes);
    parsed_snapshot.page_layouts = std::move(parsed_page_layouts);
    parsed_snapshot.page_margins = std::move(parsed_margins);
    parsed_snapshot.paragraph_layouts = std::move(parsed_paragraphs);
    parsed_snapshot.document_generation = parsed_document_generation;
    parsed_snapshot.font_generation = parsed_font_generation;
    error.clear();
    return true;
}

} // namespace

bool install_layout_snapshot(QString snapshot_json) {
    ParsedLayoutSnapshot parsed_snapshot;
    QString error;
    if (!parse_layout_snapshot(snapshot_json, parsed_snapshot, error)) {
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
    ParsedLayoutSnapshot parsed_snapshot;
    if (parse_layout_snapshot(installed_snapshot(), parsed_snapshot, m_error)) {
        m_page_sizes_pt = std::move(parsed_snapshot.page_sizes);
        m_page_layouts = std::move(parsed_snapshot.page_layouts);
        m_page_margins_pt = std::move(parsed_snapshot.page_margins);
        m_paragraph_layouts = std::move(parsed_snapshot.paragraph_layouts);
        m_document_generation = parsed_snapshot.document_generation;
        m_font_generation = parsed_snapshot.font_generation;
    }
}

bool LayoutViewModel::installSnapshot(const QString& snapshot_json) {
    ParsedLayoutSnapshot parsed_snapshot;
    QString error;
    if (!parse_layout_snapshot(snapshot_json, parsed_snapshot, error)) {
        m_error = std::move(error);
        return false;
    }
    m_page_sizes_pt = std::move(parsed_snapshot.page_sizes);
    m_page_layouts = std::move(parsed_snapshot.page_layouts);
    m_page_margins_pt = std::move(parsed_snapshot.page_margins);
    m_paragraph_layouts = std::move(parsed_snapshot.paragraph_layouts);
    m_document_generation = parsed_snapshot.document_generation;
    m_font_generation = parsed_snapshot.font_generation;
    m_error.clear();
    installed_snapshot() = snapshot_json;
    emit layoutChanged();
    return true;
}

void LayoutViewModel::installCaretGeometry(const QVariantMap& geometry) {
    if (m_caret_geometry == geometry) {
        return;
    }
    m_caret_geometry = geometry;
    emit caretGeometryChanged();
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

    return hit_result_from_json(ffi::document_hit_test_json(
        **runtime, page_index, static_cast<double>(x_pt), static_cast<double>(y_pt)));
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
    auto& runtime = installed_runtime();
    if (!runtime.has_value()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    QVariantMap result = hit_result_from_json(ffi::place_caret_at_page_json(
        **runtime, page_index, static_cast<double>(x_pt), static_cast<double>(y_pt)));
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
    auto& runtime = installed_runtime();
    if (!runtime.has_value()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    const QByteArray utf8 = text.toUtf8();
    const rust::String input(std::string(utf8.constData(), static_cast<std::size_t>(utf8.size())));
    const rust::String response = ffi::insert_text_json(**runtime, input);
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
    auto& runtime = installed_runtime();
    if (!runtime.has_value()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    const rust::String response = ffi::split_paragraph_json(**runtime);
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
    auto& runtime = installed_runtime();
    if (!runtime.has_value()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    const rust::String response = ffi::delete_backward_json(**runtime);
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
    auto& runtime = installed_runtime();
    if (!runtime.has_value()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    const rust::String response = ffi::insert_soft_break_json(**runtime);
    return installEditResponse(
        QByteArray(response.data(), static_cast<qsizetype>(response.size())));
#else
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

QVariantMap LayoutViewModel::installEditResponse(const QByteArray& response_bytes) {
#ifdef CARET_ENABLE_APP
    auto& runtime = installed_runtime();
    if (!runtime.has_value()) {
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
    const rust::String caret_response = ffi::current_caret_json(**runtime);
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

QVariantMap LayoutViewModel::rasterizePageGlyphs(int page_index, qreal px_per_pt,
                                                 qreal page_origin_device_x_px,
                                                 qreal page_origin_device_y_px, int foreground_red,
                                                 int foreground_green, int foreground_blue,
                                                 int foreground_alpha) {
#ifdef CARET_ENABLE_APP
    const QCoreApplication* application = QCoreApplication::instance();
    if (application == nullptr || QThread::currentThread() != application->thread()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.glyphs_wrong_thread")}};
    }
    if (page_index < 0 || !std::isfinite(px_per_pt) || px_per_pt <= 0.0 ||
        !std::isfinite(page_origin_device_x_px) || !std::isfinite(page_origin_device_y_px) ||
        foreground_red < 0 || foreground_red > 255 || foreground_green < 0 ||
        foreground_green > 255 || foreground_blue < 0 || foreground_blue > 255 ||
        foreground_alpha < 0 || foreground_alpha > 255) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.glyphs_invalid_request")}};
    }
    auto& runtime = installed_runtime();
    if (!runtime.has_value()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }

    const auto frame = ffi::rasterize_page_glyphs(
        **runtime, static_cast<std::uint32_t>(page_index), static_cast<double>(px_per_pt),
        static_cast<double>(page_origin_device_x_px), static_cast<double>(page_origin_device_y_px),
        static_cast<std::uint8_t>(foreground_red), static_cast<std::uint8_t>(foreground_green),
        static_cast<std::uint8_t>(foreground_blue), static_cast<std::uint8_t>(foreground_alpha));
    if (!frame.error.empty()) {
        return {
            {QStringLiteral("valid"), false},
            {QStringLiteral("error"),
             QString::fromUtf8(frame.error.data(), static_cast<qsizetype>(frame.error.size()))}};
    }

    QVariantList images;
    images.reserve(static_cast<qsizetype>(frame.images.size()));
    for (const auto& image : frame.images) {
        const auto width = static_cast<std::uint64_t>(image.width);
        const auto height = static_cast<std::uint64_t>(image.height);
        if ((width != 0 && height > std::numeric_limits<std::uint64_t>::max() / width) ||
            width * height > std::numeric_limits<std::uint64_t>::max() / 4 ||
            width * height * 4 != image.pixels.size() ||
            image.pixels.size() > static_cast<std::size_t>(std::numeric_limits<qsizetype>::max())) {
            return {{QStringLiteral("valid"), false},
                    {QStringLiteral("error"), QStringLiteral("layout.glyphs_invalid_image")}};
        }
        const QByteArray pixels(reinterpret_cast<const char*>(image.pixels.data()),
                                static_cast<qsizetype>(image.pixels.size()));
        images.push_back(QVariantMap{
            {QStringLiteral("width"), image.width},
            {QStringLiteral("height"), image.height},
            {QStringLiteral("left"), image.left},
            {QStringLiteral("top"), image.top},
            {QStringLiteral("pixelPhaseApplied"), image.pixel_phase_applied},
            {QStringLiteral("pixels"), pixels},
        });
    }

    QVariantList placements;
    placements.reserve(static_cast<qsizetype>(frame.placements.size()));
    for (const auto& placement : frame.placements) {
        if (placement.image_index >= frame.images.size() || !std::isfinite(placement.x_pt) ||
            !std::isfinite(placement.baseline_y_pt) || !std::isfinite(placement.pixel_phase_x) ||
            !std::isfinite(placement.pixel_phase_y) || placement.pixel_phase_x < 0.0F ||
            placement.pixel_phase_x >= 1.0F || placement.pixel_phase_y < 0.0F ||
            placement.pixel_phase_y >= 1.0F ||
            (placement.clip_enabled &&
             (!std::isfinite(placement.clip_y0_pt) || !std::isfinite(placement.clip_y1_pt) ||
              placement.clip_y0_pt < 0.0 || placement.clip_y1_pt <= placement.clip_y0_pt))) {
            return {{QStringLiteral("valid"), false},
                    {QStringLiteral("error"), QStringLiteral("layout.glyphs_invalid_placement")}};
        }
        placements.push_back(QVariantMap{
            {QStringLiteral("imageIndex"), placement.image_index},
            {QStringLiteral("xPt"), placement.x_pt},
            {QStringLiteral("baselineYPt"), placement.baseline_y_pt},
            {QStringLiteral("pixelPhaseX"), placement.pixel_phase_x},
            {QStringLiteral("pixelPhaseY"), placement.pixel_phase_y},
            {QStringLiteral("pixelPhaseApplied"),
             frame.images[placement.image_index].pixel_phase_applied},
            {QStringLiteral("clipEnabled"), placement.clip_enabled},
            {QStringLiteral("clipY0Pt"), placement.clip_y0_pt},
            {QStringLiteral("clipY1Pt"), placement.clip_y1_pt},
        });
    }
    return {{QStringLiteral("valid"), true},
            {QStringLiteral("documentGeneration"), QString::number(frame.document_generation)},
            {QStringLiteral("fontGeneration"), QString::number(frame.font_generation)},
            {QStringLiteral("rasterPxPerPt"), px_per_pt},
            {QStringLiteral("pageIndex"), frame.page_index},
            {QStringLiteral("images"), images},
            {QStringLiteral("placements"), placements}};
#else
    Q_UNUSED(page_index)
    Q_UNUSED(px_per_pt)
    Q_UNUSED(page_origin_device_x_px)
    Q_UNUSED(page_origin_device_y_px)
    Q_UNUSED(foreground_red)
    Q_UNUSED(foreground_green)
    Q_UNUSED(foreground_blue)
    Q_UNUSED(foreground_alpha)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
