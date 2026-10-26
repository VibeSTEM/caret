// 文档 pt 与 Qt 逻辑像素的边界换算，结果按百分之一逻辑像素传递。
#pragma once

#include <QObject>
#include <QtGlobal>
#include <QtQml/qqmlregistration.h>
#include "canvas_limits.hpp"

namespace caret {

namespace document_units {
inline constexpr int kPointsPerInch = 72;
inline constexpr int kLogicalPixelsPerInch = 96;
inline constexpr int kCentipixelsPerLogicalPixel = 100;
inline constexpr int kPercentPerFactor = 100;

constexpr qreal logical_pixels_per_point() {
    return static_cast<qreal>(kLogicalPixelsPerInch) / kPointsPerInch;
}
} // namespace document_units

/// 为 QML 和 C++ 提供相同的 pt 到整数百分之一逻辑像素换算。
class DocumentUnits final : public QObject {
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON
    Q_PROPERTY(int minimumZoomPercent READ minimumZoomPercent CONSTANT)
    Q_PROPERTY(int maximumZoomPercent READ maximumZoomPercent CONSTANT)
    Q_PROPERTY(int zoomStepPercent READ zoomStepPercent CONSTANT)

  public:
    using QObject::QObject;

    [[nodiscard]] int minimumZoomPercent() const { return canvas_limits::kMinimumZoomPercent; }
    [[nodiscard]] int maximumZoomPercent() const { return canvas_limits::kMaximumZoomPercent; }
    [[nodiscard]] int zoomStepPercent() const { return canvas_limits::kZoomStepPercent; }

    /// zoom_percent 以 100 表示 100%；无效输入返回 0。
    Q_INVOKABLE qint64 pointsToLogicalCentipixels(qreal points, int zoom_percent) const;
    /// 把整数百分比转换为 Qt 几何 API 所需的浮点倍率。
    Q_INVOKABLE qreal percentToFactor(int value) const;
    /// 把 Qt 几何 API 的浮点倍率转换为整数百分比。
    Q_INVOKABLE int factorToPercent(qreal value) const;
    /// 把整数百分之一逻辑像素转换为 Qt 几何 API 所需的逻辑像素。
    Q_INVOKABLE qreal centipixelsToLogicalPixels(qint64 value) const;
};

} // namespace caret
