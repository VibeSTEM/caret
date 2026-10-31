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
#include <QVariant>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <qcontainerfwd.h>
#include <qhashfunctions.h>
#include <qtmetamacros.h>
#include <utility>

namespace caret {
namespace {
using canvas_detail::is_boolean_variant;
using canvas_detail::is_finite_positive;

QVariantMap glyph_frame_identity(QVariantMap frame) {
    frame.remove(QStringLiteral("documentGeneration"));
    frame.remove(QStringLiteral("fontGeneration"));
    return frame;
}

QVariantMap glyph_frame_content_identity(QVariantMap frame) {
    frame.remove(QStringLiteral("pageSubmissionGeneration"));
    return glyph_frame_identity(std::move(frame));
}

QVariantMap glyph_frame_with_submission_generation(QVariantMap frame, quint64 generation) {
    frame.insert(QStringLiteral("pageSubmissionGeneration"), QString::number(generation));
    return glyph_frame_identity(std::move(frame));
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
    ++m_environment_highlight_revision;
    emit visiblePageRangeChanged();
    update();
}

void DocumentCanvas::setGlyphFrames(const QVariantList& frames) {
    QSet<int> page_indexes;
    QHash<int, QVariantMap> previous_frames;
    for (const QVariant& value : m_glyph_frames) {
        const QVariantMap frame = value.toMap();
        previous_frames.insert(frame.value(QStringLiteral("pageIndex")).toInt(), frame);
    }
    QHash<int, quint64> next_submission_generations = m_glyph_page_submission_generations;
    for (const QVariant& value : frames) {
        QVariantMap frame = value.toMap();
        bool page_converted = false;
        const double page_index_value =
            frame.value(QStringLiteral("pageIndex")).toDouble(&page_converted);
        if (!frame.value(QStringLiteral("valid")).toBool() || !page_converted ||
            !std::isfinite(page_index_value) || page_index_value < 0.0 ||
            page_index_value > std::numeric_limits<int>::max() ||
            std::floor(page_index_value) != page_index_value ||
            !is_finite_positive(frame.value(QStringLiteral("rasterPxPerPt")).toDouble()) ||
            !frame.value(QStringLiteral("images")).canConvert<QVariantList>() ||
            !frame.value(QStringLiteral("commands")).canConvert<QVariantList>()) {
            return;
        }
        const int page_index = static_cast<int>(page_index_value);
        const QVariantList images = frame.value(QStringLiteral("images")).toList();
        const QVariantList commands = frame.value(QStringLiteral("commands")).toList();
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
        for (const QVariant& command_value : commands) {
            const QVariantMap command = command_value.toMap();
            bool kind_valid = false;
            const int kind = command.value(QStringLiteral("kind")).toInt(&kind_valid);
            bool kind_number_valid = false;
            const double kind_number =
                command.value(QStringLiteral("kind")).toDouble(&kind_number_valid);
            bool image_index_valid = false;
            const double image_index_value =
                command.value(QStringLiteral("imageIndex")).toDouble(&image_index_valid);
            bool x_valid = false;
            bool y_valid = false;
            bool width_valid = false;
            bool height_valid = false;
            bool x2_valid = false;
            bool y2_valid = false;
            bool thickness_valid = false;
            bool phase_x_valid = false;
            bool phase_y_valid = false;
            const double x = command.value(QStringLiteral("xPt")).toDouble(&x_valid);
            const double y = command.value(QStringLiteral("yPt")).toDouble(&y_valid);
            const double width = command.value(QStringLiteral("widthPt")).toDouble(&width_valid);
            const double height = command.value(QStringLiteral("heightPt")).toDouble(&height_valid);
            const double x2 = command.value(QStringLiteral("x2Pt")).toDouble(&x2_valid);
            const double y2 = command.value(QStringLiteral("y2Pt")).toDouble(&y2_valid);
            const double thickness =
                command.value(QStringLiteral("thicknessPt")).toDouble(&thickness_valid);
            const double phase_x =
                command.value(QStringLiteral("pixelPhaseX")).toDouble(&phase_x_valid);
            const double phase_y =
                command.value(QStringLiteral("pixelPhaseY")).toDouble(&phase_y_valid);
            const QVariant phase_applied_value = command.value(QStringLiteral("pixelPhaseApplied"));
            const QVariant clip_enabled_value = command.value(QStringLiteral("clipEnabled"));
            const bool clip_enabled = clip_enabled_value.toBool();
            bool clip_y0_valid = false;
            bool clip_y1_valid = false;
            const double clip_y0 =
                command.value(QStringLiteral("clipY0Pt")).toDouble(&clip_y0_valid);
            const double clip_y1 =
                command.value(QStringLiteral("clipY1Pt")).toDouble(&clip_y1_valid);
            bool red_valid = false;
            bool green_valid = false;
            bool blue_valid = false;
            bool alpha_valid = false;
            const int red = command.value(QStringLiteral("colorRed")).toInt(&red_valid);
            const int green = command.value(QStringLiteral("colorGreen")).toInt(&green_valid);
            const int blue = command.value(QStringLiteral("colorBlue")).toInt(&blue_valid);
            const int alpha = command.value(QStringLiteral("colorAlpha")).toInt(&alpha_valid);
            bool red_number_valid = false;
            bool green_number_valid = false;
            bool blue_number_valid = false;
            bool alpha_number_valid = false;
            const double red_number =
                command.value(QStringLiteral("colorRed")).toDouble(&red_number_valid);
            const double green_number =
                command.value(QStringLiteral("colorGreen")).toDouble(&green_number_valid);
            const double blue_number =
                command.value(QStringLiteral("colorBlue")).toDouble(&blue_number_valid);
            const double alpha_number =
                command.value(QStringLiteral("colorAlpha")).toDouble(&alpha_number_valid);
            if (!kind_valid || !kind_number_valid || std::floor(kind_number) != kind_number ||
                kind < 0 || kind > 2 || !image_index_valid || !std::isfinite(image_index_value) ||
                image_index_value < 0.0 || std::floor(image_index_value) != image_index_value ||
                !x_valid || !y_valid || !width_valid || !height_valid || !x2_valid || !y2_valid ||
                !thickness_valid || !phase_x_valid || !phase_y_valid || !std::isfinite(x) ||
                !std::isfinite(y) || !std::isfinite(width) || !std::isfinite(height) ||
                !std::isfinite(x2) || !std::isfinite(y2) || !std::isfinite(thickness) ||
                !std::isfinite(phase_x) || !std::isfinite(phase_y) || width < 0.0 || height < 0.0 ||
                thickness < 0.0 || !is_boolean_variant(phase_applied_value) ||
                !is_boolean_variant(clip_enabled_value) || !red_valid || !green_valid ||
                !blue_valid || !alpha_valid || !red_number_valid || !green_number_valid ||
                !blue_number_valid || !alpha_number_valid || std::floor(red_number) != red_number ||
                std::floor(green_number) != green_number ||
                std::floor(blue_number) != blue_number ||
                std::floor(alpha_number) != alpha_number || red < 0 || red > 255 || green < 0 ||
                green > 255 || blue < 0 || blue > 255 || alpha < 0 || alpha > 255 ||
                (clip_enabled &&
                 (!clip_y0_valid || !clip_y1_valid || !std::isfinite(clip_y0) ||
                  !std::isfinite(clip_y1) || clip_y0 < 0.0 || clip_y1 <= clip_y0))) {
                return;
            }
            if (kind == 0) {
                if (image_index_value >= static_cast<double>(images.size()) || phase_x < 0.0 ||
                    phase_x >= 1.0 || phase_y < 0.0 || phase_y >= 1.0 ||
                    phase_applied_value.toBool() !=
                        image_phase_applied[static_cast<qsizetype>(image_index_value)]) {
                    return;
                }
            } else if (kind == 1 && (width <= 0.0 || height <= 0.0)) {
                return;
            }
        }
        page_indexes.insert(page_index);
        const auto previous = previous_frames.constFind(page_index);
        const bool same_content =
            previous != previous_frames.cend() &&
            glyph_frame_content_identity(previous.value()) == glyph_frame_content_identity(frame);
        quint64 submission_generation = next_submission_generations.value(page_index, 0);
        if (!same_content) {
            submission_generation = submission_generation == std::numeric_limits<quint64>::max()
                                        ? 1
                                        : submission_generation + 1;
            next_submission_generations.insert(page_index, submission_generation);
        }
    }
    if (m_glyph_frames == frames) {
        return;
    }
    m_glyph_page_submission_generations = std::move(next_submission_generations);
    const bool had_build_errors = glyphFrameBuildFailed();
    m_glyph_frames = frames;
    const QList<int> failed_pages = m_qsg_failed_frames.keys();
    for (const int failed_page : failed_pages) {
        const QVariantMap failed_identity = m_qsg_failed_frames.value(failed_page);
        const auto current_frame = std::find_if(
            m_glyph_frames.cbegin(), m_glyph_frames.cend(), [&failed_page](const QVariant& value) {
                const QVariantMap frame = value.toMap();
                return frame.value(QStringLiteral("pageIndex")).toInt() == failed_page;
            });
        const bool still_current =
            current_frame != m_glyph_frames.cend() &&
            glyph_frame_with_submission_generation(
                current_frame->toMap(), m_glyph_page_submission_generations.value(failed_page)) ==
                failed_identity;
        if (!still_current) {
            m_qsg_failed_frames.remove(failed_page);
        }
    }
    ++m_glyph_frame_revision;
    emit glyphFramesChanged();
    if (had_build_errors != glyphFrameBuildFailed()) {
        emit glyphFrameBuildErrorsChanged();
    }
    update();
}

void DocumentCanvas::reportGlyphFrameBuildResult(int page_index, const QVariantMap& frame_identity,
                                                 bool succeeded, quint64 geometry_revision,
                                                 quint64 scene_graph_generation,
                                                 quintptr window_identity) {
    if (geometry_revision != m_glyph_geometry_revision ||
        scene_graph_generation != m_scene_graph_generation ||
        window_identity != reinterpret_cast<quintptr>(window())) {
        return;
    }
    const auto current = std::find_if(
        m_glyph_frames.cbegin(), m_glyph_frames.cend(), [page_index](const QVariant& value) {
            return value.toMap().value(QStringLiteral("pageIndex")).toInt() == page_index;
        });
    if (current == m_glyph_frames.cend() ||
        glyph_frame_with_submission_generation(
            current->toMap(), m_glyph_page_submission_generations.value(page_index)) !=
            frame_identity) {
        return;
    }
    const bool had_build_errors = glyphFrameBuildFailed();
    if (succeeded) {
        m_qsg_failed_frames.remove(page_index);
    } else {
        m_qsg_failed_frames.insert(page_index, frame_identity);
    }
    if (had_build_errors != glyphFrameBuildFailed()) {
        emit glyphFrameBuildErrorsChanged();
    }
}

} // namespace caret
