// 将 Rust glyph 栅格投影为 Qt 二进制图像与绘制片段。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#endif

#include <QByteArray>
#include <QCoreApplication>
#include <QString>
#include <QThread>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <qcontainerfwd.h>

namespace caret {
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
    auto* runtime = document_runtime();
    if (runtime == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }

    const ffi::GlyphRasterRequest request{
        .page_index = static_cast<std::uint32_t>(page_index),
        .px_per_pt = static_cast<double>(px_per_pt),
        .page_origin_device_x_px = static_cast<double>(page_origin_device_x_px),
        .page_origin_device_y_px = static_cast<double>(page_origin_device_y_px),
        .foreground_red = static_cast<std::uint8_t>(foreground_red),
        .foreground_green = static_cast<std::uint8_t>(foreground_green),
        .foreground_blue = static_cast<std::uint8_t>(foreground_blue),
        .foreground_alpha = static_cast<std::uint8_t>(foreground_alpha),
    };
    const auto frame = ffi::rasterize_page_glyphs(*runtime, request);
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

    QVariantList commands;
    commands.reserve(static_cast<qsizetype>(frame.commands.size()));
    for (const auto& command : frame.commands) {
        if (command.kind > 2 || !std::isfinite(command.x_pt) || !std::isfinite(command.y_pt) ||
            !std::isfinite(command.width_pt) || !std::isfinite(command.height_pt) ||
            !std::isfinite(command.x2_pt) || !std::isfinite(command.y2_pt) ||
            !std::isfinite(command.thickness_pt) || !std::isfinite(command.pixel_phase_x) ||
            !std::isfinite(command.pixel_phase_y) ||
            (command.kind == 0 &&
             (command.image_index >= frame.images.size() || command.pixel_phase_x < 0.0F ||
              command.pixel_phase_x >= 1.0F || command.pixel_phase_y < 0.0F ||
              command.pixel_phase_y >= 1.0F)) ||
            (command.kind == 1 && (command.width_pt <= 0.0 || command.height_pt <= 0.0)) ||
            (command.kind == 2 && command.thickness_pt < 0.0) ||
            (command.clip_enabled &&
             (!std::isfinite(command.clip_y0_pt) || !std::isfinite(command.clip_y1_pt) ||
              command.clip_y0_pt < 0.0 || command.clip_y1_pt <= command.clip_y0_pt))) {
            return {{QStringLiteral("valid"), false},
                    {QStringLiteral("error"), QStringLiteral("layout.glyphs_invalid_command")}};
        }
        commands.push_back(QVariantMap{
            {QStringLiteral("kind"), command.kind},
            {QStringLiteral("imageIndex"), command.image_index},
            {QStringLiteral("xPt"), command.x_pt},
            {QStringLiteral("yPt"), command.y_pt},
            {QStringLiteral("widthPt"), command.width_pt},
            {QStringLiteral("heightPt"), command.height_pt},
            {QStringLiteral("x2Pt"), command.x2_pt},
            {QStringLiteral("y2Pt"), command.y2_pt},
            {QStringLiteral("thicknessPt"), command.thickness_pt},
            {QStringLiteral("pixelPhaseX"), command.pixel_phase_x},
            {QStringLiteral("pixelPhaseY"), command.pixel_phase_y},
            {QStringLiteral("pixelPhaseApplied"),
             command.kind == 0 ? frame.images[command.image_index].pixel_phase_applied : false},
            {QStringLiteral("colorRed"), command.color_red},
            {QStringLiteral("colorGreen"), command.color_green},
            {QStringLiteral("colorBlue"), command.color_blue},
            {QStringLiteral("colorAlpha"), command.color_alpha},
            {QStringLiteral("clipEnabled"), command.clip_enabled},
            {QStringLiteral("clipY0Pt"), command.clip_y0_pt},
            {QStringLiteral("clipY1Pt"), command.clip_y1_pt},
        });
    }
    return {{QStringLiteral("valid"), true},
            {QStringLiteral("documentGeneration"), QString::number(frame.document_generation)},
            {QStringLiteral("fontGeneration"), QString::number(frame.font_generation)},
            {QStringLiteral("sessionId"),
             QString::fromUtf8(frame.session_id.data(),
                               static_cast<qsizetype>(frame.session_id.size()))},
            {QStringLiteral("rasterPxPerPt"), px_per_pt},
            {QStringLiteral("pageIndex"), frame.page_index},
            {QStringLiteral("images"), images},
            {QStringLiteral("commands"), commands}};
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
