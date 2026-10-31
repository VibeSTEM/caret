// 验证文档默认保存位置与本地文件 URL 的边界转换。
#include "document_path_adapter.hpp"
#include "shell_load_test.hpp"
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QIODevice>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>
#include <QtCore/qstring.h>
#include <QtTest/qtestcase.h>

void ShellLoadTest::documentPathAdapterReturnsLocalPathsAndDefaultSaveLocation() {
    QTemporaryDir temporary_directory;
    QVERIFY(temporary_directory.isValid());
    const QString documents = temporary_directory.filePath(QStringLiteral("Documents"));
    QVERIFY(QDir().mkpath(documents));
    const QString caret_directory = QDir(documents).filePath(QStringLiteral("caret"));

    QVERIFY(
        !caret::DocumentPathAdapter::defaultSaveFileUrlForDocumentsLocation(QString{}).isValid());
    const QUrl default_url =
        caret::DocumentPathAdapter::defaultSaveFileUrlForDocumentsLocation(documents);
    QVERIFY(default_url.isLocalFile());
    const QFileInfo target(default_url.toLocalFile());
    QCOMPARE(target.path(), QDir(caret_directory).absolutePath());
    QCOMPARE(target.fileName(), QStringLiteral("Untitled.cdoc"));
    QVERIFY(QDir(caret_directory).exists());

    const QString first_candidate = QDir(caret_directory).filePath(QStringLiteral("Untitled.cdoc"));
    QFile collision(first_candidate);
    QVERIFY(collision.open(QIODevice::WriteOnly));
    collision.close();
    const QUrl collision_url =
        caret::DocumentPathAdapter::defaultSaveFileUrlForDocumentsLocation(documents);
    QVERIFY(collision_url.isLocalFile());
    QCOMPARE(QFileInfo(collision_url.toLocalFile()).fileName(), QStringLiteral("Untitled-1.cdoc"));

    QVERIFY(QFile::remove(first_candidate));
    QVERIFY(QDir().rmdir(caret_directory));
    QFile blocker(caret_directory);
    QVERIFY(blocker.open(QIODevice::WriteOnly));
    blocker.close();
    QVERIFY(
        !caret::DocumentPathAdapter::defaultSaveFileUrlForDocumentsLocation(documents).isValid());

    caret::DocumentPathAdapter adapter;
    const QString local_path = QDir(QDir::tempPath()).filePath(QStringLiteral("Caret 文档.cdoc"));
    QCOMPARE(adapter.localFileUrl(local_path), QUrl::fromLocalFile(local_path));
    QVERIFY(!adapter.localFileUrl(QString{}).isValid());
    QCOMPARE(adapter.localFilePath(QUrl::fromLocalFile(local_path)), local_path);
    QVERIFY(
        adapter.localFilePath(QUrl(QStringLiteral("https://example.invalid/file.cdoc"))).isEmpty());
}
