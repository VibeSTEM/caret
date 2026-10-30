// 把 Qt 输入法事件与 UTF-16 替换范围提交给 Rust。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "layout_view_model.hpp"
#ifdef CARET_ENABLE_APP
#include "caret_ffi.h"
#include "document_runtime_bridge.hpp"
#endif
#include <QByteArray>
#include <QCoreApplication>
#include <QString>
#include <QThread>
#include <QVariant>
#include <algorithm>
#include <cstddef>
#include <qcontainerfwd.h>
#include <string>

namespace caret {
QVariantMap LayoutViewModel::inputMethodEvent(const QString& preedit, const QString& commit,
                                              int replacement_start, int replacement_length,
                                              int cursor_position_utf16) {
#ifdef CARET_ENABLE_APP
    const QCoreApplication* application = QCoreApplication::instance();
    if (application == nullptr || QThread::currentThread() != application->thread()) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("ime.wrong_thread")}};
    }
    auto* runtime = document_runtime();
    if (runtime == nullptr) {
        return {{QStringLiteral("valid"), false},
                {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
    }
    const QByteArray preedit_utf8 = preedit.toUtf8();
    const QByteArray commit_utf8 = commit.toUtf8();
    const rust::String response = ffi::input_method_event_json(
        *runtime,
        rust::String(
            std::string(preedit_utf8.constData(), static_cast<std::size_t>(preedit_utf8.size()))),
        rust::String(
            std::string(commit_utf8.constData(), static_cast<std::size_t>(commit_utf8.size()))),
        replacement_start, replacement_length,
        static_cast<std::size_t>(std::max(0, cursor_position_utf16)));
    return installInteractionResponse(
        QByteArray(response.data(), static_cast<qsizetype>(response.size())));
#else
    Q_UNUSED(preedit)
    Q_UNUSED(commit)
    Q_UNUSED(replacement_start)
    Q_UNUSED(replacement_length)
    Q_UNUSED(cursor_position_utf16)
    return {{QStringLiteral("valid"), false},
            {QStringLiteral("error"), QStringLiteral("layout.runtime_unavailable")}};
#endif
}

} // namespace caret
