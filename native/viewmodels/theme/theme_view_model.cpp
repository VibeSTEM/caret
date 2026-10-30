// 将 Rust 主题快照投影为 QML 可绑定的主题属性。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "theme_view_model.hpp"

#include <QHash>
#include <QObject>
#include <QPointer>
#include <QString>
#include <QStringList>
#include <QVariantMap>
#include <qcontainerfwd.h>
#include <utility>

namespace caret {
namespace {

struct Catalog {
    QHash<QString, QVariantMap> themes;
    QString active;
};

Catalog& catalog() {
    static Catalog value;
    return value;
}

QPointer<ThemeViewModel>& active_view_model() {
    static QPointer<ThemeViewModel> value;
    return value;
}

} // namespace

void install_theme_snapshots(QHash<QString, QVariantMap> themes, QString active_theme) {
    catalog().themes = std::move(themes);
    catalog().active = std::move(active_theme);
    if (active_view_model() != nullptr) {
        active_view_model()->install(catalog().themes, catalog().active);
    }
}

ThemeViewModel::ThemeViewModel(QObject* parent) : QObject(parent) {
    active_view_model() = this;
    install(catalog().themes, catalog().active);
}

void ThemeViewModel::install(const QHash<QString, QVariantMap>& themes,
                             const QString& active_theme) {
    QStringList available;
    available.reserve(themes.size());
    for (auto it = themes.cbegin(); it != themes.cend(); ++it) {
        available.push_back(it.key());
    }
    available.sort();
    const QVariantMap tokens = themes.value(active_theme);
    if (m_availableThemes == available && m_activeTheme == active_theme && m_tokens == tokens) {
        return;
    }
    m_availableThemes = std::move(available);
    m_activeTheme = active_theme;
    m_tokens = tokens;
    emit tokensChanged();
}

} // namespace caret
