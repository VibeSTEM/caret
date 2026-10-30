// 将 Rust glyph 栅格投影为 Qt 二进制图像与绘制片段。
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
    QVariantList decorations;
    decorations.reserve(static_cast<qsizetype>(frame.decorations.size()));
    for (const auto& decoration : frame.decorations) {
        if (!std::isfinite(decoration.x_pt) || !std::isfinite(decoration.y_pt) ||
            !std::isfinite(decoration.width_pt) || !std::isfinite(decoration.height_pt) ||
            decoration.width_pt <= 0.0 || decoration.height_pt <= 0.0 ||
            (decoration.clip_enabled &&
             (!std::isfinite(decoration.clip_y0_pt) || !std::isfinite(decoration.clip_y1_pt) ||
              decoration.clip_y0_pt < 0.0 || decoration.clip_y1_pt <= decoration.clip_y0_pt))) {
            return {{QStringLiteral("valid"), false},
                    {QStringLiteral("error"), QStringLiteral("layout.glyphs_invalid_decoration")}};
        }
        decorations.push_back(QVariantMap{
            {QStringLiteral("xPt"), decoration.x_pt},
            {QStringLiteral("yPt"), decoration.y_pt},
            {QStringLiteral("widthPt"), decoration.width_pt},
            {QStringLiteral("heightPt"), decoration.height_pt},
            {QStringLiteral("colorRed"), decoration.color_red},
            {QStringLiteral("colorGreen"), decoration.color_green},
            {QStringLiteral("colorBlue"), decoration.color_blue},
            {QStringLiteral("colorAlpha"), decoration.color_alpha},
            {QStringLiteral("clipEnabled"), decoration.clip_enabled},
            {QStringLiteral("clipY0Pt"), decoration.clip_y0_pt},
            {QStringLiteral("clipY1Pt"), decoration.clip_y1_pt},
        });
    }
    return {{QStringLiteral("valid"), true},
            {QStringLiteral("documentGeneration"), QString::number(frame.document_generation)},
            {QStringLiteral("fontGeneration"), QString::number(frame.font_generation)},
            {QStringLiteral("rasterPxPerPt"), px_per_pt},
            {QStringLiteral("pageIndex"), frame.page_index},
            {QStringLiteral("images"), images},
            {QStringLiteral("placements"), placements},
            {QStringLiteral("decorations"), decorations}};
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
