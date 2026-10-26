#include "platform_fonts.hpp"

#include <QFontDatabase>
#include <QFontInfo>
#include <QObject>
#include <QString>

namespace caret {

PlatformFonts::PlatformFonts(QObject* parent)
    : QObject(parent),
      m_fixedFamily(QFontInfo(QFontDatabase::systemFont(QFontDatabase::FixedFont)).family()) {}

auto PlatformFonts::fixedFamily() const -> const QString& { return m_fixedFamily; }

} // namespace caret
