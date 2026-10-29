// Rust 布局 JSON 到只读 Qt 页面投影的内部转换边界。
#pragma once

#include <QByteArray>
#include <QHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonValue>
#include <QString>
#include <QVariantList>
#include <QVariantMap>

namespace caret::layout_snapshot {

struct ParsedLayoutSnapshot {
    QVariantList page_sizes;
    QJsonArray page_layouts;
    QVariantList page_glyph_revisions;
    QVariantMap page_margins;
    QJsonArray paragraph_layouts;
    QHash<QString, qsizetype> paragraph_line_counts;
    QVariantList selection_rects;
    QVariantList composition_rects;
    QVariantList current_structure;
    QVariantMap command_line;
    QVariantMap input_method_context;
    QString session_id;
    qulonglong document_generation = 0;
    qulonglong font_generation = 0;
};

/// 校验完整快照后一次性发布转换结果，失败时保留调用方原值。
bool parse(const QString& snapshot_json, ParsedLayoutSnapshot& parsed_snapshot, QString& error,
           const ParsedLayoutSnapshot* previous_snapshot = nullptr);
bool parse(const QByteArray& snapshot_json, ParsedLayoutSnapshot& parsed_snapshot, QString& error,
           const ParsedLayoutSnapshot* previous_snapshot = nullptr);
bool parse(const QJsonDocument& snapshot_document, ParsedLayoutSnapshot& parsed_snapshot,
           QString& error, const ParsedLayoutSnapshot* previous_snapshot = nullptr);
/// 校验命令查询的轻量 JSON，并转换为 Qt 属性映射。
bool parse_command_line_state(const QJsonValue& value, QVariantMap& command_line, QString& error);
bool parse_current_structure(const QJsonValue& value, QVariantList& current_structure,
                             QString& error);

} // namespace caret::layout_snapshot
