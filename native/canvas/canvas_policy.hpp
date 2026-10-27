// 提供画布缩放交互策略，独立于文档单位换算。
#pragma once

#include <QObject>
#include <QtQml/qqmlregistration.h>

namespace caret {
namespace canvas_policy {
inline constexpr int kDefaultZoomPercent = 100;
inline constexpr int kMinimumZoomPercent = 10;
inline constexpr int kMaximumZoomPercent = 800;
inline constexpr int kZoomStepPercent = 10;
} // namespace canvas_policy

/// 向 QML 发布与画布缩放边界一致的固定交互策略。
class CanvasPolicy final : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(int minimumZoomPercent READ minimumZoomPercent CONSTANT)
    Q_PROPERTY(int defaultZoomPercent READ defaultZoomPercent CONSTANT)
    Q_PROPERTY(int maximumZoomPercent READ maximumZoomPercent CONSTANT)
    Q_PROPERTY(int zoomStepPercent READ zoomStepPercent CONSTANT)

  public:
    using QObject::QObject;

    [[nodiscard]] int minimumZoomPercent() const { return canvas_policy::kMinimumZoomPercent; }
    [[nodiscard]] int defaultZoomPercent() const { return canvas_policy::kDefaultZoomPercent; }
    [[nodiscard]] int maximumZoomPercent() const { return canvas_policy::kMaximumZoomPercent; }
    [[nodiscard]] int zoomStepPercent() const { return canvas_policy::kZoomStepPercent; }
};

} // namespace caret
