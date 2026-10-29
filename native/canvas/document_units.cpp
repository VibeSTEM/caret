// 将页面尺寸从 pt 转换为 Qt 逻辑像素的整数百分之一。
#include "document_units.hpp"

#include <cmath>
#include <limits>

namespace caret {

qint64 DocumentUnits::pointsToLogicalCentipixels(qreal points, int zoom_percent) const {
    if (!std::isfinite(points) || points < 0.0 || zoom_percent < 0) {
        return 0;
    }
    const qreal scaled = points * zoom_percent * document_units::kLogicalPixelsPerInch /
                         document_units::kPointsPerInch;
    if (!std::isfinite(scaled) ||
        scaled >= static_cast<qreal>(std::numeric_limits<qint64>::max())) {
        return std::numeric_limits<qint64>::max();
    }
    return qRound64(scaled);
}

qint64 DocumentUnits::centipointsToLogicalCentipixels(qint64 centipoints, int zoom_percent) const {
    if (centipoints < 0 || zoom_percent < 0) {
        return 0;
    }
    const qreal scaled = static_cast<qreal>(centipoints) * zoom_percent *
                         document_units::kLogicalPixelsPerInch /
                         (document_units::kCentipointsPerPoint * document_units::kPointsPerInch);
    if (!std::isfinite(scaled) ||
        scaled >= static_cast<qreal>(std::numeric_limits<qint64>::max())) {
        return std::numeric_limits<qint64>::max();
    }
    return qRound64(scaled);
}

qreal DocumentUnits::percentToFactor(int value) const {
    return document_units::percent_to_factor(value);
}

int DocumentUnits::factorToPercent(qreal value) const {
    if (!std::isfinite(value)) {
        return 0;
    }
    const qreal percentage = value * document_units::kPercentPerFactor;
    if (percentage <= std::numeric_limits<int>::min() ||
        percentage >= std::numeric_limits<int>::max()) {
        return 0;
    }
    return qRound(percentage);
}

qreal DocumentUnits::centipixelsToLogicalPixels(qint64 value) const {
    return document_units::centipixels_to_logical_pixels(value);
}

} // namespace caret
