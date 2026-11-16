// 校验并安装页面字形帧；线格式解析与字段校验统一由 glyph_frame_wire 提供。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "document_canvas.hpp"
#include "glyph_frame_wire.hpp"
#include <QSet>
#include <QVariant>
#include <optional>
#include <qcontainerfwd.h>
#include <qtmetamacros.h>

namespace caret {

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
        const std::optional<glyph_frame_wire::GlyphFrame> frame =
            glyph_frame_wire::parse(value.toMap());
        if (!frame || pageSizePt(frame->page_index).isEmpty() ||
            page_indexes.contains(frame->page_index)) {
            return;
        }
        page_indexes.insert(frame->page_index);
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
