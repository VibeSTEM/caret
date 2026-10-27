#include "document_path_adapter.hpp"

#include <QDir>
#include <QObject>
#include <QStandardPaths>
#include <QString>
#include <QUrl>
#include <qstringliteral.h>

namespace caret {

DocumentPathAdapter::DocumentPathAdapter(QObject* parent) : QObject(parent) {}

QUrl DocumentPathAdapter::defaultSaveFileUrl() const {
    const QString documents = QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation);
    if (documents.isEmpty()) {
        return {};
    }
    const QString directory = QDir(documents).filePath(QStringLiteral("caret"));
    if (!QDir().mkpath(directory)) {
        return {};
    }
    return QUrl::fromLocalFile(QDir(directory).filePath(QStringLiteral("Untitled.cdoc")));
}

QUrl DocumentPathAdapter::localFileUrl(const QString& path) const {
    return path.isEmpty() ? QUrl{} : QUrl::fromLocalFile(path);
}

QString DocumentPathAdapter::localFilePath(const QUrl& url) const {
    return url.isLocalFile() ? url.toLocalFile() : QString{};
}

} // namespace caret
