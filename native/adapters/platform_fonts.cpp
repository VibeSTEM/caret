// 查询当前平台已安装并可供 Qt 使用的字体族。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "platform_fonts.hpp"

#include "fonts/code.hpp"
#include <QFontDatabase>
#include <QObject>
#include <QSet>
#include <QString>

namespace caret {

PlatformFonts::PlatformFonts(QObject* parent)
    : QObject(parent), m_codeFamily(fonts::load_code_font()) {
    for (const QString& family : QFontDatabase::families()) {
        m_familyNames.insert(family.toCaseFolded());
    }
}

auto PlatformFonts::codeFamily() const -> const QString& { return m_codeFamily; }

auto PlatformFonts::containsFamily(const QString& family) const -> bool {
    return m_familyNames.contains(family.toCaseFolded());
}

} // namespace caret
