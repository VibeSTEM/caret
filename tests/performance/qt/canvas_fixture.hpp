// 提供 Qt 画布性能基准共用的页面、字形与选区场景数据。
#pragma once

#include "document_canvas.hpp"
#include <QByteArray>
#include <QColor>
#include <QSize>
#include <QSizeF>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>

namespace caret::performance {
inline constexpr QSize render_size{1280, 960};
inline constexpr QSizeF page_size_points{612.0, 792.0};
inline constexpr int unique_glyph_images = 96;
inline constexpr int glyph_placements = 1000;
inline constexpr int selection_rectangles_count = 12;
inline constexpr int warmup_frames = 8;
inline constexpr int default_samples = 31;

inline QByteArray image_pixels(int image_index) {
    constexpr int side = 16;
    QByteArray pixels(side * side * 4, '\0');
    for (int y = 2; y < side - 2; ++y) {
        for (int x = 2; x < side - 2; ++x) {
            const qsizetype offset = (static_cast<qsizetype>(y) * side + x) * 4;
            pixels[offset] = static_cast<char>((image_index * 17 + x * 3) & 0xff);
            pixels[offset + 1] = static_cast<char>((image_index * 29 + y * 5) & 0xff);
            pixels[offset + 2] = static_cast<char>((image_index * 7 + x + y) & 0xff);
            pixels[offset + 3] = static_cast<char>(255);
        }
    }
    return pixels;
}

inline QVariantMap glyph_frame() {
    QVariantList images;
    images.reserve(unique_glyph_images);
    for (int index = 0; index < unique_glyph_images; ++index) {
        images.push_back(QVariantMap{
            {QStringLiteral("width"), 16},
            {QStringLiteral("height"), 16},
            {QStringLiteral("left"), 0},
            {QStringLiteral("top"), 14},
            {QStringLiteral("pixelPhaseApplied"), false},
            {QStringLiteral("pixels"), image_pixels(index)},
        });
    }

    QVariantList placements;
    placements.reserve(glyph_placements);
    for (int index = 0; index < glyph_placements; ++index) {
        const int column = index % 90;
        const int row = index / 90;
        placements.push_back(QVariantMap{
            {QStringLiteral("imageIndex"), index % unique_glyph_images},
            {QStringLiteral("xPt"), 24.0 + static_cast<double>(column) * 6.0},
            {QStringLiteral("baselineYPt"), 30.0 + static_cast<double>(row) * 24.0},
            {QStringLiteral("pixelPhaseX"), 0.0},
            {QStringLiteral("pixelPhaseY"), 0.0},
            {QStringLiteral("pixelPhaseApplied"), false},
            {QStringLiteral("clipEnabled"), false},
            {QStringLiteral("clipY0Pt"), 0.0},
            {QStringLiteral("clipY1Pt"), 0.0},
        });
    }
    return {{QStringLiteral("valid"), true},
            {QStringLiteral("documentGeneration"), QStringLiteral("1")},
            {QStringLiteral("fontGeneration"), QStringLiteral("1")},
            {QStringLiteral("rasterPxPerPt"), 96.0 / 72.0},
            {QStringLiteral("pageIndex"), 0},
            {QStringLiteral("images"), images},
            {QStringLiteral("placements"), placements}};
}

inline QVariantList selection_rectangles(int frame) {
    QVariantList rectangles;
    rectangles.reserve(selection_rectangles_count);
    const double offset = static_cast<double>(frame % 8);
    for (int index = 0; index < selection_rectangles_count; ++index) {
        const double y = 40.0 + static_cast<double>(index) * 28.0;
        const double x = 24.0 + offset;
        rectangles.push_back(QVariantMap{
            {QStringLiteral("pageIndex"), 0},
            {QStringLiteral("x0Pt"), x},
            {QStringLiteral("y0Pt"), y},
            {QStringLiteral("x1Pt"), x + 108.0},
            {QStringLiteral("y1Pt"), y + 16.0},
        });
    }
    return rectangles;
}

inline void set_up_canvas(caret::DocumentCanvas* canvas) {
    canvas->setPageSizesPt({QVariant::fromValue(page_size_points)});
    canvas->setPageMarginsPt({{QStringLiteral("top"), 0.0},
                              {QStringLiteral("right"), 0.0},
                              {QStringLiteral("bottom"), 0.0},
                              {QStringLiteral("left"), 0.0}});
    canvas->setPageGapPt(0.0);
    canvas->setPageMarginPt(0.0);
    canvas->setWidth(render_size.width());
    canvas->setHeight(render_size.height());
    canvas->setPageColor(Qt::white);
    canvas->setPageBorderColor(QColor(QStringLiteral("#d0d0d0")));
    canvas->setSelectionColor(QColor(QStringLiteral("#badcf5")));
    canvas->setCaretBlinkEnabled(false);
    canvas->setGlyphFrames({glyph_frame()});
    canvas->setSelectionRects(selection_rectangles(0));
}
} // namespace caret::performance
