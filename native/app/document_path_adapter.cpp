#include "document_path_adapter.hpp"

#include <QDir>
#include <QStandardPaths>
#include <QUrl>

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

} // namespace caret
