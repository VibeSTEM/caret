// 提供跨 native 入口复用的页面局部矩形数值规则。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#pragma once

#include <cmath>

namespace caret::page_rect_validation {

/// 校验坐标有限、起点非负且矩形面积为正。
[[nodiscard]] inline bool valid_coordinates(double x0, double y0, double x1, double y1) {
    return std::isfinite(x0) && std::isfinite(y0) && std::isfinite(x1) && std::isfinite(y1) &&
           x0 >= 0.0 && y0 >= 0.0 && x1 > x0 && y1 > y0;
}

/// 校验矩形未超出对应页面；拖选预览由调用方跳过此项。
[[nodiscard]] inline bool within_page(double x1, double y1, double page_width, double page_height) {
    return std::isfinite(page_width) && std::isfinite(page_height) && page_width > 0.0 &&
           page_height > 0.0 && x1 <= page_width && y1 <= page_height;
}

} // namespace caret::page_rect_validation
