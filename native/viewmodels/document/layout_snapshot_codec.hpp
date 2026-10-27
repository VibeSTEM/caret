// Rust 布局 JSON 到只读 Qt 页面投影的内部转换边界。
#pragma once

#include <QString>
#include <QVariantList>
#include <QVariantMap>

namespace caret::layout_snapshot {

struct ParsedLayoutSnapshot {
    QVariantList page_sizes;
    QVariantList page_layouts;
    QVariantMap page_margins;
    QVariantList paragraph_layouts;
    QVariantList selection_rects;
    QVariantList composition_rects;
    QVariantMap input_method_context;
    QString session_id;
    qulonglong document_generation = 0;
    qulonglong font_generation = 0;
};

/// 校验完整快照后一次性发布转换结果，失败时保留调用方原值。
bool parse(const QString& snapshot_json, ParsedLayoutSnapshot& parsed_snapshot, QString& error,
           const ParsedLayoutSnapshot* previous_snapshot = nullptr);

} // namespace caret::layout_snapshot
