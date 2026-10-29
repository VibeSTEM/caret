// 验证 Qt 文档导航命令将用户移动结果同步到可观察的插入点状态。
#include "shell_load_test.hpp"

#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#include "layout_view_model.hpp"
#endif

#include <QTest>
#include <QtCore/qcontainerfwd.h>
#include <QtCore/qglobal.h>
#include <QtCore/qstring.h>
#include <QtCore/qvariant.h>
#include <QtTest/qtestcase.h>
#include <cstddef>
#include <string_view>
#include <utility>

#ifdef CARET_ENABLE_APP
void ShellLoadTest::documentCaretNavigationUpdatesObservablePosition() {
    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(!std::string_view(snapshot.data(), snapshot.size()).starts_with("error:"));
    QVERIFY(caret::install_layout_snapshot(
        QString::fromUtf8(snapshot.data(), static_cast<qsizetype>(snapshot.size()))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));

    caret::LayoutViewModel view_model;
    const auto insert = view_model.insertText(QStringLiteral("first line"));
    QVERIFY2(insert.value(QStringLiteral("valid")).toBool(),
             qPrintable(insert.value(QStringLiteral("error")).toString()));
    const auto split = view_model.splitParagraph();
    QVERIFY2(split.value(QStringLiteral("valid")).toBool(),
             qPrintable(split.value(QStringLiteral("error")).toString()));
    const auto second_line = view_model.insertText(QStringLiteral("second line"));
    QVERIFY2(second_line.value(QStringLiteral("valid")).toBool(),
             qPrintable(second_line.value(QStringLiteral("error")).toString()));

    const QVariantList paragraphs_before_rejected_input = view_model.paragraphLayouts();
    const QVariantMap rejected_newline = view_model.insertText(QStringLiteral("\n"));
    QVERIFY(!rejected_newline.value(QStringLiteral("valid")).toBool());
    QCOMPARE(view_model.paragraphLayouts(), paragraphs_before_rejected_input);

    const QVariantMap invalid_stop{
        {QStringLiteral("position"),
         QVariantMap{{QStringLiteral("paragraphId"), QStringLiteral("missing-paragraph")},
                     {QStringLiteral("offsetUtf8"), QStringLiteral("0")},
                     {QStringLiteral("affinity"), QStringLiteral("downstream")}}},
        {QStringLiteral("visualAffinity"), QStringLiteral("downstream")},
    };
    const QVariantMap rejected_selection = view_model.setSelection(invalid_stop, invalid_stop);
    QVERIFY(!rejected_selection.value(QStringLiteral("valid")).toBool());
    QCOMPARE(view_model.paragraphLayouts(), paragraphs_before_rejected_input);

    const QVariantMap before = view_model.caretGeometry();
    const auto visual = view_model.moveVisualCaret(false, false);
    QVERIFY2(visual.value(QStringLiteral("valid")).toBool(),
             qPrintable(visual.value(QStringLiteral("error")).toString()));
    QVERIFY(!view_model.caretGeometry().isEmpty());

    const auto vertical = view_model.moveVerticalCaret(-1, true);
    QVERIFY2(vertical.value(QStringLiteral("valid")).toBool(),
             qPrintable(vertical.value(QStringLiteral("error")).toString()));
    QVERIFY(!view_model.selectionRects().isEmpty());

    const auto line_start = view_model.moveToVisualLineEdge(false, false);
    QVERIFY2(line_start.value(QStringLiteral("valid")).toBool(),
             qPrintable(line_start.value(QStringLiteral("error")).toString()));
    const auto line_end = view_model.moveToVisualLineEdge(true, true);
    QVERIFY2(line_end.value(QStringLiteral("valid")).toBool(),
             qPrintable(line_end.value(QStringLiteral("error")).toString()));
    QVERIFY(!view_model.selectionRects().isEmpty());
    QVERIFY(view_model.caretGeometry() != before);
}
#endif
