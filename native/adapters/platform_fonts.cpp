#include "platform_fonts.hpp"

#include <QFontDatabase>
#include <QFontInfo>
#include <QObject>
#include <QSet>
#include <QString>

namespace caret {

PlatformFonts::PlatformFonts(QObject* parent)
    : QObject(parent),
      m_fixedFamily(QFontInfo(QFontDatabase::systemFont(QFontDatabase::FixedFont)).family()),
      m_familyNames([] {
          QSet<QString> names;
          const auto families = QFontDatabase::families();
          for (const QString& family : families) {
              names.insert(family.toCaseFolded());
          }
          return names;
      }()) {}

auto PlatformFonts::fixedFamily() const -> const QString& { return m_fixedFamily; }

auto PlatformFonts::containsFamily(const QString& family) const -> bool {
    return m_familyNames.contains(family.toCaseFolded());
}

} // namespace caret
