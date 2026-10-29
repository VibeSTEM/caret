// 验证标题栏指标对窗口替换、生命周期与系统几何事件的通知行为。
#include "shell_load_test.hpp"
#include "window_chrome_metrics.hpp"
#include <QCoreApplication>
#include <QEvent>
#include <QQuickWindow>
#include <QSignalSpy>
#include <QTest>
#include <QtTest/qtestcase.h>
#include <memory>

void ShellLoadTest::windowChromeMetricsTracksWindowLifecycleAndRefreshEvents() {
    caret::WindowChromeMetrics metrics;
    QSignalSpy leading_spy(&metrics,
                           &caret::WindowChromeMetrics::leadingSystemControlsInsetChanged);
    QSignalSpy trailing_spy(&metrics,
                            &caret::WindowChromeMetrics::trailingSystemControlsInsetChanged);
    QVERIFY(leading_spy.isValid());
    QVERIFY(trailing_spy.isValid());
    QCOMPARE(metrics.leadingSystemControlsInset(), 0.0);
    QCOMPARE(metrics.trailingSystemControlsInset(), 0.0);
    metrics.performTitlebarDoubleClickAction();

    auto first_window = std::make_unique<QQuickWindow>();
    metrics.observeWindow(first_window.get());
    QCOMPARE(leading_spy.size(), 1);
    QCOMPARE(trailing_spy.size(), 1);
    metrics.observeWindow(first_window.get());
    QCOMPARE(leading_spy.size(), 1);
    QCOMPARE(trailing_spy.size(), 1);

    QEvent expose(QEvent::Expose);
    (void)QCoreApplication::sendEvent(first_window.get(), &expose);
    QCOMPARE(leading_spy.size(), 2);
    QCOMPARE(trailing_spy.size(), 2);
    QTRY_VERIFY_WITH_TIMEOUT(leading_spy.size() >= 3, 1000);
    QTRY_VERIFY_WITH_TIMEOUT(trailing_spy.size() >= 3, 1000);

    auto second_window = std::make_unique<QQuickWindow>();
    metrics.observeWindow(second_window.get());
    QTRY_VERIFY_WITH_TIMEOUT(leading_spy.size() >= 4, 1000);
    QTRY_VERIFY_WITH_TIMEOUT(trailing_spy.size() >= 4, 1000);
    const qsizetype leading_after_replace = leading_spy.size();
    const qsizetype trailing_after_replace = trailing_spy.size();
    QEvent show(QEvent::Show);
    (void)QCoreApplication::sendEvent(first_window.get(), &show);
    QCOMPARE(leading_spy.size(), leading_after_replace);
    QCOMPARE(trailing_spy.size(), trailing_after_replace);

    second_window.reset();
    QCOMPARE(metrics.leadingSystemControlsInset(), 0.0);
    QCOMPARE(metrics.trailingSystemControlsInset(), 0.0);
    QVERIFY(leading_spy.size() > leading_after_replace);
    QVERIFY(trailing_spy.size() > trailing_after_replace);
    metrics.observeWindow(nullptr);
}
