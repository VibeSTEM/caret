// 实现 glyph frame QVariant 线格式的单一解析与校验。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "glyph_frame_wire.hpp"

#include "value_validation.hpp"
#include <QList>
#include <QMetaType>
#include <QString>
#include <QVariant>
#include <QVariantList>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <optional>
#include <qcontainerfwd.h>

namespace caret::glyph_frame_wire {
namespace {

using canvas_detail::is_boolean_variant;
using canvas_detail::is_finite_positive;

constexpr double kMaxExactInteger = 9007199254740991.0;

/// 线格式允许省略 paintOrder；省略时按 0 处理，与场景图原有取值一致。
bool parse_paint_order(const QVariant& value, quint64* order) {
    if (!value.isValid()) {
        *order = 0;
        return true;
    }
    switch (value.metaType().id()) {
    case QMetaType::Char:
    case QMetaType::SChar:
    case QMetaType::Short:
    case QMetaType::Int:
    case QMetaType::Long:
    case QMetaType::LongLong: {
        bool converted = false;
        const qlonglong signed_order = value.toLongLong(&converted);
        if (!converted || signed_order < 0) {
            return false;
        }
        *order = static_cast<quint64>(signed_order);
        return true;
    }
    case QMetaType::UChar:
    case QMetaType::UShort:
    case QMetaType::UInt:
    case QMetaType::ULong:
    case QMetaType::ULongLong: {
        bool converted = false;
        const qulonglong unsigned_order = value.toULongLong(&converted);
        if (!converted) {
            return false;
        }
        *order = static_cast<quint64>(unsigned_order);
        return true;
    }
    case QMetaType::Float:
    case QMetaType::Double: {
        bool converted = false;
        const double order_value = value.toDouble(&converted);
        if (!converted || !std::isfinite(order_value) || order_value < 0.0 ||
            order_value > kMaxExactInteger || std::floor(order_value) != order_value) {
            return false;
        }
        *order = static_cast<quint64>(order_value);
        return true;
    }
    default:
        return false;
    }
}

bool parse_exact_integer(const QVariant& value, int minimum, int maximum, int* result) {
    switch (value.metaType().id()) {
    case QMetaType::Char:
    case QMetaType::SChar:
    case QMetaType::UChar:
    case QMetaType::Short:
    case QMetaType::UShort:
    case QMetaType::Int:
    case QMetaType::UInt:
    case QMetaType::Long:
    case QMetaType::ULong:
    case QMetaType::LongLong:
    case QMetaType::ULongLong:
    case QMetaType::Float:
    case QMetaType::Double:
        break;
    default:
        return false;
    }
    bool converted = false;
    const double number = value.toDouble(&converted);
    if (!converted || !std::isfinite(number) || std::floor(number) != number || number < minimum ||
        number > maximum) {
        return false;
    }
    *result = static_cast<int>(number);
    return true;
}

bool parse_color_channel(const QVariantMap& item, const QString& key, int* channel) {
    return parse_exact_integer(item.value(key), 0, 255, channel);
}

struct ClipFields {
    bool enabled = false;
    qreal y0_pt = 0.0;
    qreal y1_pt = 0.0;
};

/// clipEnabled 必须是布尔；启用时 clipY0Pt/clipY1Pt 必须是有限且 y0 >= 0、y1 > y0 的数值。
std::optional<ClipFields> parse_clip_fields(const QVariantMap& item) {
    const QVariant enabled_value = item.value(QStringLiteral("clipEnabled"));
    if (!is_boolean_variant(enabled_value)) {
        return std::nullopt;
    }
    bool clip_y0_valid = false;
    bool clip_y1_valid = false;
    const double clip_y0 = item.value(QStringLiteral("clipY0Pt")).toDouble(&clip_y0_valid);
    const double clip_y1 = item.value(QStringLiteral("clipY1Pt")).toDouble(&clip_y1_valid);
    ClipFields fields;
    fields.enabled = enabled_value.toBool();
    fields.y0_pt = clip_y0;
    fields.y1_pt = clip_y1;
    if (fields.enabled && (!clip_y0_valid || !clip_y1_valid || !std::isfinite(clip_y0) ||
                           !std::isfinite(clip_y1) || clip_y0 < 0.0 || clip_y1 <= clip_y0)) {
        return std::nullopt;
    }
    return fields;
}

std::optional<GlyphImage> parse_image(const QVariant& value) {
    const QVariantMap image = value.toMap();
    bool width_converted = false;
    bool height_converted = false;
    bool left_valid = false;
    bool top_valid = false;
    const double width_value = image.value(QStringLiteral("width")).toDouble(&width_converted);
    const double height_value = image.value(QStringLiteral("height")).toDouble(&height_converted);
    const double left = image.value(QStringLiteral("left")).toDouble(&left_valid);
    const double top = image.value(QStringLiteral("top")).toDouble(&top_valid);
    const QVariant phase_applied_value = image.value(QStringLiteral("pixelPhaseApplied"));
    const QVariant pixels_value = image.value(QStringLiteral("pixels"));
    const QByteArray pixels = pixels_value.toByteArray();
    if (!width_converted || !height_converted || !std::isfinite(width_value) ||
        !std::isfinite(height_value) || width_value < 0.0 || height_value < 0.0 ||
        width_value > std::numeric_limits<int>::max() ||
        height_value > std::numeric_limits<int>::max() || std::floor(width_value) != width_value ||
        std::floor(height_value) != height_value) {
        return std::nullopt;
    }
    const int width = static_cast<int>(width_value);
    const int height = static_cast<int>(height_value);
    const quint64 byte_count =
        static_cast<quint64>(std::max(width, 0)) * static_cast<quint64>(std::max(height, 0)) * 4U;
    const bool empty_image = byte_count == 0 && pixels.isEmpty();
    if (width > std::numeric_limits<int>::max() / 4 ||
        (!empty_image && byte_count != static_cast<quint64>(pixels.size())) || !left_valid ||
        !top_valid || !std::isfinite(left) || !std::isfinite(top) || std::floor(left) != left ||
        std::floor(top) != top || left < std::numeric_limits<std::int32_t>::min() ||
        left > std::numeric_limits<std::int32_t>::max() ||
        top < std::numeric_limits<std::int32_t>::min() ||
        top > std::numeric_limits<std::int32_t>::max() ||
        !is_boolean_variant(phase_applied_value) ||
        pixels_value.metaType().id() != QMetaType::QByteArray) {
        return std::nullopt;
    }
    GlyphImage parsed;
    parsed.width = width;
    parsed.height = height;
    parsed.left = static_cast<qint32>(left);
    parsed.top = static_cast<qint32>(top);
    parsed.pixel_phase_applied = phase_applied_value.toBool();
    parsed.pixels = pixels;
    return parsed;
}

std::optional<GlyphPlacement> parse_placement(const QVariantMap& placement,
                                              const QVector<GlyphImage>& images) {
    bool image_index_valid = false;
    const double image_index_value =
        placement.value(QStringLiteral("imageIndex")).toDouble(&image_index_valid);
    bool x_valid = false;
    bool baseline_valid = false;
    bool phase_x_valid = false;
    bool phase_y_valid = false;
    const double x_pt = placement.value(QStringLiteral("xPt")).toDouble(&x_valid);
    const double baseline_y_pt =
        placement.value(QStringLiteral("baselineYPt")).toDouble(&baseline_valid);
    const double pixel_phase_x =
        placement.value(QStringLiteral("pixelPhaseX")).toDouble(&phase_x_valid);
    const double pixel_phase_y =
        placement.value(QStringLiteral("pixelPhaseY")).toDouble(&phase_y_valid);
    const QVariant phase_applied_value = placement.value(QStringLiteral("pixelPhaseApplied"));
    const std::optional<ClipFields> clip_fields = parse_clip_fields(placement);
    quint64 paint_order = 0;
    if (!image_index_valid || !std::isfinite(image_index_value) || image_index_value < 0.0 ||
        image_index_value >= static_cast<double>(images.size()) ||
        std::floor(image_index_value) != image_index_value || !x_valid || !baseline_valid ||
        !phase_x_valid || !phase_y_valid || !std::isfinite(x_pt) || !std::isfinite(baseline_y_pt) ||
        !std::isfinite(pixel_phase_x) || !std::isfinite(pixel_phase_y) || pixel_phase_x < 0.0 ||
        pixel_phase_x >= 1.0 || pixel_phase_y < 0.0 || pixel_phase_y >= 1.0 ||
        !is_boolean_variant(phase_applied_value) || !clip_fields ||
        !parse_paint_order(placement.value(QStringLiteral("paintOrder")), &paint_order)) {
        return std::nullopt;
    }
    const qsizetype image_index = static_cast<qsizetype>(image_index_value);
    if (phase_applied_value.toBool() != images.at(image_index).pixel_phase_applied) {
        return std::nullopt;
    }
    GlyphPlacement parsed;
    parsed.paint_order = paint_order;
    parsed.image_index = image_index;
    parsed.x_pt = x_pt;
    parsed.baseline_y_pt = baseline_y_pt;
    parsed.pixel_phase_x = pixel_phase_x;
    parsed.pixel_phase_y = pixel_phase_y;
    parsed.pixel_phase_applied = phase_applied_value.toBool();
    parsed.clip_enabled = clip_fields->enabled;
    parsed.clip_y0_pt = clip_fields->y0_pt;
    parsed.clip_y1_pt = clip_fields->y1_pt;
    return parsed;
}

std::optional<GlyphDecoration> parse_decoration(const QVariant& value) {
    if (value.metaType().id() != QMetaType::QVariantMap) {
        return std::nullopt;
    }
    const QVariantMap decoration = value.toMap();
    bool x_valid = false;
    bool y_valid = false;
    bool width_valid = false;
    bool height_valid = false;
    const double x_pt = decoration.value(QStringLiteral("xPt")).toDouble(&x_valid);
    const double y_pt = decoration.value(QStringLiteral("yPt")).toDouble(&y_valid);
    const double width_pt = decoration.value(QStringLiteral("widthPt")).toDouble(&width_valid);
    const double height_pt = decoration.value(QStringLiteral("heightPt")).toDouble(&height_valid);
    const std::optional<ClipFields> clip_fields = parse_clip_fields(decoration);
    quint64 paint_order = 0;
    if (!x_valid || !y_valid || !width_valid || !height_valid || !std::isfinite(x_pt) ||
        !std::isfinite(y_pt) || !std::isfinite(width_pt) || !std::isfinite(height_pt) ||
        width_pt <= 0.0 || height_pt <= 0.0 || !clip_fields ||
        !parse_paint_order(decoration.value(QStringLiteral("paintOrder")), &paint_order)) {
        return std::nullopt;
    }
    GlyphDecoration parsed;
    parsed.paint_order = paint_order;
    parsed.x_pt = x_pt;
    parsed.y_pt = y_pt;
    parsed.width_pt = width_pt;
    parsed.height_pt = height_pt;
    parsed.clip_enabled = clip_fields->enabled;
    parsed.clip_y0_pt = clip_fields->y0_pt;
    parsed.clip_y1_pt = clip_fields->y1_pt;
    if (!parse_color_channel(decoration, QStringLiteral("colorRed"), &parsed.color_red) ||
        !parse_color_channel(decoration, QStringLiteral("colorGreen"), &parsed.color_green) ||
        !parse_color_channel(decoration, QStringLiteral("colorBlue"), &parsed.color_blue) ||
        !parse_color_channel(decoration, QStringLiteral("colorAlpha"), &parsed.color_alpha)) {
        return std::nullopt;
    }
    return parsed;
}

std::optional<GlyphShape> parse_shape(const QVariant& value) {
    if (value.metaType().id() != QMetaType::QVariantMap) {
        return std::nullopt;
    }
    const QVariantMap shape = value.toMap();
    int kind = 0;
    const bool kind_valid = parse_exact_integer(shape.value(QStringLiteral("kind")), 0, 1, &kind);
    bool x_valid = false;
    bool y_valid = false;
    bool dx_valid = false;
    bool dy_valid = false;
    bool width_valid = false;
    bool height_valid = false;
    bool stroke_width_valid = false;
    const double x_pt = shape.value(QStringLiteral("xPt")).toDouble(&x_valid);
    const double y_pt = shape.value(QStringLiteral("yPt")).toDouble(&y_valid);
    const double dx_pt = shape.value(QStringLiteral("dxPt")).toDouble(&dx_valid);
    const double dy_pt = shape.value(QStringLiteral("dyPt")).toDouble(&dy_valid);
    const double width_pt = shape.value(QStringLiteral("widthPt")).toDouble(&width_valid);
    const double height_pt = shape.value(QStringLiteral("heightPt")).toDouble(&height_valid);
    const double stroke_width_pt =
        shape.value(QStringLiteral("strokeWidthPt")).toDouble(&stroke_width_valid);
    const QVariant fill_enabled_value = shape.value(QStringLiteral("fillEnabled"));
    const QVariant stroke_enabled_value = shape.value(QStringLiteral("strokeEnabled"));
    const std::optional<ClipFields> clip_fields = parse_clip_fields(shape);
    quint64 paint_order = 0;
    if (!kind_valid || !x_valid || !y_valid || !dx_valid || !dy_valid || !width_valid ||
        !height_valid || !stroke_width_valid || !std::isfinite(x_pt) || !std::isfinite(y_pt) ||
        !std::isfinite(dx_pt) || !std::isfinite(dy_pt) || !std::isfinite(width_pt) ||
        !std::isfinite(height_pt) || !std::isfinite(stroke_width_pt) || width_pt < 0.0 ||
        height_pt < 0.0 || stroke_width_pt < 0.0 || !is_boolean_variant(fill_enabled_value) ||
        !is_boolean_variant(stroke_enabled_value) || !clip_fields ||
        (kind == 0 && (!stroke_enabled_value.toBool() || stroke_width_pt <= 0.0)) ||
        (kind == 1 && (width_pt <= 0.0 || height_pt <= 0.0)) ||
        !parse_paint_order(shape.value(QStringLiteral("paintOrder")), &paint_order)) {
        return std::nullopt;
    }
    GlyphShape parsed;
    parsed.paint_order = paint_order;
    parsed.kind = kind;
    parsed.x_pt = x_pt;
    parsed.y_pt = y_pt;
    parsed.dx_pt = dx_pt;
    parsed.dy_pt = dy_pt;
    parsed.width_pt = width_pt;
    parsed.height_pt = height_pt;
    parsed.fill_enabled = fill_enabled_value.toBool();
    parsed.stroke_enabled = stroke_enabled_value.toBool();
    parsed.stroke_width_pt = stroke_width_pt;
    parsed.clip_enabled = clip_fields->enabled;
    parsed.clip_y0_pt = clip_fields->y0_pt;
    parsed.clip_y1_pt = clip_fields->y1_pt;
    if (!parse_color_channel(shape, QStringLiteral("fillRed"), &parsed.fill_red) ||
        !parse_color_channel(shape, QStringLiteral("fillGreen"), &parsed.fill_green) ||
        !parse_color_channel(shape, QStringLiteral("fillBlue"), &parsed.fill_blue) ||
        !parse_color_channel(shape, QStringLiteral("fillAlpha"), &parsed.fill_alpha) ||
        !parse_color_channel(shape, QStringLiteral("strokeRed"), &parsed.stroke_red) ||
        !parse_color_channel(shape, QStringLiteral("strokeGreen"), &parsed.stroke_green) ||
        !parse_color_channel(shape, QStringLiteral("strokeBlue"), &parsed.stroke_blue) ||
        !parse_color_channel(shape, QStringLiteral("strokeAlpha"), &parsed.stroke_alpha)) {
        return std::nullopt;
    }
    return parsed;
}

} // namespace

std::optional<GlyphFrame> parse(const QVariantMap& frame) {
    if (!frame.value(QStringLiteral("valid")).toBool()) {
        return std::nullopt;
    }
    bool page_index_valid = false;
    const double page_index_value =
        frame.value(QStringLiteral("pageIndex")).toDouble(&page_index_valid);
    if (!page_index_valid || !std::isfinite(page_index_value) || page_index_value < 0.0 ||
        page_index_value > std::numeric_limits<int>::max() ||
        std::floor(page_index_value) != page_index_value) {
        return std::nullopt;
    }
    if (!is_finite_positive(frame.value(QStringLiteral("rasterPxPerPt")).toDouble())) {
        return std::nullopt;
    }
    if (!frame.value(QStringLiteral("images")).canConvert<QVariantList>() ||
        !frame.value(QStringLiteral("placements")).canConvert<QVariantList>() ||
        (frame.contains(QStringLiteral("decorations")) &&
         !frame.value(QStringLiteral("decorations")).canConvert<QVariantList>()) ||
        (frame.contains(QStringLiteral("shapes")) &&
         !frame.value(QStringLiteral("shapes")).canConvert<QVariantList>())) {
        return std::nullopt;
    }

    GlyphFrame parsed;
    parsed.page_index = static_cast<int>(page_index_value);
    parsed.raster_px_per_pt = frame.value(QStringLiteral("rasterPxPerPt")).toDouble();
    const QVariantList image_values = frame.value(QStringLiteral("images")).toList();
    const QVariantList placement_values = frame.value(QStringLiteral("placements")).toList();
    const QVariantList decoration_values = frame.value(QStringLiteral("decorations")).toList();
    const QVariantList shape_values = frame.value(QStringLiteral("shapes")).toList();

    parsed.images.reserve(image_values.size());
    for (const QVariant& value : image_values) {
        std::optional<GlyphImage> image = parse_image(value);
        if (!image) {
            return std::nullopt;
        }
        parsed.images.push_back(*image);
    }
    parsed.placements.reserve(placement_values.size());
    for (const QVariant& value : placement_values) {
        std::optional<GlyphPlacement> placement = parse_placement(value.toMap(), parsed.images);
        if (!placement) {
            return std::nullopt;
        }
        parsed.placements.push_back(*placement);
    }
    parsed.decorations.reserve(decoration_values.size());
    for (const QVariant& value : decoration_values) {
        std::optional<GlyphDecoration> decoration = parse_decoration(value);
        if (!decoration) {
            return std::nullopt;
        }
        parsed.decorations.push_back(*decoration);
    }
    parsed.shapes.reserve(shape_values.size());
    for (const QVariant& value : shape_values) {
        std::optional<GlyphShape> shape = parse_shape(value);
        if (!shape) {
            return std::nullopt;
        }
        parsed.shapes.push_back(*shape);
    }
    return parsed;
}

} // namespace caret::glyph_frame_wire
