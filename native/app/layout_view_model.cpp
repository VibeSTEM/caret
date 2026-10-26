// Rust 页面快照的只读 Qt 属性适配。
#include "layout_view_model.hpp"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSizeF>
#include <QVariant>
#include <cmath>
#include <utility>

namespace caret {
namespace {

QString& installed_snapshot() {
    static QString value;
    return value;
}

bool parse_page_sizes(const QString& snapshot_json, QVariantList& page_sizes, QString& error) {
    QJsonParseError parse_error;
    const QJsonDocument document = QJsonDocument::fromJson(snapshot_json.toUtf8(), &parse_error);
    if (parse_error.error != QJsonParseError::NoError || !document.isObject()) {
        error = QStringLiteral("layout.invalid_snapshot");
        return false;
    }
    const QJsonValue pages_value = document.object().value(QStringLiteral("pages"));
    if (!pages_value.isArray()) {
        error = QStringLiteral("layout.invalid_pages");
        return false;
    }

    QVariantList parsed;
    const QJsonArray pages = pages_value.toArray();
    parsed.reserve(pages.size());
    for (const QJsonValue& page_value : pages) {
        if (!page_value.isObject()) {
            error = QStringLiteral("layout.invalid_page");
            return false;
        }
        const QJsonObject page = page_value.toObject();
        const double width_pt = page.value(QStringLiteral("widthPt")).toDouble(-1.0);
        const double height_pt = page.value(QStringLiteral("heightPt")).toDouble(-1.0);
        if (!std::isfinite(width_pt) || !std::isfinite(height_pt) || width_pt <= 0.0 ||
            height_pt <= 0.0) {
            error = QStringLiteral("layout.invalid_page_geometry");
            return false;
        }
        parsed.push_back(QVariant::fromValue(QSizeF(width_pt, height_pt)));
    }
    page_sizes = std::move(parsed);
    error.clear();
    return true;
}

} // namespace

bool install_layout_snapshot(QString snapshot_json) {
    QVariantList ignored_pages;
    QString error;
    if (!parse_page_sizes(snapshot_json, ignored_pages, error)) {
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
    (void)parse_page_sizes(installed_snapshot(), m_page_sizes_pt, m_error);
}

} // namespace caret
