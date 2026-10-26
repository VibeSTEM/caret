// 画布交互范围及其整数单位，供画布实现与 ViewModel 共用。
#pragma once

namespace caret::canvas_limits {

inline constexpr int kMinimumZoomPercent = 10;
inline constexpr int kMaximumZoomPercent = 800;
inline constexpr int kZoomStepPercent = 10;

} // namespace caret::canvas_limits
