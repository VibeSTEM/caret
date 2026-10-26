#include "theme_view_model.hpp"

#include <QHash>
#include <QObject>
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

} // namespace caret
