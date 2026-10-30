// 校验画布几何和跨层快照中的有限数值与布尔类型。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

#pragma once

#include <QMetaType>
#include <QVariant>
#include <QtGlobal>
#include <cmath>

namespace caret::canvas_detail {

inline bool is_finite_positive(qreal value) { return std::isfinite(value) && value > 0.0; }
inline bool is_finite_non_negative(qreal value) { return std::isfinite(value) && value >= 0.0; }
inline bool is_boolean_variant(const QVariant& value) {
    return value.metaType().id() == QMetaType::Bool;
}

} // namespace caret::canvas_detail
