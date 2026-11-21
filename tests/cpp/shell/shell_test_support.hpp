// Shell 集成测试共用的初始化、运行时设置与应用窗口夹具。
#pragma once

#include <QColor>
#include <QObject>
#include <QString>
#include <QVariantMap>

#ifdef CARET_ENABLE_APP
#include <QQmlApplicationEngine>
#include <QQuickWindow>
#include <QTemporaryDir>
#include <memory>

namespace caret {
class LayoutViewModel;
}
#endif

namespace caret::shell_test {

QVariantMap snapshot_from_theme_facade(const QString& path, const QColor& background, int body);

} // namespace caret::shell_test

class ShellTestBase : public QObject {
    Q_OBJECT

  protected slots:
    void initTestCase();

#ifdef CARET_ENABLE_APP
  protected:
    static bool configureDocumentRecovery(const QString& recovery_root);
    static bool installBlankDocumentRuntime();
#endif
};

#ifdef CARET_ENABLE_APP
class AppFixture final {
  public:
    bool initialize(const QString& recovery_root, QString* error);
    bool saveTo(const QString& path);
    bool insert(const QString& text);
    QObject* child(const QString& name) const;
    bool click(const QString& name) const;

    QTemporaryDir directory;
    std::unique_ptr<QQmlApplicationEngine> engine;
    QQuickWindow* window = nullptr;
    caret::LayoutViewModel* view_model = nullptr;
};

// 等待窗口启动时的恢复扫描及其文件活动全部结束，避免把扫描结果误当成后续操作。
bool waitForStartupScan(const AppFixture& fixture);
#endif
