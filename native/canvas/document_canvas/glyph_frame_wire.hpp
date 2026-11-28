// 解析并严格校验 glyph frame 的 QVariant 线格式，供画布安装与场景图渲染共用。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#pragma once

#include <QByteArray>
#include <QVariantMap>
#include <QVector>
#include <QtGlobal>
#include <optional>

namespace caret::glyph_frame_wire {

/// Rust 字形栅格图像；像素为 RGBA8888 预乘字节。
struct GlyphImage {
    int width = 0;
    int height = 0;
    qint32 left = 0;
    qint32 top = 0;
    bool pixel_phase_applied = false;
    QByteArray pixels;
};

/// 单个字形放置；`image_index` 保证落在所属帧的 images 内。
struct GlyphPlacement {
    quint64 paint_order = 0;
    qsizetype image_index = 0;
    qreal x_pt = 0.0;
    qreal baseline_y_pt = 0.0;
    qreal pixel_phase_x = 0.0;
    qreal pixel_phase_y = 0.0;
    bool pixel_phase_applied = false;
    bool clip_enabled = false;
    qreal clip_y0_pt = 0.0;
    qreal clip_y1_pt = 0.0;
    bool clip_x_enabled = false;
    qreal clip_x0_pt = 0.0;
    qreal clip_x1_pt = 0.0;
};

/// 下划线等装饰矩形。
struct GlyphDecoration {
    quint64 paint_order = 0;
    qreal x_pt = 0.0;
    qreal y_pt = 0.0;
    qreal width_pt = 0.0;
    qreal height_pt = 0.0;
    int color_red = 0;
    int color_green = 0;
    int color_blue = 0;
    int color_alpha = 0;
    bool clip_enabled = false;
    qreal clip_y0_pt = 0.0;
    qreal clip_y1_pt = 0.0;
    bool clip_x_enabled = false;
    qreal clip_x0_pt = 0.0;
    qreal clip_x1_pt = 0.0;
};

/// 线段（kind 0）或矩形（kind 1）绘制项。
struct GlyphShape {
    quint64 paint_order = 0;
    int kind = 0;
    qreal x_pt = 0.0;
    qreal y_pt = 0.0;
    qreal dx_pt = 0.0;
    qreal dy_pt = 0.0;
    qreal width_pt = 0.0;
    qreal height_pt = 0.0;
    bool fill_enabled = false;
    int fill_red = 0;
    int fill_green = 0;
    int fill_blue = 0;
    int fill_alpha = 0;
    bool stroke_enabled = false;
    int stroke_red = 0;
    int stroke_green = 0;
    int stroke_blue = 0;
    int stroke_alpha = 0;
    qreal stroke_width_pt = 0.0;
    bool clip_enabled = false;
    qreal clip_y0_pt = 0.0;
    qreal clip_y1_pt = 0.0;
    bool clip_x_enabled = false;
    qreal clip_x0_pt = 0.0;
    qreal clip_x1_pt = 0.0;
};

/// 一帧已校验的字形数据；字段与服务端 Rust `PageGlyphFrame` 线格式一一对应。
struct GlyphFrame {
    int page_index = 0;
    qreal raster_px_per_pt = 0.0;
    QVector<GlyphImage> images;
    QVector<GlyphPlacement> placements;
    QVector<GlyphDecoration> decorations;
    QVector<GlyphShape> shapes;
};

/// 严格解析一帧；任一字段类型、有限性或范围非法返回 `std::nullopt`。
/// `DocumentCanvas::setGlyphFrames` 用它整列拒绝非法输入，场景图用它读取已校验字段。
[[nodiscard]] std::optional<GlyphFrame> parse(const QVariantMap& frame);

} // namespace caret::glyph_frame_wire
