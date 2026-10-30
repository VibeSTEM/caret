// 验证无效布局快照被拒绝且不会替换当前页面。
#include "shell_load_test.hpp"

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "layout_snapshot_codec.hpp"
#include "layout_view_model.hpp"
#endif

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTest>
#include <QtCore/qcontainerfwd.h>
#include <QtCore/qglobal.h>
#include <QtCore/qstring.h>
#include <QtCore/qvariant.h>
#include <QtTest/qtestcase.h>
#include <string_view>
#include <utility>

#ifdef CARET_ENABLE_APP
void ShellLoadTest::invalidLayoutSnapshotsAreRejectedWithoutReplacingPublishedPages() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String empty_snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(empty_snapshot.data(), empty_snapshot.size()).starts_with("error:"));
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(empty_snapshot.data(), static_cast<qsizetype>(empty_snapshot.size()))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));

    caret::LayoutViewModel view_model;
    const QVariantMap inserted = view_model.insertText(QStringLiteral("snapshot validation"));
    QVERIFY2(inserted.value(QStringLiteral("valid")).toBool(),
             qPrintable(inserted.value(QStringLiteral("error")).toString()));
    const QVariantMap multibyte_text = view_model.insertText(QStringLiteral("中"));
    QVERIFY2(multibyte_text.value(QStringLiteral("valid")).toBool(),
             qPrintable(multibyte_text.value(QStringLiteral("error")).toString()));
    const rust::String current_snapshot =
        caret::ffi::layout_snapshot_json(*caret::document_runtime());
    const QJsonObject root =
        QJsonDocument::fromJson(
            QByteArray(current_snapshot.data(), static_cast<qsizetype>(current_snapshot.size())))
            .object();
    QVERIFY(!root.isEmpty());
    const QVariantList published_pages = view_model.pageLayouts();
    QVERIFY(!published_pages.isEmpty());

    const auto reject = [](const QJsonObject& invalid) {
        return !caret::install_layout_snapshot(
            QString::fromUtf8(QJsonDocument(invalid).toJson(QJsonDocument::Compact)));
    };
    QVERIFY(!caret::install_layout_snapshot(QStringLiteral("{invalid json")));
    QVERIFY(!caret::install_layout_snapshot(QStringLiteral("[]")));

    QVariantList structures;
    QString structure_error;
    const auto parse_structure = [&](const QJsonObject& entry) {
        return caret::layout_snapshot::parse_current_structure(QJsonArray{entry}, structures,
                                                               structure_error);
    };
    QJsonObject heading{{QStringLiteral("id"), QStringLiteral("heading-id")},
                        {QStringLiteral("kind"), QStringLiteral("heading")},
                        {QStringLiteral("level"), 2}};
    QVERIFY(parse_structure(heading));
    QCOMPARE(structures.first().toMap().value(QStringLiteral("level")).toInt(), 2);
    heading.remove(QStringLiteral("level"));
    QVERIFY(!parse_structure(heading));
    heading.insert(QStringLiteral("level"), 10);
    QVERIFY(!parse_structure(heading));
    heading.insert(QStringLiteral("level"), 1.5);
    QVERIFY(!parse_structure(heading));
    heading.insert(QStringLiteral("kind"), QStringLiteral("section"));
    QVERIFY(!parse_structure(heading));
    heading.remove(QStringLiteral("level"));
    QVERIFY(parse_structure(heading));
    heading.insert(QStringLiteral("label"), QStringLiteral("Section"));
    QVERIFY(!parse_structure(heading));

    QVariantMap parsed_command_line;
    QString command_line_error;
    const QJsonObject unicode_command_line{{QStringLiteral("active"), true},
                                           {QStringLiteral("mode"), QStringLiteral("fontFamily")},
                                           {QStringLiteral("prefix"), QStringLiteral("\\font ")},
                                           {QStringLiteral("query"), QStringLiteral("字")},
                                           {QStringLiteral("cursor"), 3},
                                           {QStringLiteral("cursorUtf16"), 7},
                                           {QStringLiteral("selected"), 0},
                                           {QStringLiteral("candidates"), QJsonArray{}}};
    QVERIFY(caret::layout_snapshot::parse_command_line_state(
        unicode_command_line, parsed_command_line, command_line_error));
    QJsonObject invalid_command_prefix = unicode_command_line;
    invalid_command_prefix.insert(QStringLiteral("prefix"), QStringLiteral("\\"));
    QVERIFY(!caret::layout_snapshot::parse_command_line_state(
        invalid_command_prefix, parsed_command_line, command_line_error));
    QJsonObject invalid_command_cursor = unicode_command_line;
    invalid_command_cursor.insert(QStringLiteral("cursorUtf16"), 6);
    QVERIFY(!caret::layout_snapshot::parse_command_line_state(
        invalid_command_cursor, parsed_command_line, command_line_error));

    QJsonObject invalid_partial = root;
    invalid_partial.insert(QStringLiteral("partial"), QStringLiteral("yes"));
    QVERIFY(reject(invalid_partial));

    QJsonObject invalid_preview = root;
    invalid_preview.insert(QStringLiteral("compositionPreview"), true);
    QVERIFY(reject(invalid_preview));

    QJsonObject invalid_generation = root;
    invalid_generation.insert(QStringLiteral("documentGeneration"), 3);
    QVERIFY(reject(invalid_generation));
    QJsonObject invalid_font_generation = root;
    invalid_font_generation.insert(QStringLiteral("fontGeneration"), QStringLiteral("invalid"));
    QVERIFY(reject(invalid_font_generation));

    QJsonObject invalid_margins = root;
    QJsonObject margins = root.value(QStringLiteral("marginsPt")).toObject();
    margins.insert(QStringLiteral("top"), -1.0);
    invalid_margins.insert(QStringLiteral("marginsPt"), margins);
    QVERIFY(reject(invalid_margins));

    QJsonObject invalid_paragraph = root;
    QJsonArray paragraphs = root.value(QStringLiteral("paragraphs")).toArray();
    QJsonObject paragraph = paragraphs.first().toObject();
    paragraph.insert(QStringLiteral("lines"), QJsonArray{});
    paragraphs.replace(0, paragraph);
    invalid_paragraph.insert(QStringLiteral("paragraphs"), paragraphs);
    QVERIFY(reject(invalid_paragraph));

    QJsonObject invalid_paragraph_item = root;
    invalid_paragraph_item.insert(QStringLiteral("paragraphs"), QJsonArray{QStringLiteral("bad")});
    QVERIFY(reject(invalid_paragraph_item));
    QJsonObject invalid_paragraph_list = root;
    invalid_paragraph_list.insert(QStringLiteral("paragraphs"), QStringLiteral("bad"));
    QVERIFY(reject(invalid_paragraph_list));

    QJsonObject duplicate_paragraph = root;
    const QJsonObject valid_paragraph =
        root.value(QStringLiteral("paragraphs")).toArray().first().toObject();
    duplicate_paragraph.insert(QStringLiteral("paragraphs"),
                               QJsonArray{valid_paragraph, valid_paragraph});
    QVERIFY(reject(duplicate_paragraph));

    QJsonObject invalid_line = root;
    paragraphs = root.value(QStringLiteral("paragraphs")).toArray();
    paragraph = paragraphs.first().toObject();
    paragraph.insert(QStringLiteral("lines"), QJsonArray{QStringLiteral("bad")});
    paragraphs.replace(0, paragraph);
    invalid_line.insert(QStringLiteral("paragraphs"), paragraphs);
    QVERIFY(reject(invalid_line));

    QJsonObject invalid_line_ranges = root;
    paragraphs = root.value(QStringLiteral("paragraphs")).toArray();
    paragraph = paragraphs.first().toObject();
    QJsonArray lines = paragraph.value(QStringLiteral("lines")).toArray();
    QJsonObject line = lines.first().toObject();
    line.insert(QStringLiteral("consumedEnd"), 1);
    lines.replace(0, line);
    paragraph.insert(QStringLiteral("lines"), lines);
    paragraphs.replace(0, paragraph);
    invalid_line_ranges.insert(QStringLiteral("paragraphs"), paragraphs);
    QVERIFY(reject(invalid_line_ranges));

    QJsonObject invalid_utf8_boundary = root;
    paragraphs = root.value(QStringLiteral("paragraphs")).toArray();
    paragraph = paragraphs.first().toObject();
    lines = paragraph.value(QStringLiteral("lines")).toArray();
    line = lines.first().toObject();
    const qsizetype utf8_size = paragraph.value(QStringLiteral("text")).toString().toUtf8().size();
    line.insert(QStringLiteral("sourceEnd"), static_cast<qint64>(utf8_size - 1));
    lines.replace(0, line);
    paragraph.insert(QStringLiteral("lines"), lines);
    paragraphs.replace(0, paragraph);
    invalid_utf8_boundary.insert(QStringLiteral("paragraphs"), paragraphs);
    QVERIFY(reject(invalid_utf8_boundary));

    QJsonObject invalid_line_geometry = root;
    paragraphs = root.value(QStringLiteral("paragraphs")).toArray();
    paragraph = paragraphs.first().toObject();
    lines = paragraph.value(QStringLiteral("lines")).toArray();
    line = lines.first().toObject();
    line.insert(QStringLiteral("topPt"), 1.0);
    lines.replace(0, line);
    paragraph.insert(QStringLiteral("lines"), lines);
    paragraphs.replace(0, paragraph);
    invalid_line_geometry.insert(QStringLiteral("paragraphs"), paragraphs);
    QVERIFY(reject(invalid_line_geometry));

    QJsonObject invalid_page = root;
    QJsonArray pages = root.value(QStringLiteral("pages")).toArray();
    QJsonObject page = pages.first().toObject();
    page.insert(QStringLiteral("widthPt"), 0.0);
    pages.replace(0, page);
    invalid_page.insert(QStringLiteral("pages"), pages);
    QVERIFY(reject(invalid_page));

    QJsonObject invalid_pages = root;
    invalid_pages.insert(QStringLiteral("pages"), QStringLiteral("bad"));
    QVERIFY(reject(invalid_pages));

    QJsonObject invalid_page_item = root;
    invalid_page_item.insert(QStringLiteral("pages"), QJsonArray{QStringLiteral("bad")});
    QVERIFY(reject(invalid_page_item));

    QJsonObject invalid_fragment = root;
    pages = root.value(QStringLiteral("pages")).toArray();
    page = pages.first().toObject();
    page.insert(QStringLiteral("fragments"), QJsonArray{QJsonObject{}});
    pages.replace(0, page);
    invalid_fragment.insert(QStringLiteral("pages"), pages);
    QVERIFY(reject(invalid_fragment));

    QJsonObject invalid_content_margins = root;
    pages = root.value(QStringLiteral("pages")).toArray();
    page = pages.first().toObject();
    page.insert(QStringLiteral("widthPt"), 100.0);
    pages.replace(0, page);
    invalid_content_margins.insert(QStringLiteral("pages"), pages);
    QVERIFY(reject(invalid_content_margins));

    QJsonObject invalid_selection = root;
    invalid_selection.insert(QStringLiteral("selectionRects"),
                             QJsonArray{QJsonObject{{QStringLiteral("pageIndex"), 99}}});
    QVERIFY(reject(invalid_selection));
    QJsonObject invalid_selection_list = root;
    invalid_selection_list.insert(QStringLiteral("selectionRects"), QStringLiteral("bad"));
    QVERIFY(reject(invalid_selection_list));
    QJsonObject invalid_composition_rectangles = root;
    invalid_composition_rectangles.insert(QStringLiteral("compositionRects"),
                                          QJsonArray{QStringLiteral("bad")});
    QVERIFY(reject(invalid_composition_rectangles));

    QJsonObject invalid_input_context = root;
    invalid_input_context.insert(
        QStringLiteral("inputMethodContext"),
        QJsonObject{{QStringLiteral("surroundingText"), QStringLiteral("a")},
                    {QStringLiteral("cursorPosition"), 2},
                    {QStringLiteral("anchorPosition"), 0}});
    QVERIFY(reject(invalid_input_context));
    const caret::LayoutViewModel after_rejections;
    QCOMPARE(after_rejections.pageLayouts(), published_pages);
    QCOMPARE(after_rejections.paragraphLayouts(), view_model.paragraphLayouts());
}
#endif
