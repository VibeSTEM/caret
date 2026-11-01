// 从 Qt 静态资源加载 JetBrains Mono 的四种静态字形样式。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "code.hpp"

#include <QDebug>
#include <QFile>
#include <QFontDatabase>
#include <QList>
#include <QResource>
#include <QString>
#include <QStringList>
#include <mutex>

static void initialize_caret_code_fonts() { Q_INIT_RESOURCE(caret_code_fonts); }

namespace caret::fonts {

auto load_code_font() -> const QString& {
    static QString family;
    static std::once_flag loaded;
    std::call_once(loaded, [] {
        initialize_caret_code_fonts();
        const QStringList files{
            QStringLiteral(":/caret/fonts/JetBrainsMono-Regular.ttf"),
            QStringLiteral(":/caret/fonts/JetBrainsMono-Bold.ttf"),
            QStringLiteral(":/caret/fonts/JetBrainsMono-Italic.ttf"),
            QStringLiteral(":/caret/fonts/JetBrainsMono-BoldItalic.ttf"),
        };
        QList<int> registered_ids;
        QString failed_file;
        for (const QString& file : files) {
            QFile resource(file);
            if (!resource.open(QIODevice::ReadOnly)) {
                failed_file = file;
                break;
            }
            const int font_id = QFontDatabase::addApplicationFontFromData(resource.readAll());
            if (font_id < 0) {
                failed_file = file;
                break;
            }
            registered_ids.append(font_id);
            const QStringList families = QFontDatabase::applicationFontFamilies(font_id);
            if (families.isEmpty()) {
                failed_file = file;
                break;
            }
            if (file.endsWith(QStringLiteral("Regular.ttf"))) {
                family = families.constFirst();
            } else if (!families.contains(family)) {
                failed_file = file;
                break;
            }
        }
        if (failed_file.isEmpty() && !family.isEmpty() && registered_ids.size() == files.size()) {
            return;
        }
        for (const int font_id : registered_ids) {
            (void)QFontDatabase::removeApplicationFont(font_id);
        }
        family.clear();
        qCritical().noquote() << "无法完整注册内置代码字体资源：" << failed_file;
    });
    return family;
}

} // namespace caret::fonts
