#include "theme_view_model.hpp"

#include <QHash>
#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariantMap>
#include <qcontainerfwd.h>
#include <qtmetamacros.h>
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

} // namespace

void install_theme_snapshots(QHash<QString, QVariantMap> themes, QString active_theme) {
    catalog().themes = std::move(themes);
    catalog().active = std::move(active_theme);
}

ThemeViewModel::ThemeViewModel(QObject* parent) : QObject(parent) {
    const Catalog& source = catalog();
    m_availableThemes.reserve(source.themes.size());
    for (auto it = source.themes.cbegin(); it != source.themes.cend(); ++it) {
        m_availableThemes.push_back(it.key());
    }
    m_availableThemes.sort();
    m_activeTheme = source.active;
    m_tokens = source.themes.value(m_activeTheme);
}

bool ThemeViewModel::selectTheme(const QString& id) {
    const Catalog& source = catalog();
    if (!source.themes.contains(id)) {
        setError(QStringLiteral("theme.unknown_id"));
        return false;
    }
    if (id == m_activeTheme) {
        setError({});
        return true;
    }
    const QVariantMap next = source.themes.value(id);
    const bool tokens_changed = next != m_tokens;
    m_tokens = next;
    m_activeTheme = id;
    setError({});
    if (tokens_changed) {
        emit tokensChanged();
    }
    emit activeThemeChanged();
    return true;
}

void ThemeViewModel::setError(QString error) {
    if (m_error == error) {
        return;
    }
    m_error = std::move(error);
    emit errorChanged();
}

} // namespace caret
