//! 定义文档持久化字号使用的 centipoints 与 points 转换。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

/// 文档字号与 Rust 排版共用的单位换算。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DocumentUnits;

impl DocumentUnits {
    const CENTIPOINTS_PER_POINT: f64 = 100.0;

    /// 将百分之一 pt 转为 pt。
    pub fn centipoints_to_points(centipoints: u32) -> f64 {
        f64::from(centipoints) / Self::CENTIPOINTS_PER_POINT
    }

    /// 将 Rust 排版字号转回文档的 u16 百分之一 pt 表示。
    pub fn points_to_font_size_centipoints(points: f32) -> Option<u16> {
        let centipoints = f64::from(points) * Self::CENTIPOINTS_PER_POINT;
        if !centipoints.is_finite() || centipoints < 1.0 || centipoints > f64::from(u16::MAX) {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "字号已校验并四舍五入到 u16 范围"
        )]
        u16::try_from(centipoints.round() as u32).ok()
    }

    /// 文档 `u16` 字号可表示的最大 pt 值。
    pub fn max_font_size_points() -> f64 {
        Self::centipoints_to_points(u32::from(u16::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::DocumentUnits;

    #[test]
    fn font_size_conversions_share_the_document_centipoint_scale() {
        assert_eq!(DocumentUnits::centipoints_to_points(1200), 12.0);
        assert_eq!(DocumentUnits::centipoints_to_points(1234), 12.34);
        assert_eq!(DocumentUnits::max_font_size_points(), 655.35);
        assert_eq!(
            DocumentUnits::points_to_font_size_centipoints(12.34),
            Some(1234)
        );
    }

    #[test]
    fn font_size_conversion_rejects_non_finite_and_unrepresentable_values() {
        for points in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.0,
            -1.0,
            655.36,
        ] {
            assert_eq!(DocumentUnits::points_to_font_size_centipoints(points), None);
        }
    }
}
