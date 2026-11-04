// 验证文档默认保存位置与本地文件 URL 的边界转换。
#include "document_path_adapter.hpp"
#include "shell_load_test.hpp"
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>
#include <QtCore/qstring.h>
#include <QtTest/qtestcase.h>

void ShellLoadTest::documentPathAdapterCreatesDefaultSaveLocation() {
    QTemporaryDir documents;
    QVERIFY(documents.isValid());
    caret::DocumentPathAdapter adapter;
    const QString caret_directory = QDir(documents.path()).filePath(QStringLiteral("caret"));
    QVERIFY(!QDir(caret_directory).exists());

    const QUrl default_url = adapter.defaultSaveFileUrlIn(documents.path());
    QVERIFY(default_url.isLocalFile());
    const QFileInfo target(default_url.toLocalFile());
    QCOMPARE(target.path(), QDir(caret_directory).absolutePath());
    QVERIFY(target.fileName().startsWith(QStringLiteral("Untitled")));
    QCOMPARE(target.suffix(), QStringLiteral("cdoc"));
    QVERIFY(QDir(caret_directory).exists());

    QFile occupied(QDir(caret_directory).filePath(QStringLiteral("Untitled.cdoc")));
    QVERIFY(occupied.open(QIODevice::WriteOnly));
    occupied.close();
    const QUrl next_url = adapter.defaultSaveFileUrlIn(documents.path());
    QCOMPARE(QFileInfo(next_url.toLocalFile()).fileName(), QStringLiteral("Untitled-1.cdoc"));
    QVERIFY(!adapter.defaultSaveFileUrlIn(QString{}).isValid());
}

void ShellLoadTest::documentPathAdapterConvertsLocalPaths() {
    caret::DocumentPathAdapter adapter;
    const QString local_path = QDir(QDir::tempPath()).filePath(QStringLiteral("Caret 文档.cdoc"));
    QCOMPARE(adapter.localFileUrl(local_path), QUrl::fromLocalFile(local_path));
    QVERIFY(!adapter.localFileUrl(QString{}).isValid());
    QCOMPARE(adapter.localFilePath(QUrl::fromLocalFile(local_path)), local_path);
    QVERIFY(
        adapter.localFilePath(QUrl(QStringLiteral("https://example.invalid/file.cdoc"))).isEmpty());
}
