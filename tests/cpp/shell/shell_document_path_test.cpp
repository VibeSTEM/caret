// 验证文档默认保存位置与本地文件 URL 的边界转换。
#include "document_path_adapter.hpp"
#include "shell_load_test.hpp"
#include <QDir>
#include <QFileInfo>
#include <QStandardPaths>
#include <QTest>
#include <QUrl>
#include <QtCore/qstring.h>
#include <QtTest/qtestcase.h>

void ShellLoadTest::documentPathAdapterReturnsLocalPathsAndDefaultSaveLocation() {
    const QString documents = QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation);
    if (documents.isEmpty()) {
        QSKIP("当前平台未提供 Documents 目录");
    }
    const QString caret_directory = QDir(documents).filePath(QStringLiteral("caret"));
    const bool documents_existed = QDir(documents).exists();
    const bool caret_directory_existed = QDir(caret_directory).exists();
    struct CreatedDirectoryCleanup {
        QString documents;
        QString directory;
        bool remove_documents = false;
        bool remove_directory = false;
        ~CreatedDirectoryCleanup() {
            if (remove_directory) {
                (void)QDir().rmdir(directory);
            }
            if (remove_documents) {
                (void)QDir().rmdir(documents);
            }
        }
    } cleanup{documents, caret_directory, !documents_existed, !caret_directory_existed};

    caret::DocumentPathAdapter adapter;
    const QUrl default_url = adapter.defaultSaveFileUrl();
    QVERIFY(default_url.isLocalFile());
    const QFileInfo target(default_url.toLocalFile());
    QCOMPARE(target.path(), QDir(caret_directory).absolutePath());
    QVERIFY(target.fileName().startsWith(QStringLiteral("Untitled")));
    QCOMPARE(target.suffix(), QStringLiteral("cdoc"));
    QVERIFY(QDir(caret_directory).exists());

    const QString local_path = QDir(QDir::tempPath()).filePath(QStringLiteral("Caret 文档.cdoc"));
    QCOMPARE(adapter.localFileUrl(local_path), QUrl::fromLocalFile(local_path));
    QVERIFY(!adapter.localFileUrl(QString{}).isValid());
    QCOMPARE(adapter.localFilePath(QUrl::fromLocalFile(local_path)), local_path);
    QVERIFY(
        adapter.localFilePath(QUrl(QStringLiteral("https://example.invalid/file.cdoc"))).isEmpty());
}
