// 原子校验并安装页面字形帧，不创建场景图或纹理资源。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "document_canvas.hpp"
#include "value_validation.hpp"
#include <QByteArray>
#include <QList>
#include <QMetaType>
#include <QSet>
#include <QStringList>
#include <QVariant>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <qcontainerfwd.h>
#include <qhashfunctions.h>
#include <qtmetamacros.h>

namespace caret {
namespace {
using canvas_detail::is_boolean_variant;
using canvas_detail::is_finite_positive;

bool has_valid_paint_order(const QVariantMap& item) {
    const QVariant value = item.value(QStringLiteral("paintOrder"));
    if (!value.isValid()) {
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
        const qlonglong order = value.toLongLong(&converted);
        return converted && order >= 0;
    }
    case QMetaType::UChar:
    case QMetaType::UShort:
    case QMetaType::UInt:
    case QMetaType::ULong:
    case QMetaType::ULongLong: {
        bool converted = false;
        value.toULongLong(&converted);
        return converted;
    }
    case QMetaType::Float:
    case QMetaType::Double: {
        bool converted = false;
        const double order = value.toDouble(&converted);
        constexpr double max_exact_integer = 9007199254740991.0;
        return converted && std::isfinite(order) && order >= 0.0 && order <= max_exact_integer &&
               std::floor(order) == order;
    }
    default:
        return false;
    }
}

bool is_exact_integer(const QVariant& value, int minimum, int maximum, int* result = nullptr) {
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
    if (result != nullptr) {
        *result = static_cast<int>(number);
    }
    return true;
}

bool has_valid_color_channels(const QVariantMap& item, const QStringList& channels) {
    for (const QString& channel : channels) {
        if (!is_exact_integer(item.value(channel), 0, 255)) {
            return false;
        }
    }
    return true;
}

bool is_valid_decoration(const QVariant& value) {
    if (value.metaType().id() != QMetaType::QVariantMap) {
        return false;
    }
    const QVariantMap decoration = value.toMap();
    bool x_valid = false;
    bool y_valid = false;
    bool width_valid = false;
    bool height_valid = false;
    const double x = decoration.value(QStringLiteral("xPt")).toDouble(&x_valid);
    const double y = decoration.value(QStringLiteral("yPt")).toDouble(&y_valid);
    const double width = decoration.value(QStringLiteral("widthPt")).toDouble(&width_valid);
    const double height = decoration.value(QStringLiteral("heightPt")).toDouble(&height_valid);
    const QVariant clip_enabled_value = decoration.value(QStringLiteral("clipEnabled"));
    const bool clip_enabled = clip_enabled_value.toBool();
    bool clip_y0_valid = false;
    bool clip_y1_valid = false;
    const double clip_y0 = decoration.value(QStringLiteral("clipY0Pt")).toDouble(&clip_y0_valid);
    const double clip_y1 = decoration.value(QStringLiteral("clipY1Pt")).toDouble(&clip_y1_valid);
    return x_valid && y_valid && width_valid && height_valid && std::isfinite(x) &&
           std::isfinite(y) && std::isfinite(width) && std::isfinite(height) && width > 0.0 &&
           height > 0.0 && is_boolean_variant(clip_enabled_value) &&
           (!clip_enabled || (clip_y0_valid && clip_y1_valid && std::isfinite(clip_y0) &&
                              std::isfinite(clip_y1) && clip_y0 >= 0.0 && clip_y1 > clip_y0)) &&
           has_valid_paint_order(decoration) &&
           has_valid_color_channels(decoration,
                                    {QStringLiteral("colorRed"), QStringLiteral("colorGreen"),
                                     QStringLiteral("colorBlue"), QStringLiteral("colorAlpha")});
}

bool is_valid_shape(const QVariant& value) {
    if (value.metaType().id() != QMetaType::QVariantMap) {
        return false;
    }
    const QVariantMap shape = value.toMap();
    int kind = 0;
    const bool kind_valid = is_exact_integer(shape.value(QStringLiteral("kind")), 0, 1, &kind);
    bool x_valid = false;
    bool y_valid = false;
    bool dx_valid = false;
    bool dy_valid = false;
    bool width_valid = false;
    bool height_valid = false;
    bool stroke_width_valid = false;
    const double x = shape.value(QStringLiteral("xPt")).toDouble(&x_valid);
    const double y = shape.value(QStringLiteral("yPt")).toDouble(&y_valid);
    const double dx = shape.value(QStringLiteral("dxPt")).toDouble(&dx_valid);
    const double dy = shape.value(QStringLiteral("dyPt")).toDouble(&dy_valid);
    const double width = shape.value(QStringLiteral("widthPt")).toDouble(&width_valid);
    const double height = shape.value(QStringLiteral("heightPt")).toDouble(&height_valid);
    const double stroke_width =
        shape.value(QStringLiteral("strokeWidthPt")).toDouble(&stroke_width_valid);
    const QVariant fill_enabled_value = shape.value(QStringLiteral("fillEnabled"));
    const QVariant stroke_enabled_value = shape.value(QStringLiteral("strokeEnabled"));
    const QVariant clip_enabled_value = shape.value(QStringLiteral("clipEnabled"));
    const bool clip_enabled = clip_enabled_value.toBool();
    bool clip_y0_valid = false;
    bool clip_y1_valid = false;
    const double clip_y0 = shape.value(QStringLiteral("clipY0Pt")).toDouble(&clip_y0_valid);
    const double clip_y1 = shape.value(QStringLiteral("clipY1Pt")).toDouble(&clip_y1_valid);
    if (!kind_valid || (kind != 0 && kind != 1) || !x_valid || !y_valid || !dx_valid || !dy_valid ||
        !width_valid || !height_valid || !stroke_width_valid || !std::isfinite(x) ||
        !std::isfinite(y) || !std::isfinite(dx) || !std::isfinite(dy) || !std::isfinite(width) ||
        !std::isfinite(height) || !std::isfinite(stroke_width) || width < 0.0 || height < 0.0 ||
        stroke_width < 0.0 || !is_boolean_variant(fill_enabled_value) ||
        !is_boolean_variant(stroke_enabled_value) || !is_boolean_variant(clip_enabled_value) ||
        (kind == 0 && (!stroke_enabled_value.toBool() || stroke_width <= 0.0)) ||
        (kind == 1 && (width <= 0.0 || height <= 0.0)) ||
        (clip_enabled && (!clip_y0_valid || !clip_y1_valid || !std::isfinite(clip_y0) ||
                          !std::isfinite(clip_y1) || clip_y0 < 0.0 || clip_y1 <= clip_y0)) ||
        !has_valid_paint_order(shape)) {
        return false;
    }
    return has_valid_color_channels(shape,
                                    {QStringLiteral("fillRed"), QStringLiteral("fillGreen"),
                                     QStringLiteral("fillBlue"), QStringLiteral("fillAlpha"),
                                     QStringLiteral("strokeRed"), QStringLiteral("strokeGreen"),
                                     QStringLiteral("strokeBlue"), QStringLiteral("strokeAlpha")});
}
} // namespace

QVariantList DocumentCanvas::glyphFrames() const { return m_glyph_frames; }

QVariantList DocumentCanvas::visiblePageRange() const {
    const int page_end =
        m_page_end_visible < 0 ? static_cast<int>(m_page_sizes_pt.size()) : m_page_end_visible;
    return {m_first_visible_page, page_end};
}

void DocumentCanvas::setVisiblePageRange(const QVariantList& range) {
    if (range.size() != 2) {
        return;
    }
    bool first_valid = false;
    bool end_valid = false;
    const int first = range.at(0).toInt(&first_valid);
    const int end = range.at(1).toInt(&end_valid);
    if (!first_valid || !end_valid || first < 0 || end < first || end > m_page_sizes_pt.size() ||
        (first == m_first_visible_page && end == m_page_end_visible)) {
        return;
    }
    m_first_visible_page = first;
    m_page_end_visible = end;
    ++m_highlight_revision;
    emit visiblePageRangeChanged();
    update();
}

void DocumentCanvas::setGlyphFrames(const QVariantList& frames) {
    QSet<int> page_indexes;
    for (const QVariant& value : frames) {
        const QVariantMap frame = value.toMap();
        bool page_converted = false;
        const double page_index_value =
            frame.value(QStringLiteral("pageIndex")).toDouble(&page_converted);
        if (!frame.value(QStringLiteral("valid")).toBool() || !page_converted ||
            !std::isfinite(page_index_value) || page_index_value < 0.0 ||
            page_index_value > std::numeric_limits<int>::max() ||
            std::floor(page_index_value) != page_index_value ||
            !is_finite_positive(frame.value(QStringLiteral("rasterPxPerPt")).toDouble()) ||
            !frame.value(QStringLiteral("images")).canConvert<QVariantList>() ||
            !frame.value(QStringLiteral("placements")).canConvert<QVariantList>() ||
            (frame.contains(QStringLiteral("decorations")) &&
             !frame.value(QStringLiteral("decorations")).canConvert<QVariantList>()) ||
            (frame.contains(QStringLiteral("shapes")) &&
             !frame.value(QStringLiteral("shapes")).canConvert<QVariantList>())) {
            return;
        }
        const int page_index = static_cast<int>(page_index_value);
        const QVariantList images = frame.value(QStringLiteral("images")).toList();
        const QVariantList placements = frame.value(QStringLiteral("placements")).toList();
        const QVariantList decorations = frame.value(QStringLiteral("decorations")).toList();
        const QVariantList shapes = frame.value(QStringLiteral("shapes")).toList();
        if (pageSizePt(page_index).isEmpty() || page_indexes.contains(page_index)) {
            return;
        }
        QList<bool> image_phase_applied;
        image_phase_applied.reserve(images.size());
        for (const QVariant& image_value : images) {
            const QVariantMap image = image_value.toMap();
            bool width_converted = false;
            bool height_converted = false;
            bool left_valid = false;
            bool top_valid = false;
            const double width_value =
                image.value(QStringLiteral("width")).toDouble(&width_converted);
            const double height_value =
                image.value(QStringLiteral("height")).toDouble(&height_converted);
            const double left = image.value(QStringLiteral("left")).toDouble(&left_valid);
            const double top = image.value(QStringLiteral("top")).toDouble(&top_valid);
            const QVariant phase_applied_value = image.value(QStringLiteral("pixelPhaseApplied"));
            const QByteArray pixels = image.value(QStringLiteral("pixels")).toByteArray();
            if (!width_converted || !height_converted || !std::isfinite(width_value) ||
                !std::isfinite(height_value) || width_value < 0.0 || height_value < 0.0 ||
                width_value > std::numeric_limits<int>::max() ||
                height_value > std::numeric_limits<int>::max() ||
                std::floor(width_value) != width_value ||
                std::floor(height_value) != height_value) {
                return;
            }
            const int width = static_cast<int>(width_value);
            const int height = static_cast<int>(height_value);
            const quint64 byte_count = static_cast<quint64>(std::max(width, 0)) *
                                       static_cast<quint64>(std::max(height, 0)) * 4U;
            const bool empty_image = byte_count == 0 && pixels.isEmpty();
            if (width > std::numeric_limits<int>::max() / 4 ||
                (!empty_image && byte_count != static_cast<quint64>(pixels.size())) ||
                !left_valid || !top_valid || !std::isfinite(left) || !std::isfinite(top) ||
                std::floor(left) != left || std::floor(top) != top ||
                left < std::numeric_limits<std::int32_t>::min() ||
                left > std::numeric_limits<std::int32_t>::max() ||
                top < std::numeric_limits<std::int32_t>::min() ||
                top > std::numeric_limits<std::int32_t>::max() ||
                !is_boolean_variant(phase_applied_value) ||
                image.value(QStringLiteral("pixels")).metaType().id() != QMetaType::QByteArray) {
                return;
            }
            image_phase_applied.push_back(phase_applied_value.toBool());
        }
        for (const QVariant& placement_value : placements) {
            const QVariantMap placement = placement_value.toMap();
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
            const double phase_x =
                placement.value(QStringLiteral("pixelPhaseX")).toDouble(&phase_x_valid);
            const double phase_y =
                placement.value(QStringLiteral("pixelPhaseY")).toDouble(&phase_y_valid);
            const QVariant phase_applied_value =
                placement.value(QStringLiteral("pixelPhaseApplied"));
            const QVariant clip_enabled_value = placement.value(QStringLiteral("clipEnabled"));
            const bool clip_enabled = clip_enabled_value.toBool();
            bool clip_y0_valid = false;
            bool clip_y1_valid = false;
            const double clip_y0 =
                placement.value(QStringLiteral("clipY0Pt")).toDouble(&clip_y0_valid);
            const double clip_y1 =
                placement.value(QStringLiteral("clipY1Pt")).toDouble(&clip_y1_valid);
            if (!image_index_valid || !std::isfinite(image_index_value) ||
                image_index_value < 0.0 ||
                image_index_value >= static_cast<double>(images.size()) ||
                std::floor(image_index_value) != image_index_value || !x_valid || !baseline_valid ||
                !phase_x_valid || !phase_y_valid || !std::isfinite(x_pt) ||
                !std::isfinite(baseline_y_pt) || !std::isfinite(phase_x) ||
                !std::isfinite(phase_y) || phase_x < 0.0 || phase_x >= 1.0 || phase_y < 0.0 ||
                phase_y >= 1.0 || !is_boolean_variant(phase_applied_value) ||
                !is_boolean_variant(clip_enabled_value) || !has_valid_paint_order(placement) ||
                (clip_enabled &&
                 (!clip_y0_valid || !clip_y1_valid || !std::isfinite(clip_y0) ||
                  !std::isfinite(clip_y1) || clip_y0 < 0.0 || clip_y1 <= clip_y0))) {
                return;
            }
            const qsizetype image_index = static_cast<qsizetype>(image_index_value);
            if (phase_applied_value.toBool() != image_phase_applied[image_index]) {
                return;
            }
        }
        for (const QVariant& decoration : decorations) {
            if (!is_valid_decoration(decoration)) {
                return;
            }
        }
        for (const QVariant& shape : shapes) {
            if (!is_valid_shape(shape)) {
                return;
            }
        }
        page_indexes.insert(page_index);
    }
    if (m_glyph_frames == frames) {
        return;
    }
    m_glyph_frames = frames;
    ++m_glyph_frame_revision;
    emit glyphFramesChanged();
    update();
}

} // namespace caret
