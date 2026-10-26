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

bool parse_layout_snapshot(const QString& snapshot_json, QVariantList& page_sizes,
                           QVariantMap& page_margins, QString& error) {
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
    const QJsonValue margins_value = document.object().value(QStringLiteral("marginsPt"));
    if (!margins_value.isObject()) {
        error = QStringLiteral("layout.invalid_page_margins");
        return false;
    }
    const QJsonObject margins = margins_value.toObject();
    QVariantMap parsed_margins;
    for (const QString& name : {QStringLiteral("top"), QStringLiteral("right"),
                                QStringLiteral("bottom"), QStringLiteral("left")}) {
        const double value = margins.value(name).toDouble(-1.0);
        if (!std::isfinite(value) || value < 0.0) {
            error = QStringLiteral("layout.invalid_page_margins");
            return false;
        }
        parsed_margins.insert(name, value);
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
        if (parsed_margins.value(QStringLiteral("left")).toDouble() +
                    parsed_margins.value(QStringLiteral("right")).toDouble() >=
                width_pt ||
            parsed_margins.value(QStringLiteral("top")).toDouble() +
                    parsed_margins.value(QStringLiteral("bottom")).toDouble() >=
                height_pt) {
            error = QStringLiteral("layout.invalid_page_margins");
            return false;
        }
        parsed.push_back(QVariant::fromValue(QSizeF(width_pt, height_pt)));
    }
    page_sizes = std::move(parsed);
    page_margins = std::move(parsed_margins);
    error.clear();
    return true;
}

} // namespace

bool install_layout_snapshot(QString snapshot_json) {
    QVariantList ignored_pages;
    QVariantMap ignored_margins;
    QString error;
    if (!parse_layout_snapshot(snapshot_json, ignored_pages, ignored_margins, error)) {
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
    (void)parse_layout_snapshot(installed_snapshot(), m_page_sizes_pt, m_page_margins_pt, m_error);
}

} // namespace caret
