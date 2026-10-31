// 将文档保存路径转换为 Qt 文件对话框使用的路径。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#include "document_path_adapter.hpp"

#include <QDir>
#include <QFileInfo>
#include <QObject>
#include <QStandardPaths>
#include <QString>
#include <QUrl>
#include <qstringliteral.h>

namespace caret {

DocumentPathAdapter::DocumentPathAdapter(QObject* parent) : QObject(parent) {}

QUrl DocumentPathAdapter::defaultSaveFileUrl() const {
    const QString documents = QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation);
    return defaultSaveFileUrlForDocumentsLocation(documents);
}

QUrl DocumentPathAdapter::defaultSaveFileUrlForDocumentsLocation(const QString& documents) {
    if (documents.isEmpty()) {
        return {};
    }
    const QString directory = QDir(documents).filePath(QStringLiteral("caret"));
    if (!QDir().mkpath(directory)) {
        return {};
    }
    const QDir save_directory(directory);
    const QString base_name = QStringLiteral("Untitled");
    const QString suffix = QStringLiteral("cdoc");
    QString candidate = save_directory.filePath(base_name + u'.' + suffix);
    for (quint64 index = 1; QFileInfo::exists(candidate); ++index) {
        candidate =
            save_directory.filePath(base_name + u'-' + QString::number(index) + u'.' + suffix);
    }
    return QUrl::fromLocalFile(candidate);
}

QUrl DocumentPathAdapter::localFileUrl(const QString& path) const {
    return path.isEmpty() ? QUrl{} : QUrl::fromLocalFile(path);
}

QString DocumentPathAdapter::localFilePath(const QUrl& url) const {
    return url.isLocalFile() ? url.toLocalFile() : QString{};
}

} // namespace caret
