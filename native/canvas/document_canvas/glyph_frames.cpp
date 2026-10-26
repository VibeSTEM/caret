// 原子校验并安装页面字形帧，不创建场景图或纹理资源。
#include "document_canvas.hpp"
#include "value_validation.hpp"
#include <QByteArray>
#include <QList>
#include <QMetaType>
#include <QSet>
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
} // namespace

QVariantList DocumentCanvas::glyphFrames() const { return m_glyph_frames; }

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
            !frame.value(QStringLiteral("placements")).canConvert<QVariantList>()) {
            return;
        }
        const int page_index = static_cast<int>(page_index_value);
        const QVariantList images = frame.value(QStringLiteral("images")).toList();
        const QVariantList placements = frame.value(QStringLiteral("placements")).toList();
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
            const double clip_y0 = placement.value(QStringLiteral("clipY0Pt")).toDouble();
            const double clip_y1 = placement.value(QStringLiteral("clipY1Pt")).toDouble();
            if (!image_index_valid || !std::isfinite(image_index_value) ||
                image_index_value < 0.0 ||
                image_index_value >= static_cast<double>(images.size()) ||
                std::floor(image_index_value) != image_index_value || !x_valid || !baseline_valid ||
                !phase_x_valid || !phase_y_valid || !std::isfinite(x_pt) ||
                !std::isfinite(baseline_y_pt) || !std::isfinite(phase_x) ||
                !std::isfinite(phase_y) || phase_x < 0.0 || phase_x >= 1.0 || phase_y < 0.0 ||
                phase_y >= 1.0 || !is_boolean_variant(phase_applied_value) ||
                !is_boolean_variant(clip_enabled_value) ||
                (clip_enabled && (!std::isfinite(clip_y0) || !std::isfinite(clip_y1) ||
                                  clip_y0 < 0.0 || clip_y1 <= clip_y0))) {
                return;
            }
            const qsizetype image_index = static_cast<qsizetype>(image_index_value);
            if (phase_applied_value.toBool() != image_phase_applied[image_index]) {
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
