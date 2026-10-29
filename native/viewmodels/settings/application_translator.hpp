// 预加载并原子替换应用级 Qt 翻译器。
#pragma once

#include <QString>
#include <QTranslator>
#include <memory>

class QQmlEngine;

namespace caret {

/// 先加载候选 QM；空指针表示 locale 没有构建翻译资源。
std::unique_ptr<QTranslator> prepare_application_translator(const QString& locale);
/// 绑定应用的 QML engine，以便语言切换后刷新已有的翻译绑定。
void set_application_qml_engine(QQmlEngine* engine);
/// 安装已验证的翻译器，并释放上一份活动翻译器。
bool install_application_translator(std::unique_ptr<QTranslator> translator);

} // namespace caret
