// Qt/QML 画布几何、输入和缩放行为的集成测试。
#include "canvas_policy.hpp"
#include "document_canvas.hpp"
#include "document_canvas/scene_graph_geometry.hpp"
#include "glyph_frame_adapter.hpp"
#include "icon_provider.hpp"
#include "layout_view_model.hpp"
#include "shell_load_test.hpp"
#include "theme_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#endif
#include <QByteArray>
#include <QChar>
#include <QClipboard>
#include <QColor>
#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QHash>
#include <QImage>
#include <QInputMethodEvent>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QKeyEvent>
#include <QList>
#include <QMouseEvent>
#include <QObject>
#include <QPointF>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QQuickImageProvider>
#include <QQuickItem>
#include <QQuickWindow>
#include <QRect>
#include <QRectF>
#include <QRegularExpression>
#include <QScopeGuard>
#include <QSignalSpy>
#include <QSize>
#include <QString>
#include <QStyleHints>
#include <QTemporaryDir>
#include <QVariantList>
#include <QVariantMap>
#include <QWindow>
#include <Qt>
#include <QtCore/qglobal.h>
#include <QtCore/qobject.h>
#include <QtCore/qobjectdefs.h>
#include <QtCore/qvariant.h>
#include <QtGui/qrgb.h>
#include <QtMath>
#include <QtQml/qqmlextensionplugin.h>
#include <QtTest/QTest>
#include <QtTest/qtest.h>
#include <QtTest/qtestcase.h>
#include <algorithm>
#include <cstdint>
#include <iterator>
#include <limits>
#include <qcontainerfwd.h>
#include <qtestcase.h>
#include <qtestsupport_core.h>
#include <string_view>
#include <utility>

void ShellLoadTest::preeditCancellationDoesNotReenterTheInputMethodHandler() {
    caret::DocumentCanvas canvas;
    int event_count = 0;
    QObject::connect(&canvas, &caret::DocumentCanvas::inputMethodEventRequested, &canvas,
                     [&canvas, &event_count](const QString&, const QString&, int, int, int) {
                         ++event_count;
                         canvas.cancelPreedit();
                     });

    canvas.cancelPreedit();

    QCOMPARE(event_count, 1);
}

void ShellLoadTest::mathSourceDisablesInputMethodAndCancelsPreedit() {
    caret::DocumentCanvas canvas;
    int event_count = 0;
    QObject::connect(
        &canvas, &caret::DocumentCanvas::inputMethodEventRequested, &canvas,
        [&event_count](const QString&, const QString&, int, int, int) { ++event_count; });
    canvas.setInputMethodContext({{QStringLiteral("enabled"), true}});
    QInputMethodEvent preedit(QStringLiteral("ni"), {});
    QCoreApplication::sendEvent(&canvas, &preedit);
    QCOMPARE(canvas.preeditText(), QStringLiteral("ni"));
    QCOMPARE(event_count, 1);

    canvas.setInputMethodContext({{QStringLiteral("enabled"), false}});
    QVERIFY(!canvas.inputMethodQuery(Qt::ImEnabled).toBool());
    QVERIFY(canvas.preeditText().isEmpty());
    QCOMPARE(event_count, 2);

    QInputMethodEvent stale_commit;
    stale_commit.setCommitString(QStringLiteral("你好"));
    QCoreApplication::sendEvent(&canvas, &stale_commit);
    QVERIFY(canvas.preeditText().isEmpty());
    QCOMPARE(event_count, 2);
}

#ifdef CARET_ENABLE_APP
void ShellLoadTest::glyphFrameAdapterRejectsFramesOutsideCanvasLimits() {
    const auto invalid_scale_result =
        caret::glyph_frame_adapter::to_variant_map(caret::ffi::PageGlyphFrame{}, 0.0);
    QVERIFY(!invalid_scale_result.value(QStringLiteral("valid")).toBool());
    QCOMPARE(invalid_scale_result.value(QStringLiteral("error")).toString(),
             QStringLiteral("layout.glyphs_invalid_request"));

    caret::ffi::PageGlyphFrame oversized_page_frame{};
    oversized_page_frame.page_index = std::numeric_limits<std::uint32_t>::max();
    const auto oversized_page_result =
        caret::glyph_frame_adapter::to_variant_map(oversized_page_frame, 1.0);
    QVERIFY(!oversized_page_result.value(QStringLiteral("valid")).toBool());
    QCOMPARE(oversized_page_result.value(QStringLiteral("error")).toString(),
             QStringLiteral("layout.glyphs_invalid_request"));

    caret::ffi::PageGlyphFrame oversized_image_frame{};
    caret::ffi::GlyphImage oversized_image{};
    oversized_image.width = static_cast<std::uint32_t>(std::numeric_limits<int>::max() / 4 + 1);
    oversized_image_frame.images.push_back(oversized_image);
    const auto oversized_image_result =
        caret::glyph_frame_adapter::to_variant_map(oversized_image_frame, 1.0);
    QVERIFY(!oversized_image_result.value(QStringLiteral("valid")).toBool());
    QCOMPARE(oversized_image_result.value(QStringLiteral("error")).toString(),
             QStringLiteral("layout.glyphs_invalid_image"));

    const auto invalid_width_result = [] {
        caret::ffi::PageGlyphFrame frame{};
        caret::ffi::GlyphShape shape{};
        shape.kind = 0;
        shape.width_pt = -1.0;
        shape.stroke_enabled = true;
        shape.stroke_width_pt = 3.0;
        frame.shapes.push_back(shape);
        return caret::glyph_frame_adapter::to_variant_map(frame, 1.0);
    }();
    QVERIFY(!invalid_width_result.value(QStringLiteral("valid")).toBool());
    QCOMPARE(invalid_width_result.value(QStringLiteral("error")).toString(),
             QStringLiteral("layout.glyphs_invalid_shape"));

    const auto invalid_height_result = [] {
        caret::ffi::PageGlyphFrame frame{};
        caret::ffi::GlyphShape shape{};
        shape.kind = 0;
        shape.height_pt = -1.0;
        shape.stroke_enabled = true;
        shape.stroke_width_pt = 3.0;
        frame.shapes.push_back(shape);
        return caret::glyph_frame_adapter::to_variant_map(frame, 1.0);
    }();
    QVERIFY(!invalid_height_result.value(QStringLiteral("valid")).toBool());
    QCOMPARE(invalid_height_result.value(QStringLiteral("error")).toString(),
             QStringLiteral("layout.glyphs_invalid_shape"));

    caret::ffi::PageGlyphFrame zero_size_line_frame{};
    caret::ffi::GlyphShape zero_size_line{};
    zero_size_line.kind = 0;
    zero_size_line.dx_pt = 4.0;
    zero_size_line.stroke_enabled = true;
    zero_size_line.stroke_width_pt = 3.0;
    zero_size_line_frame.shapes.push_back(zero_size_line);
    QVERIFY(caret::glyph_frame_adapter::to_variant_map(zero_size_line_frame, 1.0)
                .value(QStringLiteral("valid"))
                .toBool());
}

void ShellLoadTest::underlinedTextRendersTheRustFfiDecorationInTheQtCanvas() {
    const QPointF phase_baseline(10.0, 20.0);
    const QRectF adjusted_glyph_rect = caret::canvas_detail::glyph_image_rect(
        phase_baseline, QPointF(3.0, -8.0), QSizeF(7.0, 9.0), 2.0, QPointF(0.25, 0.5), true);
    const QRectF unadjusted_glyph_rect = caret::canvas_detail::glyph_image_rect(
        phase_baseline, QPointF(3.0, -8.0), QSizeF(7.0, 9.0), 2.0, QPointF(0.25, 0.5), false);
    QCOMPARE(adjusted_glyph_rect, QRectF(15.5, 35.0, 14.0, 18.0));
    QCOMPARE(unadjusted_glyph_rect, QRectF(16.0, 36.0, 14.0, 18.0));

    auto runtime = caret::ffi::new_blank_document_runtime();
    const rust::String initial_snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(
        !std::string_view(initial_snapshot.data(), initial_snapshot.size()).starts_with("error:"));
    const QJsonObject initial =
        QJsonDocument::fromJson(
            QByteArray(initial_snapshot.data(), static_cast<qsizetype>(initial_snapshot.size())))
            .object();
    const QJsonArray paragraphs = initial.value(QStringLiteral("paragraphs")).toArray();
    QVERIFY(!paragraphs.isEmpty());
    const QString paragraph_id =
        paragraphs.first().toObject().value(QStringLiteral("paragraphId")).toString();
    QVERIFY(!paragraph_id.isEmpty());
    const QJsonObject endpoint{{QStringLiteral("paragraphId"), paragraph_id},
                               {QStringLiteral("offsetUtf8"), QStringLiteral("0")},
                               {QStringLiteral("affinity"), QStringLiteral("downstream")},
                               {QStringLiteral("visualAffinity"), QStringLiteral("downstream")}};
    const QJsonObject focus{{QStringLiteral("paragraphId"), paragraph_id},
                            {QStringLiteral("offsetUtf8"), QStringLiteral("10")},
                            {QStringLiteral("affinity"), QStringLiteral("upstream")},
                            {QStringLiteral("visualAffinity"), QStringLiteral("upstream")}};
    const rust::String inserted = caret::ffi::insert_text_json(*runtime, "underlined");
    QVERIFY(!std::string_view(inserted.data(), inserted.size()).starts_with("error:"));
    const rust::String selected = caret::ffi::set_selection_json(
        *runtime, QJsonDocument(QJsonObject{{QStringLiteral("anchor"), endpoint},
                                            {QStringLiteral("focus"), focus}})
                      .toJson(QJsonDocument::Compact)
                      .toStdString());
    QVERIFY(!std::string_view(selected.data(), selected.size()).starts_with("error:"));
    for (const auto& [action, text] :
         {std::pair{QStringLiteral("begin"), QString{}},
          std::pair{QStringLiteral("type"), QStringLiteral("underline")},
          std::pair{QStringLiteral("accept"), QString{}}}) {
        const rust::String response = caret::ffi::command_line_command_json(
            *runtime, action.toStdString(), text.toStdString());
        QVERIFY(!std::string_view(response.data(), response.size()).starts_with("error:"));
    }

    const rust::String updated_snapshot = caret::ffi::layout_snapshot_json(*runtime);
    QVERIFY(
        !std::string_view(updated_snapshot.data(), updated_snapshot.size()).starts_with("error:"));
    QVERIFY(caret::install_layout_snapshot(QString::fromUtf8(
        updated_snapshot.data(), static_cast<qsizetype>(updated_snapshot.size()))));
    QVERIFY(caret::install_document_runtime(std::move(runtime)));

    caret::LayoutViewModel view_model;
    const QVariantMap request =
        view_model.rasterizePageGlyphs(0, 96.0 / 72.0, 0.0, 0.0, 32, 32, 32, 255);
    QVERIFY2(request.value(QStringLiteral("valid")).toBool(),
             qPrintable(request.value(QStringLiteral("error")).toString()));
    const QVariantList decorations = request.value(QStringLiteral("decorations")).toList();
    QVERIFY(!decorations.isEmpty());
    const QVariantMap decoration = decorations.first().toMap();
    QVariantMap decoration_only_request = request;
    decoration_only_request.insert(QStringLiteral("images"), QVariantList{});
    decoration_only_request.insert(QStringLiteral("placements"), QVariantList{});
    decoration_only_request.insert(QStringLiteral("shapes"),
                                   QVariantList{
                                       QVariantMap{{QStringLiteral("kind"), 0},
                                                   {QStringLiteral("xPt"), 200.0},
                                                   {QStringLiteral("yPt"), 200.0},
                                                   {QStringLiteral("dxPt"), 30.0},
                                                   {QStringLiteral("dyPt"), 0.0},
                                                   {QStringLiteral("widthPt"), 0.0},
                                                   {QStringLiteral("heightPt"), 0.0},
                                                   {QStringLiteral("fillEnabled"), false},
                                                   {QStringLiteral("fillRed"), 0},
                                                   {QStringLiteral("fillGreen"), 0},
                                                   {QStringLiteral("fillBlue"), 0},
                                                   {QStringLiteral("fillAlpha"), 0},
                                                   {QStringLiteral("strokeEnabled"), true},
                                                   {QStringLiteral("strokeRed"), 20},
                                                   {QStringLiteral("strokeGreen"), 40},
                                                   {QStringLiteral("strokeBlue"), 240},
                                                   {QStringLiteral("strokeAlpha"), 255},
                                                   {QStringLiteral("strokeWidthPt"), 3.0},
                                                   {QStringLiteral("clipEnabled"), false},
                                                   {QStringLiteral("clipY0Pt"), 0.0},
                                                   {QStringLiteral("clipY1Pt"), 0.0}},
                                       QVariantMap{{QStringLiteral("kind"), 1},
                                                   {QStringLiteral("xPt"), 250.0},
                                                   {QStringLiteral("yPt"), 200.0},
                                                   {QStringLiteral("dxPt"), 0.0},
                                                   {QStringLiteral("dyPt"), 0.0},
                                                   {QStringLiteral("widthPt"), 30.0},
                                                   {QStringLiteral("heightPt"), 20.0},
                                                   {QStringLiteral("fillEnabled"), true},
                                                   {QStringLiteral("fillRed"), 20},
                                                   {QStringLiteral("fillGreen"), 230},
                                                   {QStringLiteral("fillBlue"), 40},
                                                   {QStringLiteral("fillAlpha"), 255},
                                                   {QStringLiteral("strokeEnabled"), true},
                                                   {QStringLiteral("strokeRed"), 240},
                                                   {QStringLiteral("strokeGreen"), 30},
                                                   {QStringLiteral("strokeBlue"), 30},
                                                   {QStringLiteral("strokeAlpha"), 255},
                                                   {QStringLiteral("strokeWidthPt"), 2.0},
                                                   {QStringLiteral("clipEnabled"), false},
                                                   {QStringLiteral("clipY0Pt"), 0.0},
                                                   {QStringLiteral("clipY1Pt"), 0.0}},
                                   });

    auto* canvas = new caret::DocumentCanvas();
    canvas->setPageSizesPt(view_model.pageSizesPt());
    canvas->setPageMarginsPt(view_model.pageMarginsPt());
    canvas->setGlyphFrames(QVariantList{decoration_only_request});
    QCOMPARE(canvas->glyphFrames().size(), 1);
    const QVariantList accepted_glyph_frames = canvas->glyphFrames();
    QVariantMap invalid_shape =
        decoration_only_request.value(QStringLiteral("shapes")).toList().first().toMap();
    invalid_shape.insert(QStringLiteral("paintOrder"), QStringLiteral("invalid"));
    QVariantMap invalid_shape_frame = decoration_only_request;
    invalid_shape_frame.insert(QStringLiteral("shapes"), QVariantList{invalid_shape});
    canvas->setGlyphFrames(QVariantList{invalid_shape_frame});
    QCOMPARE(canvas->glyphFrames(), accepted_glyph_frames);
    QVariantMap invalid_clip_shape =
        decoration_only_request.value(QStringLiteral("shapes")).toList().first().toMap();
    invalid_clip_shape.insert(QStringLiteral("clipEnabled"), true);
    invalid_clip_shape.remove(QStringLiteral("clipY0Pt"));
    invalid_clip_shape.insert(QStringLiteral("clipY1Pt"), 220.0);
    QVariantMap invalid_clip_frame = decoration_only_request;
    invalid_clip_frame.insert(QStringLiteral("shapes"), QVariantList{invalid_clip_shape});
    canvas->setGlyphFrames(QVariantList{invalid_clip_frame});
    QCOMPARE(canvas->glyphFrames(), accepted_glyph_frames);
    QVariantMap invalid_line_shape =
        decoration_only_request.value(QStringLiteral("shapes")).toList().first().toMap();
    invalid_line_shape.insert(QStringLiteral("widthPt"), -1.0);
    QVariantMap invalid_line_frame = decoration_only_request;
    invalid_line_frame.insert(QStringLiteral("shapes"), QVariantList{invalid_line_shape});
    canvas->setGlyphFrames(QVariantList{invalid_line_frame});
    QCOMPARE(canvas->glyphFrames(), accepted_glyph_frames);
    QVERIFY(!canvas->pageSizesPt().isEmpty());
    QQuickWindow window;
    window.resize(1000, 1200);
    canvas->setParent(&window);
    canvas->setParentItem(window.contentItem());
    canvas->setWidth(1000);
    canvas->setHeight(1200);
    window.show();
    QTRY_VERIFY_WITH_TIMEOUT(window.isExposed(), 3000);

    QTRY_VERIFY_WITH_TIMEOUT(
        ([&]() {
            const QImage frame = window.grabWindow().convertToFormat(QImage::Format_ARGB32);
            if (frame.isNull()) {
                return false;
            }
            const qreal scale_x = static_cast<qreal>(frame.width()) / window.width();
            const qreal scale_y = static_cast<qreal>(frame.height()) / window.height();
            const QPointF line_point = canvas->documentToContent(0, QPointF(215.0, 200.0));
            const QPointF fill_point = canvas->documentToContent(0, QPointF(265.0, 210.0));
            const QRgb line_pixel =
                frame.pixel(qBound(0, qRound(line_point.x() * scale_x), frame.width() - 1),
                            qBound(0, qRound(line_point.y() * scale_y), frame.height() - 1));
            const QRgb fill_pixel =
                frame.pixel(qBound(0, qRound(fill_point.x() * scale_x), frame.width() - 1),
                            qBound(0, qRound(fill_point.y() * scale_y), frame.height() - 1));
            return qBlue(line_pixel) > 180 && qRed(line_pixel) < 100 && qGreen(line_pixel) < 100 &&
                   qGreen(fill_pixel) > 180 && qRed(fill_pixel) < 100;
        }()),
        3000);

    const int page_index = request.value(QStringLiteral("pageIndex")).toInt();
    const QPointF decoration_origin = canvas->documentToContent(
        page_index, QPointF(decoration.value(QStringLiteral("xPt")).toReal(),
                            decoration.value(QStringLiteral("yPt")).toReal()));
    QTRY_VERIFY_WITH_TIMEOUT(
        ([&]() {
            const QImage frame = window.grabWindow();
            if (frame.isNull()) {
                return false;
            }
            const qreal scale_x = static_cast<qreal>(frame.width()) / window.width();
            const qreal scale_y = static_cast<qreal>(frame.height()) / window.height();
            const qreal left = decoration_origin.x() * scale_x;
            const qreal top = decoration_origin.y() * scale_y;
            const qreal logical_scale = request.value(QStringLiteral("rasterPxPerPt")).toReal();
            const qreal right = left + decoration.value(QStringLiteral("widthPt")).toReal() *
                                           logical_scale * scale_x;
            const qreal bottom = top + decoration.value(QStringLiteral("heightPt")).toReal() *
                                           logical_scale * scale_y;
            const QRect pixel_rect(QPoint(qFloor(left), qFloor(top)),
                                   QPoint(qCeil(right), qCeil(bottom)));
            const QImage region = frame.copy(pixel_rect.intersected(frame.rect()))
                                      .convertToFormat(QImage::Format_ARGB32);
            if (region.isNull()) {
                return false;
            }
            const auto* first_pixel = reinterpret_cast<const QRgb*>(region.constBits());
            const auto* last_pixel = first_pixel + region.sizeInBytes() / sizeof(QRgb);
            return std::any_of(first_pixel, last_pixel,
                               [](QRgb pixel) { return qGray(pixel) < 160; });
        }()),
        3000);

    const QVariantList placements = request.value(QStringLiteral("placements")).toList();
    QVariantMap first_placement = placements.first().toMap();
    const QVariantList images = request.value(QStringLiteral("images")).toList();
    QVariantMap first_image =
        images.at(first_placement.value(QStringLiteral("imageIndex")).toInt()).toMap();
    const qreal raster_scale = request.value(QStringLiteral("rasterPxPerPt")).toReal();
    // 显式指定非零像素相位，避免断言依赖平台栅格结果。
    first_placement.insert(QStringLiteral("pixelPhaseX"), 0.25);
    first_placement.insert(QStringLiteral("pixelPhaseY"), 0.5);
    first_placement.insert(QStringLiteral("pixelPhaseApplied"), true);
    first_placement.insert(QStringLiteral("imageIndex"), 0);
    first_image.insert(QStringLiteral("pixelPhaseApplied"), true);
    const qreal phase_x = first_placement.value(QStringLiteral("pixelPhaseX")).toReal();
    const qreal phase_y = first_placement.value(QStringLiteral("pixelPhaseY")).toReal();
    QVERIFY(phase_x > 0.0);
    QVERIFY(phase_y > 0.0);
    const qreal glyph_x =
        first_placement.value(QStringLiteral("xPt")).toReal() +
        (first_image.value(QStringLiteral("left")).toReal() - phase_x) / raster_scale;
    const qreal glyph_y =
        first_placement.value(QStringLiteral("baselineYPt")).toReal() +
        (-first_image.value(QStringLiteral("top")).toReal() - phase_y) / raster_scale;
    const qreal glyph_width = first_image.value(QStringLiteral("width")).toReal() / raster_scale;
    const qreal glyph_height = first_image.value(QStringLiteral("height")).toReal() / raster_scale;
    QVariantMap ordered_frame = request;
    ordered_frame.insert(QStringLiteral("images"), QVariantList{first_image});
    ordered_frame.insert(QStringLiteral("placements"), QVariantList{first_placement});
    ordered_frame.insert(QStringLiteral("decorations"), QVariantList{});
    ordered_frame.insert(QStringLiteral("shapes"), QVariantList{QVariantMap{
                                                       {QStringLiteral("kind"), 1},
                                                       {QStringLiteral("xPt"), glyph_x},
                                                       {QStringLiteral("yPt"), glyph_y},
                                                       {QStringLiteral("dxPt"), 0.0},
                                                       {QStringLiteral("dyPt"), 0.0},
                                                       {QStringLiteral("widthPt"), glyph_width},
                                                       {QStringLiteral("heightPt"), glyph_height},
                                                       {QStringLiteral("fillEnabled"), true},
                                                       {QStringLiteral("fillRed"), 20},
                                                       {QStringLiteral("fillGreen"), 220},
                                                       {QStringLiteral("fillBlue"), 40},
                                                       {QStringLiteral("fillAlpha"), 255},
                                                       {QStringLiteral("strokeEnabled"), false},
                                                       {QStringLiteral("strokeRed"), 0},
                                                       {QStringLiteral("strokeGreen"), 0},
                                                       {QStringLiteral("strokeBlue"), 0},
                                                       {QStringLiteral("strokeAlpha"), 0},
                                                       {QStringLiteral("strokeWidthPt"), 0.0},
                                                       {QStringLiteral("clipEnabled"), false},
                                                       {QStringLiteral("clipY0Pt"), 0.0},
                                                       {QStringLiteral("clipY1Pt"), 0.0},
                                                       {QStringLiteral("paintOrder"), 0},
                                                   }});
    canvas->setGlyphFrames(QVariantList{ordered_frame});
    QPoint glyph_ink_pixel(-1, -1);
    QTRY_VERIFY_WITH_TIMEOUT(
        ([&]() {
            const QImage frame = window.grabWindow().convertToFormat(QImage::Format_ARGB32);
            if (frame.isNull()) {
                return false;
            }
            const qreal scale_x = static_cast<qreal>(frame.width()) / window.width();
            const qreal scale_y = static_cast<qreal>(frame.height()) / window.height();
            const QPointF origin = canvas->documentToContent(0, QPointF(glyph_x, glyph_y));
            const QRect region(qFloor(origin.x() * scale_x), qFloor(origin.y() * scale_y),
                               qCeil(glyph_width * raster_scale * scale_x),
                               qCeil(glyph_height * raster_scale * scale_y));
            const QRect visible_region = region.intersected(frame.rect());
            const QImage pixels = frame.copy(visible_region).convertToFormat(QImage::Format_ARGB32);
            if (pixels.isNull()) {
                return false;
            }
            const auto* pixel_begin = reinterpret_cast<const QRgb*>(pixels.constBits());
            const auto* pixel_end = pixel_begin + pixels.sizeInBytes() / sizeof(QRgb);
            const bool saw_fill = std::any_of(pixel_begin, pixel_end, [](QRgb pixel) {
                return qGreen(pixel) > 170 && qRed(pixel) < 100;
            });
            const auto ink_pixel =
                std::find_if(pixel_begin, pixel_end, [](QRgb pixel) { return qGray(pixel) < 120; });
            if (!saw_fill || ink_pixel == pixel_end || pixels.width() <= 0) {
                return false;
            }
            const qsizetype ink_index = std::distance(pixel_begin, ink_pixel);
            glyph_ink_pixel =
                visible_region.topLeft() + QPoint(static_cast<int>(ink_index % pixels.width()),
                                                  static_cast<int>(ink_index / pixels.width()));
            return true;
        }()),
        3000);

    QVariantMap overlay_shape =
        ordered_frame.value(QStringLiteral("shapes")).toList().first().toMap();
    overlay_shape.insert(QStringLiteral("paintOrder"),
                         QVariant::fromValue<qulonglong>(std::numeric_limits<qulonglong>::max()));
    ordered_frame.insert(QStringLiteral("shapes"), QVariantList{overlay_shape});
    canvas->setGlyphFrames(QVariantList{ordered_frame});
    QTRY_VERIFY_WITH_TIMEOUT(
        ([&]() {
            const QImage frame = window.grabWindow().convertToFormat(QImage::Format_ARGB32);
            if (frame.isNull()) {
                return false;
            }
            if (glyph_ink_pixel.x() < 0 || glyph_ink_pixel.y() < 0 ||
                glyph_ink_pixel.x() >= frame.width() || glyph_ink_pixel.y() >= frame.height()) {
                return false;
            }
            const QRgb pixel = frame.pixel(glyph_ink_pixel);
            return qGreen(pixel) > 170 && qRed(pixel) < 100 && qBlue(pixel) < 100;
        }()),
        3000);

    QVariantMap clipped_frame = ordered_frame;
    clipped_frame.insert(QStringLiteral("placements"), QVariantList{});
    clipped_frame.insert(QStringLiteral("images"), QVariantList{});
    QVariantMap edge_line =
        decoration_only_request.value(QStringLiteral("shapes")).toList().first().toMap();
    edge_line.insert(QStringLiteral("yPt"), 200.0);
    edge_line.insert(QStringLiteral("strokeRed"), 240);
    edge_line.insert(QStringLiteral("strokeGreen"), 30);
    edge_line.insert(QStringLiteral("strokeBlue"), 30);
    edge_line.insert(QStringLiteral("strokeWidthPt"), 3.0);
    edge_line.insert(QStringLiteral("clipEnabled"), true);
    edge_line.insert(QStringLiteral("clipY0Pt"), 200.0);
    edge_line.insert(QStringLiteral("clipY1Pt"), 220.0);
    edge_line.insert(QStringLiteral("paintOrder"), 0);
    clipped_frame.insert(QStringLiteral("shapes"), QVariantList{edge_line});
    QVariantMap unclipped_frame = clipped_frame;
    edge_line.insert(QStringLiteral("clipEnabled"), false);
    unclipped_frame.insert(QStringLiteral("shapes"), QVariantList{edge_line});
    auto* unclipped_canvas = new caret::DocumentCanvas();
    unclipped_canvas->setPageSizesPt(view_model.pageSizesPt());
    unclipped_canvas->setPageMarginsPt(view_model.pageMarginsPt());
    unclipped_canvas->setGlyphFrames(QVariantList{unclipped_frame});
    QQuickWindow unclipped_window;
    unclipped_window.resize(1000, 1200);
    unclipped_canvas->setParent(&unclipped_window);
    unclipped_canvas->setParentItem(unclipped_window.contentItem());
    unclipped_canvas->setWidth(1000);
    unclipped_canvas->setHeight(1200);
    unclipped_window.show();
    QTRY_VERIFY_WITH_TIMEOUT(unclipped_window.isExposed(), 3000);
    const auto has_red_pixel_at = [](const QImage& frame, QQuickWindow& window,
                                     caret::DocumentCanvas* target_canvas,
                                     const QPointF& document_point) {
        const qreal scale_x = static_cast<qreal>(frame.width()) / window.width();
        const qreal scale_y = static_cast<qreal>(frame.height()) / window.height();
        const QPointF point = target_canvas->documentToContent(0, document_point);
        const int x = qBound(0, qRound(point.x() * scale_x), frame.width() - 1);
        const int y = qBound(0, qRound(point.y() * scale_y), frame.height() - 1);
        const QRgb pixel = frame.pixel(x, y);
        return qRed(pixel) > 170 && qGreen(pixel) < 100;
    };
    QTRY_VERIFY_WITH_TIMEOUT(
        ([&]() {
            const QImage frame =
                unclipped_window.grabWindow().convertToFormat(QImage::Format_ARGB32);
            if (frame.isNull()) {
                return false;
            }
            const qreal scale_x = static_cast<qreal>(frame.width()) / unclipped_window.width();
            const qreal scale_y = static_cast<qreal>(frame.height()) / unclipped_window.height();
            const QPointF visible_top =
                unclipped_canvas->documentToContent(0, QPointF(212.0, 200.5));
            const QPointF visible_bottom =
                unclipped_canvas->documentToContent(0, QPointF(218.0, 202.0));
            const QRect region(
                QPoint(qFloor(visible_top.x() * scale_x), qFloor(visible_top.y() * scale_y)),
                QPoint(qCeil(visible_bottom.x() * scale_x), qCeil(visible_bottom.y() * scale_y)));
            const QImage pixels =
                frame.copy(region.intersected(frame.rect())).convertToFormat(QImage::Format_ARGB32);
            if (pixels.isNull()) {
                return false;
            }
            const auto* pixel_begin = reinterpret_cast<const QRgb*>(pixels.constBits());
            const auto* pixel_end = pixel_begin + pixels.sizeInBytes() / sizeof(QRgb);
            const bool line_visible = std::any_of(pixel_begin, pixel_end, [](QRgb pixel) {
                return qRed(pixel) > 170 && qGreen(pixel) < 100;
            });
            return line_visible && has_red_pixel_at(frame, unclipped_window, unclipped_canvas,
                                                    QPointF(215.0, 199.5));
        }()),
        3000);
    auto* clip_canvas = new caret::DocumentCanvas();
    clip_canvas->setPageSizesPt(view_model.pageSizesPt());
    clip_canvas->setPageMarginsPt(view_model.pageMarginsPt());
    clip_canvas->setGlyphFrames(QVariantList{clipped_frame});
    QQuickWindow clip_window;
    clip_window.resize(1000, 1200);
    clip_canvas->setParent(&clip_window);
    clip_canvas->setParentItem(clip_window.contentItem());
    clip_canvas->setWidth(1000);
    clip_canvas->setHeight(1200);
    clip_window.show();
    QTRY_VERIFY_WITH_TIMEOUT(clip_window.isExposed(), 3000);
    QTRY_VERIFY_WITH_TIMEOUT(
        ([&]() {
            const QImage frame = clip_window.grabWindow().convertToFormat(QImage::Format_ARGB32);
            if (frame.isNull()) {
                return false;
            }
            const qreal scale_x = static_cast<qreal>(frame.width()) / clip_window.width();
            const qreal scale_y = static_cast<qreal>(frame.height()) / clip_window.height();
            const QPointF visible_top = clip_canvas->documentToContent(0, QPointF(212.0, 200.5));
            const QPointF visible_bottom = clip_canvas->documentToContent(0, QPointF(218.0, 202.0));
            const QRect region(
                QPoint(qFloor(visible_top.x() * scale_x), qFloor(visible_top.y() * scale_y)),
                QPoint(qCeil(visible_bottom.x() * scale_x), qCeil(visible_bottom.y() * scale_y)));
            const QImage pixels =
                frame.copy(region.intersected(frame.rect())).convertToFormat(QImage::Format_ARGB32);
            if (pixels.isNull()) {
                return false;
            }
            const auto* pixel_begin = reinterpret_cast<const QRgb*>(pixels.constBits());
            const auto* pixel_end = pixel_begin + pixels.sizeInBytes() / sizeof(QRgb);
            const bool line_visible = std::any_of(pixel_begin, pixel_end, [](QRgb pixel) {
                return qRed(pixel) > 170 && qGreen(pixel) < 100;
            });
            return line_visible &&
                   !has_red_pixel_at(frame, clip_window, clip_canvas, QPointF(215.0, 199.5));
        }()),
        3000);

    QVariantMap border_shape{{QStringLiteral("kind"), 1},
                             {QStringLiteral("xPt"), 250.0},
                             {QStringLiteral("yPt"), 200.0},
                             {QStringLiteral("dxPt"), 0.0},
                             {QStringLiteral("dyPt"), 0.0},
                             {QStringLiteral("widthPt"), 40.0},
                             {QStringLiteral("heightPt"), 30.0},
                             {QStringLiteral("fillEnabled"), false},
                             {QStringLiteral("fillRed"), 0},
                             {QStringLiteral("fillGreen"), 0},
                             {QStringLiteral("fillBlue"), 0},
                             {QStringLiteral("fillAlpha"), 0},
                             {QStringLiteral("strokeEnabled"), true},
                             {QStringLiteral("strokeRed"), 240},
                             {QStringLiteral("strokeGreen"), 20},
                             {QStringLiteral("strokeBlue"), 20},
                             {QStringLiteral("strokeAlpha"), 128},
                             {QStringLiteral("strokeWidthPt"), 4.0},
                             {QStringLiteral("clipEnabled"), false},
                             {QStringLiteral("clipY0Pt"), 0.0},
                             {QStringLiteral("clipY1Pt"), 0.0},
                             {QStringLiteral("paintOrder"), 0}};
    border_shape.insert(QStringLiteral("kind"), 1.0);
    border_shape.insert(QStringLiteral("strokeRed"), 240.0);
    border_shape.insert(QStringLiteral("strokeGreen"), 20.0);
    border_shape.insert(QStringLiteral("strokeBlue"), 20.0);
    border_shape.insert(QStringLiteral("strokeAlpha"), 128.0);
    border_shape.insert(QStringLiteral("paintOrder"), 0.0);
    QVariantMap wide_border_shape = border_shape;
    wide_border_shape.insert(QStringLiteral("xPt"), 250.0);
    wide_border_shape.insert(QStringLiteral("yPt"), 250.0);
    wide_border_shape.insert(QStringLiteral("widthPt"), 10.0);
    wide_border_shape.insert(QStringLiteral("heightPt"), 20.0);
    wide_border_shape.insert(QStringLiteral("strokeWidthPt"), 12.0);
    QVariantMap border_frame = clipped_frame;
    border_frame.insert(QStringLiteral("shapes"), QVariantList{border_shape, wide_border_shape});
    auto* border_canvas = new caret::DocumentCanvas();
    border_canvas->setPageSizesPt(view_model.pageSizesPt());
    border_canvas->setPageMarginsPt(view_model.pageMarginsPt());
    border_canvas->setGlyphFrames(QVariantList{border_frame});
    QCOMPARE(border_canvas->glyphFrames().size(), 1);
    QCOMPARE(border_canvas->glyphFrames()
                 .first()
                 .toMap()
                 .value(QStringLiteral("shapes"))
                 .toList()
                 .size(),
             2);
    QQuickWindow border_window;
    border_window.resize(1000, 1200);
    border_canvas->setParent(&border_window);
    border_canvas->setParentItem(border_window.contentItem());
    border_canvas->setWidth(1000);
    border_canvas->setHeight(1200);
    border_window.show();
    QTRY_VERIFY_WITH_TIMEOUT(border_window.isExposed(), 3000);
    QTRY_VERIFY_WITH_TIMEOUT(
        ([&]() {
            const QImage frame = border_window.grabWindow().convertToFormat(QImage::Format_ARGB32);
            if (frame.isNull()) {
                return false;
            }
            const qreal scale_x = static_cast<qreal>(frame.width()) / border_window.width();
            const qreal scale_y = static_cast<qreal>(frame.height()) / border_window.height();
            const auto pixel_at = [&](const QPointF& document_point) {
                const QPointF point = border_canvas->documentToContent(0, document_point);
                return frame.pixel(qBound(0, qRound(point.x() * scale_x), frame.width() - 1),
                                   qBound(0, qRound(point.y() * scale_y), frame.height() - 1));
            };
            const QRgb outer_corner = pixel_at(QPointF(248.5, 198.5));
            const QRgb top_edge = pixel_at(QPointF(270.0, 198.5));
            const QRgb inner_corner = pixel_at(QPointF(250.5, 200.5));
            const QRgb left_edge = pixel_at(QPointF(248.5, 215.0));
            const QRgb wide_stroke = pixel_at(QPointF(255.0, 260.0));
            const QRgb rectangle_interior = pixel_at(QPointF(270.0, 215.0));
            const auto is_semitransparent_red = [](QRgb pixel) {
                return qRed(pixel) > 220 && qGreen(pixel) > 80 && qGreen(pixel) < 190 &&
                       qBlue(pixel) > 80 && qBlue(pixel) < 190;
            };
            return outer_corner == top_edge && inner_corner == left_edge &&
                   is_semitransparent_red(top_edge) && is_semitransparent_red(left_edge) &&
                   qGreen(rectangle_interior) > qGreen(top_edge) &&
                   is_semitransparent_red(wide_stroke);
        }()),
        3000);
}
#endif

void ShellLoadTest::canvasPolicyPublishesDocumentedZoomLimits() {
    const caret::CanvasPolicy policy;
    QCOMPARE(policy.defaultZoomPercent(), 100);
    QCOMPARE(policy.minimumZoomPercent(), 50);
    QCOMPARE(policy.maximumZoomPercent(), 300);
    QCOMPARE(policy.zoomStepPercent(), 10);
}

void ShellLoadTest::documentCanvasMapsPageCoordinatesAndHitsPages() {
    constexpr int kTestCursorFlashPeriodMs = 100;
    constexpr int kTestBlinkTransitionTimeoutMs = 500;
    constexpr int kDisabledBlinkObservationMs = 250;

    const QString theme_path =
        QString::fromUtf8(CARET_SOURCE_DIR) + QStringLiteral("/qml/Themes/Theme.qml");
    QHash<QString, QVariantMap> themes;
    themes.insert(QStringLiteral("light"),
                  caret::shell_test::snapshot_from_theme_facade(theme_path, QColor(Qt::white), 16));
    caret::install_theme_snapshots(std::move(themes), QStringLiteral("light"));
    const QString empty_snapshot = QStringLiteral(
        R"({"documentGeneration":"0","fontGeneration":"1","marginsPt":{"top":72,"right":72,"bottom":72,"left":72},"pages":[{"widthPt":600,"heightPt":800,"fragments":[{"paragraphId":"p1","firstLine":0,"lineEnd":1,"yOffsetPt":0}]}],"paragraphs":[{"paragraphId":"p1","text":"","lines":[{"sourceStart":0,"sourceEnd":0,"consumedStart":0,"consumedEnd":0,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":0,"overflowsWidth":false}]}],"inputMethodContext":{"surroundingText":"","cursorPosition":0,"anchorPosition":0,"enabled":true}})");
    QString max_generation_snapshot = empty_snapshot;
    max_generation_snapshot.replace(
        QStringLiteral("\"documentGeneration\":\"0\""),
        QStringLiteral("\"documentGeneration\":\"18446744073709551615\""));
    QVERIFY(caret::install_layout_snapshot(max_generation_snapshot));
    const caret::LayoutViewModel max_generation_model;
    const QString max_generation = QString::number(std::numeric_limits<qulonglong>::max());
    QCOMPARE(max_generation_model.documentGeneration(), max_generation);
    QQmlEngine generation_engine;
    generation_engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent generation_component(&generation_engine);
    generation_component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.ViewModels\n"
                          "Item { property var generation: LayoutViewModel.documentGeneration }\n"),
        QUrl());
    QVERIFY2(generation_component.isReady(), qPrintable(generation_component.errorString()));
    QObject* generation_object = generation_component.create();
    QVERIFY(generation_object != nullptr);
    QCOMPARE(generation_object->property("generation").toString(), max_generation);
    delete generation_object;
    QVERIFY(caret::install_layout_snapshot(empty_snapshot));

    QVERIFY(!caret::install_layout_snapshot(QStringLiteral("{invalid")));
    QString unsafe_index_snapshot = empty_snapshot;
    unsafe_index_snapshot.replace(QStringLiteral("\"sourceStart\":0"),
                                  QStringLiteral("\"sourceStart\":9223372036854775808"));
    QVERIFY(!caret::install_layout_snapshot(unsafe_index_snapshot));
    const QString split_utf8_snapshot = QStringLiteral(
        R"({"documentGeneration":"0","fontGeneration":"1","marginsPt":{"top":72,"right":72,"bottom":72,"left":72},"pages":[],"paragraphs":[{"paragraphId":"p1","text":"éx","lines":[{"sourceStart":0,"sourceEnd":1,"consumedStart":0,"consumedEnd":1,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":6,"overflowsWidth":false},{"sourceStart":1,"sourceEnd":3,"consumedStart":1,"consumedEnd":3,"topPt":12,"baselinePt":21,"heightPt":12,"advancePt":6,"overflowsWidth":false}]}]})");
    QVERIFY(!caret::install_layout_snapshot(split_utf8_snapshot));
    const QString overlapping_lines_snapshot = QStringLiteral(
        R"({"documentGeneration":"0","fontGeneration":"1","marginsPt":{"top":72,"right":72,"bottom":72,"left":72},"pages":[],"paragraphs":[{"paragraphId":"p1","text":"ab","lines":[{"sourceStart":0,"sourceEnd":1,"consumedStart":0,"consumedEnd":1,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":6,"overflowsWidth":false},{"sourceStart":1,"sourceEnd":2,"consumedStart":1,"consumedEnd":2,"topPt":0,"baselinePt":9,"heightPt":12,"advancePt":6,"overflowsWidth":false}]}]})");
    QVERIFY(!caret::install_layout_snapshot(overlapping_lines_snapshot));
    QString negative_line_top = empty_snapshot;
    negative_line_top.replace(QStringLiteral("\"topPt\":0"), QStringLiteral("\"topPt\":-1"));
    QVERIFY(!caret::install_layout_snapshot(negative_line_top));
    QString outside_page_fragment = empty_snapshot;
    outside_page_fragment.replace(QStringLiteral("\"yOffsetPt\":0"),
                                  QStringLiteral("\"yOffsetPt\":1e300"));
    QVERIFY(!caret::install_layout_snapshot(outside_page_fragment));
    const caret::LayoutViewModel retained_snapshot_model;
    QCOMPARE(retained_snapshot_model.documentGeneration(), QStringLiteral("0"));
    QCOMPARE(retained_snapshot_model.fontGeneration(), QStringLiteral("1"));
    QQmlEngine engine;
    engine.addImportPath(QString::fromUtf8(CARET_QML_IMPORT_PATH));
    QQmlComponent view_model_component(&engine);
    view_model_component.setData(
        QByteArrayLiteral(
            "import QtQuick\n"
            "import Caret.ViewModels\n"
            "Item {\n"
            "    property var pageSizes: LayoutViewModel.pageSizesPt\n"
            "    property var pages: LayoutViewModel.pageLayouts\n"
            "    property var paragraphs: LayoutViewModel.paragraphLayouts\n"
            "    property string documentGeneration: LayoutViewModel.documentGeneration\n"
            "    property string fontGeneration: LayoutViewModel.fontGeneration\n"
            "}\n"),
        QUrl());
    QVERIFY2(view_model_component.isReady(), qPrintable(view_model_component.errorString()));
    QObject* view_model = view_model_component.create();
    QVERIFY(view_model != nullptr);
    const QVariantList page_sizes = view_model->property("pageSizes").toList();
    QCOMPARE(page_sizes.size(), 1);
    QCOMPARE(page_sizes.front().toSizeF(), QSizeF(600.0, 800.0));
    QCOMPARE(view_model->property("pages").toList().size(), 1);
    QCOMPARE(view_model->property("paragraphs").toList().size(), 1);
    QCOMPARE(view_model->property("documentGeneration").toString(), QStringLiteral("0"));
    QCOMPARE(view_model->property("fontGeneration").toString(), QStringLiteral("1"));
    QQmlComponent component(&engine);
    component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Canvas\n"
                          "Item {\n"
                          "    DocumentCanvas {\n"
                          "        objectName: \"canvas\"\n"
                          "        pageSizesPt: [Qt.size(600, 800), Qt.size(300, 400)]\n"
                          "        zoom: 1.5\n"
                          "        width: implicitWidth + 100\n"
                          "    }\n"
                          "}\n"),
        QUrl());
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    QObject* root = component.create();
    QVERIFY(root != nullptr);
    auto* root_item = qobject_cast<QQuickItem*>(root);
    QVERIFY(root_item != nullptr);
    QQuickWindow input_method_window;
    input_method_window.resize(1000, 700);
    root_item->setWidth(1300.0);
    root_item->setHeight(2600.0);
    root_item->setParentItem(input_method_window.contentItem());
    input_method_window.show();
    auto* canvas = root->findChild<caret::DocumentCanvas*>(QStringLiteral("canvas"));
    QVERIFY(canvas != nullptr);
    canvas->forceActiveFocus();
    QTRY_VERIFY_WITH_TIMEOUT(canvas->hasActiveFocus(), 3000);
    QSignalSpy text_input_spy(canvas, &caret::DocumentCanvas::textInputRequested);
    QVERIFY(text_input_spy.isValid());
    QSignalSpy editor_key_spy(canvas, &caret::DocumentCanvas::editorKeyRequested);
    QVERIFY(editor_key_spy.isValid());
    QSignalSpy visual_navigation_spy(canvas, &caret::DocumentCanvas::visualNavigationRequested);
    QVERIFY(visual_navigation_spy.isValid());
    QSignalSpy vertical_navigation_spy(canvas, &caret::DocumentCanvas::verticalNavigationRequested);
    QVERIFY(vertical_navigation_spy.isValid());
    QSignalSpy line_edge_navigation_spy(canvas,
                                        &caret::DocumentCanvas::lineEdgeNavigationRequested);
    QVERIFY(line_edge_navigation_spy.isValid());
    QKeyEvent altgr_input(QEvent::KeyPress, Qt::Key_Q,
                          Qt::ControlModifier | Qt::AltModifier | Qt::GroupSwitchModifier,
                          QStringLiteral("@"));
    QCoreApplication::sendEvent(canvas, &altgr_input);
    QCOMPARE(text_input_spy.size(), 1);
    QCOMPARE(text_input_spy.takeFirst().at(0).toString(), QStringLiteral("@"));
    QKeyEvent altgr_without_group_switch(
        QEvent::KeyPress, Qt::Key_Q, Qt::ControlModifier | Qt::AltModifier, QStringLiteral("@"));
    QCoreApplication::sendEvent(canvas, &altgr_without_group_switch);
    QCOMPARE(text_input_spy.size(), 1);
    QCOMPARE(text_input_spy.takeFirst().at(0).toString(), QStringLiteral("@"));
    QKeyEvent shortcut_input(QEvent::KeyPress, Qt::Key_X, Qt::ControlModifier, QStringLiteral("x"));
    QCoreApplication::sendEvent(canvas, &shortcut_input);
    QCOMPARE(text_input_spy.size(), 0);
    QKeyEvent keypad_enter(QEvent::KeyPress, Qt::Key_Enter, Qt::KeypadModifier, QString());
    QCoreApplication::sendEvent(canvas, &keypad_enter);
    QCOMPARE(editor_key_spy.size(), 1);
    QCOMPARE(editor_key_spy.takeFirst().at(0).toString(), QStringLiteral("enter"));
    QKeyEvent shift_left(QEvent::KeyPress, Qt::Key_Left, Qt::ShiftModifier, QString());
    QCoreApplication::sendEvent(canvas, &shift_left);
    QCOMPARE(visual_navigation_spy.size(), 1);
    const QList<QVariant> navigation_arguments = visual_navigation_spy.takeFirst();
    QCOMPARE(navigation_arguments.at(0).toBool(), false);
    QCOMPARE(navigation_arguments.at(1).toBool(), true);
    QKeyEvent down(QEvent::KeyPress, Qt::Key_Down, Qt::NoModifier, QString());
    QCoreApplication::sendEvent(canvas, &down);
    QCOMPARE(vertical_navigation_spy.size(), 1);
    QCOMPARE(vertical_navigation_spy.takeFirst().at(0).toInt(), 1);
    QKeyEvent line_end(QEvent::KeyPress, Qt::Key_End, Qt::NoModifier, QString());
    QCoreApplication::sendEvent(canvas, &line_end);
    QCOMPARE(line_edge_navigation_spy.size(), 1);
    QCOMPARE(line_edge_navigation_spy.takeFirst().at(0).toBool(), true);

    const QSizeF natural_size = canvas->naturalContentSize();
    QCOMPARE(natural_size.width(), 1296.0);
    QCOMPARE(natural_size.height(), 2544.0);
    const QPointF origin = canvas->pageOrigin(1);
    QCOMPARE(origin, QPointF(398.0, 1696.0));

    const QPointF document_point(12.0, 20.0);
    const QPointF content_point = canvas->documentToContent(1, document_point);
    QCOMPARE(content_point, QPointF(422.0, 1736.0));
    QCOMPARE(canvas->contentToDocument(1, content_point), document_point);
    const QVariantMap hit = canvas->hitTestPage(content_point);
    QCOMPARE(hit.value(QStringLiteral("pageIndex")).toInt(), 1);
    QCOMPARE(hit.value(QStringLiteral("xPt")).toReal(), document_point.x());
    QCOMPARE(hit.value(QStringLiteral("yPt")).toReal(), document_point.y());
    QCOMPARE(canvas->hitTestPage(QPointF(-1.0, -1.0)).value(QStringLiteral("pageIndex")).toInt(),
             -1);

    const QVariantMap caret_geometry{{QStringLiteral("pageIndex"), 1},
                                     {QStringLiteral("xPt"), 12.0},
                                     {QStringLiteral("y0Pt"), 20.0},
                                     {QStringLiteral("y1Pt"), 30.0}};
    canvas->setCaretWidthCentipixels(100);
    canvas->setCaretGeometry(caret_geometry);
    const QVariantMap installed_caret = canvas->caretGeometry();
    QCOMPARE(installed_caret.value(QStringLiteral("pageIndex")).toInt(), 1);
    QVariantMap invalid_caret = caret_geometry;
    invalid_caret.insert(QStringLiteral("pageIndex"), 0.5);
    canvas->setCaretGeometry(invalid_caret);
    QCOMPARE(canvas->caretGeometry(), installed_caret);

    canvas->setInputMethodContext({{QStringLiteral("surroundingText"), QStringLiteral("A😀B")},
                                   {QStringLiteral("cursorPosition"), 3},
                                   {QStringLiteral("anchorPosition"), 1},
                                   {QStringLiteral("enabled"), true}});
    QCOMPARE(canvas->inputMethodQuery(Qt::ImSurroundingText).toString(), QStringLiteral("A😀B"));
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCursorPosition).toInt(), 3);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImAnchorPosition).toInt(), 1);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCurrentSelection).toString(), QStringLiteral("😀"));
    QList<QInputMethodEvent::Attribute> attributes;
    attributes.push_back(QInputMethodEvent::Attribute(QInputMethodEvent::Cursor, 2, 0, {}));
    QInputMethodEvent input_method_event(QStringLiteral("ni hao"), attributes);
    QCoreApplication::sendEvent(canvas, &input_method_event);
    QCOMPARE(canvas->preeditText(), QStringLiteral("ni hao"));
    QCOMPARE(canvas->preeditCursorPosition(), 2);
    const QRectF composition_caret = canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF();
    QCOMPARE(composition_caret.x(), canvas->documentToContent(1, QPointF(12.0, 20.0)).x());
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF(), composition_caret);
    canvas->cancelPreedit();
    QVERIFY(canvas->preeditText().isEmpty());
    const QRectF document_caret = canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF();
    const QRectF command_line_caret(40.0, 50.0, 1.0, 18.0);
    canvas->setInputMethodCursorRectangle(command_line_caret);
    QCOMPARE(canvas->inputMethodCursorRectangle(), command_line_caret);
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF(), command_line_caret);
    canvas->setInputMethodCursorRectangle(QRectF{});
    QCOMPARE(canvas->inputMethodQuery(Qt::ImCursorRectangle).toRectF(), document_caret);
    invalid_caret.insert(QStringLiteral("pageIndex"),
                         QVariant::fromValue<qulonglong>(4294967296ULL));
    canvas->setCaretGeometry(invalid_caret);
    QCOMPARE(canvas->caretGeometry(), installed_caret);

    // QQuickItem 输入坐标是逻辑像素，pt 映射不接收 DPR。
    for (const qreal zoom : {0.5, 1.0, 2.0}) {
        canvas->setZoom(zoom);
        const QPointF zoomed = canvas->documentToContent(1, document_point);
        QCOMPARE(canvas->contentToDocument(1, zoomed), document_point);
    }

    QQmlComponent surface_component(&engine);
    surface_component.setData(QByteArrayLiteral("import QtQuick\n"
                                                "import Caret.Shell\n"
                                                "PageSurface { width: 500; height: 500 }\n"),
                              QUrl());
    QVERIFY2(surface_component.isReady(), qPrintable(surface_component.errorString()));
    QObject* surface = surface_component.create();
    QVERIFY(surface != nullptr);
    auto* bound_canvas =
        surface->findChild<caret::DocumentCanvas*>(QStringLiteral("documentCanvas"));
    QVERIFY(bound_canvas != nullptr);
    QCOMPARE(bound_canvas->pageSizesPt().size(), 1);
    QCOMPARE(bound_canvas->pageSizesPt().front().toSizeF(), QSizeF(600.0, 800.0));
    QCOMPARE(bound_canvas->pageColor(), QColor(Qt::white));
    QCOMPARE(bound_canvas->pageBorderColor(), QColor(QStringLiteral("#c9c9c9")));
    QCOMPARE(bound_canvas->caretColor(), QColor(QStringLiteral("#1c1c1c")));
    QCOMPARE(bound_canvas->caretWidthCentipixels(), 100);

    QQmlComponent render_component(&engine);
    render_component.setData(
        QByteArrayLiteral("import QtQuick\n"
                          "import Caret.Canvas\n"
                          "DocumentCanvas {\n"
                          "    width: 200; height: 200\n"
                          "    pageColor: \"#ffffff\"\n"
                          "    pageBorderColor: \"#c8c8c8\"\n"
                          "    caretColor: \"#ff0000\"\n"
                          "    caretWidthCentipixels: 125\n"
                          "    environmentHighlightColor: \"#fff2cc\"\n"
                          "    environmentHighlightRadius: 4\n"
                          "    pageSizesPt: [Qt.size(60, 60), Qt.size(60, 60)]\n"
                          "    caretGeometry: ({ pageIndex: 0, xPt: 30, y0Pt: 20, y1Pt: 80 })\n"
                          "    pageMarginPt: 12; pageGapPt: 0\n"
                          "}\n"),
        QUrl());
    QVERIFY2(render_component.isReady(), qPrintable(render_component.errorString()));
    auto* rendered_canvas = qobject_cast<caret::DocumentCanvas*>(render_component.create());
    QVERIFY(rendered_canvas != nullptr);
    QCOMPARE(rendered_canvas->property("caretWidthCentipixels").toInt(), 125);
    QVERIFY(rendered_canvas->setProperty("environmentHighlightRects",
                                         QVariantList{QVariantMap{{QStringLiteral("pageIndex"), 0},
                                                                  {QStringLiteral("x0Pt"), 10},
                                                                  {QStringLiteral("y0Pt"), 10},
                                                                  {QStringLiteral("x1Pt"), 50},
                                                                  {QStringLiteral("y1Pt"), 20}},
                                                      QVariantMap{{QStringLiteral("pageIndex"), 1},
                                                                  {QStringLiteral("x0Pt"), 10},
                                                                  {QStringLiteral("y0Pt"), 10},
                                                                  {QStringLiteral("x1Pt"), 50},
                                                                  {QStringLiteral("y1Pt"), 20}}}));
    QVERIFY(rendered_canvas->setProperty("environmentHighlightColor",
                                         QColor(QStringLiteral("#fff2cc"))));
    constexpr qsizetype glyph_pixel_count = qsizetype{8} * 8;
    QByteArray glyph_pixels(glyph_pixel_count * 4, '\0');
    for (qsizetype pixel = 0; pixel < glyph_pixel_count; ++pixel) {
        const qsizetype x = pixel % 8;
        glyph_pixels[pixel * 4 + (x < 4 ? 0 : 1)] = static_cast<char>(255);
        glyph_pixels[pixel * 4 + 3] = static_cast<char>(255);
    }
    const QVariantMap glyph_frame{
        {QStringLiteral("valid"), true},
        {QStringLiteral("rasterPxPerPt"), 96.0 / 72.0},
        {QStringLiteral("pageIndex"), 0},
        {QStringLiteral("images"),
         QVariantList{QVariantMap{{QStringLiteral("width"), 8},
                                  {QStringLiteral("height"), 8},
                                  {QStringLiteral("left"), 0},
                                  {QStringLiteral("top"), 8},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("pixels"), glyph_pixels}},
                      QVariantMap{{QStringLiteral("width"), 0},
                                  {QStringLiteral("height"), 1},
                                  {QStringLiteral("left"), 0},
                                  {QStringLiteral("top"), 0},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("pixels"), QByteArray{}}}}},
        {QStringLiteral("placements"),
         QVariantList{QVariantMap{{QStringLiteral("imageIndex"), 0},
                                  {QStringLiteral("xPt"), 20.0},
                                  {QStringLiteral("baselineYPt"), 32.0},
                                  {QStringLiteral("pixelPhaseX"), 0.0},
                                  {QStringLiteral("pixelPhaseY"), 0.0},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("clipEnabled"), true},
                                  {QStringLiteral("clipY0Pt"), 28.0},
                                  {QStringLiteral("clipY1Pt"), 31.0}},
                      QVariantMap{{QStringLiteral("imageIndex"), 1},
                                  {QStringLiteral("xPt"), 30.0},
                                  {QStringLiteral("baselineYPt"), 32.0},
                                  {QStringLiteral("pixelPhaseX"), 0.0},
                                  {QStringLiteral("pixelPhaseY"), 0.0},
                                  {QStringLiteral("pixelPhaseApplied"), false},
                                  {QStringLiteral("clipEnabled"), false}}}},
    };
    QVariantMap second_glyph_frame = glyph_frame;
    second_glyph_frame.insert(QStringLiteral("pageIndex"), 1);
    QVERIFY(
        rendered_canvas->setProperty("glyphFrames", QVariantList{glyph_frame, second_glyph_frame}));
    QQuickWindow render_window;
    render_window.setColor(Qt::green);
    render_window.resize(200, 200);
    rendered_canvas->setParent(&render_window);
    rendered_canvas->setParentItem(render_window.contentItem());
    render_window.show();
    QTRY_VERIFY_WITH_TIMEOUT(render_window.isExposed(), 3000);
    rendered_canvas->forceActiveFocus(Qt::OtherFocusReason);
    QTRY_VERIFY(rendered_canvas->hasActiveFocus());
    const auto count_color_pixels = [](const QImage& image, const QColor& color) {
        qsizetype count = 0;
        for (qsizetype y = 0; y < image.height(); ++y) {
            for (qsizetype x = 0; x < image.width(); ++x) {
                count += image.pixelColor(static_cast<int>(x), static_cast<int>(y)) == color;
            }
        }
        return count;
    };
    const QColor environment_highlight_color(QStringLiteral("#fff2cc"));
    QCOMPARE(rendered_canvas->property("environmentHighlightRects").toList().size(), 2);
    QCOMPARE(rendered_canvas->property("environmentHighlightColor").value<QColor>(),
             environment_highlight_color);
    QCOMPARE(rendered_canvas->property("environmentHighlightRadius").toReal(), 4.0);
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), environment_highlight_color) > 0, 3000);
    const QImage highlight_frame = render_window.grabWindow();
    QVERIFY(!highlight_frame.isNull());
    QVERIFY(count_color_pixels(highlight_frame, environment_highlight_color) > 0);
    QVERIFY(rendered_canvas->setProperty("visiblePageRange", QVariantList{1, 2}));
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), environment_highlight_color) > 0, 3000);
    QVERIFY(rendered_canvas->setProperty("visiblePageRange", QVariantList{0, 2}));
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), environment_highlight_color) > 0, 3000);
    const auto pixel_at_logical_point = [&render_window](const QImage& image, qreal x, qreal y) {
        const qreal scale_x = static_cast<qreal>(image.width()) / render_window.width();
        const qreal scale_y = static_cast<qreal>(image.height()) / render_window.height();
        return image.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    const QPointF highlight_origin = rendered_canvas->documentToContent(0, QPointF(10.0, 10.0));
    const QPointF highlight_center = rendered_canvas->documentToContent(0, QPointF(30.0, 15.0));
    const QPointF next_page_highlight_center =
        rendered_canvas->documentToContent(1, QPointF(30.0, 15.0));
    QCOMPARE(pixel_at_logical_point(highlight_frame, highlight_center.x(), highlight_center.y()),
             environment_highlight_color);
    QCOMPARE(pixel_at_logical_point(highlight_frame, next_page_highlight_center.x(),
                                    next_page_highlight_center.y()),
             environment_highlight_color);
    const QColor rounded_highlight_edge = pixel_at_logical_point(
        highlight_frame, highlight_origin.x() + 1.0, highlight_origin.y() + 1.0);
    QVERIFY(rounded_highlight_edge.red() >= environment_highlight_color.red());
    QVERIFY(rounded_highlight_edge.green() >= environment_highlight_color.green());
    QVERIFY(rounded_highlight_edge.blue() >= environment_highlight_color.blue());
    QVERIFY(rounded_highlight_edge != environment_highlight_color);
    const QColor rounded_highlight_inner_edge = pixel_at_logical_point(
        highlight_frame, highlight_origin.x() + 4.0, highlight_origin.y() + 4.0);
    QVERIFY(rounded_highlight_inner_edge.red() >= environment_highlight_color.red());
    QVERIFY(rounded_highlight_inner_edge.green() >= environment_highlight_color.green());
    QVERIFY(rounded_highlight_inner_edge.blue() >= environment_highlight_color.blue());
    QVERIFY(rounded_highlight_inner_edge != environment_highlight_color);
    QVERIFY(rendered_canvas->setProperty("visiblePageRange", QVariantList{1, 2}));
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), environment_highlight_color) > 0, 3000);
    const QImage second_page_frame = render_window.grabWindow();
    QCOMPARE(pixel_at_logical_point(second_page_frame, next_page_highlight_center.x(),
                                    next_page_highlight_center.y()),
             environment_highlight_color);
    QVERIFY(pixel_at_logical_point(second_page_frame, highlight_center.x(), highlight_center.y()) !=
            environment_highlight_color);
    QVERIFY(rendered_canvas->setProperty("visiblePageRange", QVariantList{0, 1}));
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), environment_highlight_color) > 0, 3000);
    const QImage first_page_frame = render_window.grabWindow();
    QCOMPARE(pixel_at_logical_point(first_page_frame, highlight_center.x(), highlight_center.y()),
             environment_highlight_color);
    QVERIFY(pixel_at_logical_point(first_page_frame, next_page_highlight_center.x(),
                                   next_page_highlight_center.y()) != environment_highlight_color);
    QVERIFY(rendered_canvas->setProperty("visiblePageRange", QVariantList{0, 2}));
    QVERIFY(rendered_canvas->setProperty("environmentHighlightRects", QVariantList{}));
    QTRY_VERIFY_WITH_TIMEOUT(
        count_color_pixels(render_window.grabWindow(), environment_highlight_color) == 0, 3000);
    const QImage frame = render_window.grabWindow();
    QVERIFY(!frame.isNull());
    QCOMPARE(pixel_at_logical_point(frame, 60.5, 50.0), QColor(QStringLiteral("#c8c8c8")));
    QCOMPARE(pixel_at_logical_point(frame, 90.0, 50.0), QColor(Qt::white));
    QCOMPARE(pixel_at_logical_point(frame, 88.5, 55.0), QColor(Qt::red));
    QCOMPARE(pixel_at_logical_point(frame, 92.5, 55.0), QColor(Qt::green));
    QCOMPARE(pixel_at_logical_point(frame, 92.0, 51.0), QColor(Qt::white));
    QVERIFY(pixel_at_logical_point(frame, 101.0, 50.0) != QColor(Qt::white));
    QCOMPARE(pixel_at_logical_point(frame, 102.5, 50.0), QColor(Qt::white));
    QCOMPARE(pixel_at_logical_point(frame, 100.5, 95.5), QColor(QStringLiteral("#ff0000")));
    QCOMPARE(pixel_at_logical_point(frame, 101.0, 180.0), QColor(Qt::green));
    QCOMPARE(pixel_at_logical_point(frame, 88.5, 134.5), QColor(Qt::red));

    const auto current_caret_pixel = [&render_window]() {
        const QImage current_frame = render_window.grabWindow();
        return current_frame.pixelColor(
            qFloor(100.5 * current_frame.width() / render_window.width()),
            qFloor(50.0 * current_frame.height() / render_window.height()));
    };
    const int cursor_flash_time = QGuiApplication::styleHints()->cursorFlashTime();
    if (cursor_flash_time > 0) {
        QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                                 cursor_flash_time + 500);
        QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                                  cursor_flash_time + 500);
    }

    QStyleHints* style_hints = QGuiApplication::styleHints();
    const int original_cursor_flash_time = style_hints->cursorFlashTime();
    [[maybe_unused]] const auto restore_cursor_flash_time =
        qScopeGuard([style_hints, original_cursor_flash_time]() {
            style_hints->setCursorFlashTime(original_cursor_flash_time);
        });
    style_hints->setCursorFlashTime(kTestCursorFlashPeriodMs);
    QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                             kTestBlinkTransitionTimeoutMs);
    QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                              kTestBlinkTransitionTimeoutMs);
    rendered_canvas->setProperty("caretBlinkEnabled", false);
    QTRY_COMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    QTest::qWait(kDisabledBlinkObservationMs);
    QCOMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    rendered_canvas->setProperty("caretBlinkEnabled", true);
    QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                             kTestBlinkTransitionTimeoutMs);
    QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                              kTestBlinkTransitionTimeoutMs);
    style_hints->setCursorFlashTime(0);
    QTRY_COMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    QTest::qWait(kDisabledBlinkObservationMs);
    QCOMPARE(current_caret_pixel(), QColor(QStringLiteral("#ff0000")));
    style_hints->setCursorFlashTime(kTestCursorFlashPeriodMs);
    QTRY_VERIFY_WITH_TIMEOUT(current_caret_pixel() != QColor(QStringLiteral("#ff0000")),
                             kTestBlinkTransitionTimeoutMs);
    QTRY_COMPARE_WITH_TIMEOUT(current_caret_pixel(), QColor(QStringLiteral("#ff0000")),
                              kTestBlinkTransitionTimeoutMs);

    rendered_canvas->setFocus(false);
    QTRY_VERIFY(!rendered_canvas->hasActiveFocus());
    const QImage unfocused_frame = render_window.grabWindow();
    QVERIFY(!unfocused_frame.isNull());
    QCOMPARE(unfocused_frame.pixelColor(qFloor(101.0 * frame.width() / render_window.width()),
                                        qFloor(50.0 * frame.height() / render_window.height())),
             QColor(Qt::white));

    QByteArray changed_glyph_pixels = glyph_pixels;
    for (qsizetype pixel = 0; pixel < glyph_pixel_count; ++pixel) {
        const qsizetype x = pixel % 8;
        if (x < 4) {
            changed_glyph_pixels[pixel * 4 + 0] = '\0';
            changed_glyph_pixels[pixel * 4 + 2] = static_cast<char>(255);
        }
    }
    QVariantMap changed_second_image =
        second_glyph_frame.value(QStringLiteral("images")).toList().front().toMap();
    changed_second_image.insert(QStringLiteral("pixels"), changed_glyph_pixels);
    QVariantMap changed_second_frame = second_glyph_frame;
    const QVariantList changed_images{
        changed_second_image,
        second_glyph_frame.value(QStringLiteral("images")).toList().at(1),
    };
    changed_second_frame.insert(QStringLiteral("images"), changed_images);
    const QVariantList partially_changed_frames{glyph_frame, changed_second_frame};
    QVERIFY(rendered_canvas->setProperty("glyphFrames", partially_changed_frames));
    QCOMPARE(rendered_canvas->property("glyphFrames").toList(), partially_changed_frames);
    QVariantMap fractional_width_image = changed_second_image;
    fractional_width_image.insert(QStringLiteral("width"), 8.5);
    QVariantMap malformed_second_frame = changed_second_frame;
    QVariantList malformed_images = changed_images;
    malformed_images[0] = fractional_width_image;
    malformed_second_frame.insert(QStringLiteral("images"), malformed_images);
    QVERIFY(rendered_canvas->setProperty("glyphFrames",
                                         QVariantList{glyph_frame, malformed_second_frame}));
    QCOMPARE(rendered_canvas->property("glyphFrames").toList(), partially_changed_frames);
    QImage partial_update_frame;
    const auto partial_update_pixel = [&partial_update_frame, &render_window](qreal x, qreal y) {
        const qreal scale_x =
            static_cast<qreal>(partial_update_frame.width()) / render_window.width();
        const qreal scale_y =
            static_cast<qreal>(partial_update_frame.height()) / render_window.height();
        return partial_update_frame.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    QTRY_VERIFY_WITH_TIMEOUT(([&]() {
                                 partial_update_frame = render_window.grabWindow();
                                 return !partial_update_frame.isNull() &&
                                        partial_update_pixel(88.5, 134.5) == QColor(Qt::blue);
                             }()),
                             3000);
    QCOMPARE(partial_update_pixel(88.5, 55.0), QColor(Qt::red));
    QCOMPARE(partial_update_pixel(88.5, 134.5), QColor(Qt::blue));

    rendered_canvas->setWidth(240);
    QTRY_COMPARE_WITH_TIMEOUT(rendered_canvas->width(), 240.0, 3000);
    const QImage recentered_frame = render_window.grabWindow();
    QVERIFY(!recentered_frame.isNull());
    const auto recentered_pixel = [&recentered_frame, &render_window](qreal x, qreal y) {
        const qreal scale_x = static_cast<qreal>(recentered_frame.width()) / render_window.width();
        const qreal scale_y =
            static_cast<qreal>(recentered_frame.height()) / render_window.height();
        return recentered_frame.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    QCOMPARE(recentered_pixel(88.5, 55.0), QColor(Qt::white));
    QCOMPARE(recentered_pixel(108.5, 55.0), QColor(Qt::red));
    QCOMPARE(recentered_pixel(88.5, 134.5), QColor(Qt::white));
    QCOMPARE(recentered_pixel(108.5, 134.5), QColor(Qt::blue));

    rendered_canvas->setWidth(300);
    rendered_canvas->setProperty("zoom", 2.0);
    render_window.resize(300, 200);
    QTRY_COMPARE_WITH_TIMEOUT(rendered_canvas->width(), 300.0, 3000);
    const QImage zoomed_frame = render_window.grabWindow();
    QVERIFY(!zoomed_frame.isNull());
    const auto zoomed_pixel = [&zoomed_frame, &render_window](qreal x, qreal y) {
        const qreal scale_x = static_cast<qreal>(zoomed_frame.width()) / render_window.width();
        const qreal scale_y = static_cast<qreal>(zoomed_frame.height()) / render_window.height();
        return zoomed_frame.pixelColor(qFloor(x * scale_x), qFloor(y * scale_y));
    };
    QCOMPARE(zoomed_pixel(126.0, 109.0), QColor(Qt::red));
    QCOMPARE(zoomed_pixel(134.0, 109.0), QColor(Qt::green));
    QObject* scroller = surface->findChild<QObject*>(QStringLiteral("documentScroller"));
    QVERIFY(scroller != nullptr);
    bound_canvas->setZoom(2.0);
    QCoreApplication::processEvents();
    const qreal previous_max_y =
        scroller->property("contentHeight").toReal() - scroller->property("height").toReal();
    QVERIFY(previous_max_y > 0.0);
    scroller->setProperty("contentY", previous_max_y);
    QVERIFY(QMetaObject::invokeMethod(surface, "setZoomAtViewportCenter",
                                      Q_ARG(QVariant, QVariant(0.5))));
    QCoreApplication::processEvents();
    const qreal new_max_y =
        scroller->property("contentHeight").toReal() - scroller->property("height").toReal();
    QVERIFY(scroller->property("contentY").toReal() <= std::max(0.0, new_max_y));
    delete surface;
    delete view_model;
    delete root;
}
