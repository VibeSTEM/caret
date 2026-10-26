// 安装 Rust 布局快照，并发布只读页面与插入点属性。
#include "layout_view_model.hpp"

#include "layout_snapshot_codec.hpp"
#include <QObject>
#include <qtmetamacros.h>
#include <utility>

namespace caret {
namespace {
QString& installed_snapshot() {
    static QString value;
    return value;
}
} // namespace

bool install_layout_snapshot(QString snapshot_json) {
    layout_snapshot::ParsedLayoutSnapshot parsed_snapshot;
    QString error;
    if (!layout_snapshot::parse(snapshot_json, parsed_snapshot, error)) {
        return false;
    }
    installed_snapshot() = std::move(snapshot_json);
    return true;
}

LayoutViewModel::LayoutViewModel(QObject* parent) : QObject(parent) {
    if (installed_snapshot().isEmpty()) {
        m_error = QStringLiteral("layout.snapshot_unavailable");
        return;
    }
    layout_snapshot::ParsedLayoutSnapshot parsed_snapshot;
    if (layout_snapshot::parse(installed_snapshot(), parsed_snapshot, m_error)) {
        m_page_sizes_pt = std::move(parsed_snapshot.page_sizes);
        m_page_layouts = std::move(parsed_snapshot.page_layouts);
        m_page_margins_pt = std::move(parsed_snapshot.page_margins);
        m_paragraph_layouts = std::move(parsed_snapshot.paragraph_layouts);
        m_document_generation = parsed_snapshot.document_generation;
        m_font_generation = parsed_snapshot.font_generation;
    }
}

bool LayoutViewModel::installSnapshot(const QString& snapshot_json) {
    layout_snapshot::ParsedLayoutSnapshot parsed_snapshot;
    QString error;
    if (!layout_snapshot::parse(snapshot_json, parsed_snapshot, error)) {
        m_error = std::move(error);
        return false;
    }
    m_page_sizes_pt = std::move(parsed_snapshot.page_sizes);
    m_page_layouts = std::move(parsed_snapshot.page_layouts);
    m_page_margins_pt = std::move(parsed_snapshot.page_margins);
    m_paragraph_layouts = std::move(parsed_snapshot.paragraph_layouts);
    m_document_generation = parsed_snapshot.document_generation;
    m_font_generation = parsed_snapshot.font_generation;
    m_error.clear();
    installed_snapshot() = snapshot_json;
    emit layoutChanged();
    return true;
}

void LayoutViewModel::installCaretGeometry(const QVariantMap& geometry) {
    if (m_caret_geometry == geometry) {
        return;
    }
    m_caret_geometry = geometry;
    emit caretGeometryChanged();
}

} // namespace caret
