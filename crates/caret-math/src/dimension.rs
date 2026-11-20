//! 解析和序列化数学 em/mu TeX 小数长度。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

/// 解析 `em` 或 `mu` 长度为有理数 em。
pub(crate) fn parse_em(raw: &str) -> Option<(i128, i128)> {
    let raw = raw.trim();
    let (number, unit) = raw.split_at(raw.find(|c: char| c.is_ascii_alphabetic())?);
    let unit = unit.trim();
    let number = number.trim();
    let (negative, digits) = match number.strip_prefix('-') {
        Some(rest) => (true, rest.trim()),
        None => (false, number.trim_prefix('+').trim()),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() && fraction.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
        || whole.len() + fraction.len() > 18
    {
        return None;
    }
    let mut numerator: i128 = format!("{whole}{fraction}").parse().ok()?;
    let mut denominator: i128 = 10i128.pow(fraction.len() as u32);
    match unit {
        "em" => {}
        "mu" => denominator *= 18,
        _ => return None,
    }
    if negative {
        numerator = -numerator;
    }
    Some(reduce((numerator, denominator)))
}

/// 两个分数相加并约分。
pub(crate) fn add_ratio(left: (i128, i128), right: (i128, i128)) -> (i128, i128) {
    reduce((left.0 * right.1 + right.0 * left.1, left.1 * right.1))
}

/// 有理数长度写成 TeX 小数：优先保留有限小数 em，其次有限小数 mu，否则保留 em。
pub(crate) fn format_dimension((numerator, denominator): (i128, i128)) -> String {
    if is_finite_decimal(denominator) {
        return format!("{}em", decimal((numerator, denominator)));
    }
    let mu = (numerator * 18, denominator);
    if is_finite_decimal(mu.1 / gcd(mu.0, mu.1)) {
        return format!("{}mu", decimal(mu));
    }
    format!("{}em", decimal((numerator, denominator)))
}

/// 与 [`format_dimension`] 相同的规范表示选择，但所有算术使用 checked 形式，
/// 并在结果超出当前解析器 18 位精度预算时返回 `None`。检查式片段序列化用它
/// 判定一个合法模型能否产生可再读取的 TeX；未受检查的旧序列化路径保持原行为。
pub(crate) fn format_dimension_checked((numerator, denominator): (i128, i128)) -> Option<String> {
    if denominator <= 0 {
        return None;
    }
    if is_finite_decimal(denominator) {
        let text = decimal_checked((numerator, denominator))?;
        let candidate = format!("{text}em");
        return parse_em(&candidate).is_some().then_some(candidate);
    }
    let mu_numerator = numerator.checked_mul(18)?;
    let divisor = gcd_unsigned(mu_numerator.unsigned_abs(), denominator.unsigned_abs());
    let mu_denominator = denominator / i128::try_from(divisor).ok()?;
    if is_finite_decimal(mu_denominator) {
        let text = decimal_checked((mu_numerator, denominator))?;
        let candidate = format!("{text}mu");
        // 与 format_dimension 的表示选择严格一致：mu 分支不可回读即损失，
        // 不能 fallthrough 到 em 候选而让检查与实际写出分叉。
        return parse_em(&candidate).is_some().then_some(candidate);
    }
    let text = decimal_checked((numerator, denominator))?;
    let candidate = format!("{text}em");
    parse_em(&candidate).is_some().then_some(candidate)
}

fn gcd_unsigned(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// [`decimal`] 的 checked 版本；超过 18 位小数或算术溢出时返回 `None`。
fn decimal_checked((numerator, denominator): (i128, i128)) -> Option<String> {
    let negative = (numerator < 0) != (denominator < 0);
    let (numerator, denominator) = (numerator.unsigned_abs(), denominator.unsigned_abs().max(1));
    let whole = numerator / denominator;
    let mut remainder = numerator % denominator;
    let mut digits = String::new();
    let mut steps = 0;
    while remainder != 0 && steps < 18 {
        remainder = remainder.checked_mul(10)?;
        digits.push(char::from(b'0' + (remainder / denominator) as u8));
        remainder %= denominator;
        steps += 1;
    }
    let sign = if negative && (whole != 0 || !digits.is_empty()) {
        "-"
    } else {
        ""
    };
    if digits.is_empty() {
        Some(format!("{sign}{whole}"))
    } else {
        Some(format!("{sign}{whole}.{digits}"))
    }
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

fn reduce((numerator, denominator): (i128, i128)) -> (i128, i128) {
    let divisor = gcd(numerator, denominator);
    (numerator / divisor, denominator / divisor)
}

fn is_finite_decimal(mut denominator: i128) -> bool {
    denominator = denominator.abs().max(1);
    for factor in [2, 5] {
        while denominator % factor == 0 {
            denominator /= factor;
        }
    }
    denominator == 1
}

/// 有理数写成小数，最多输出 18 位小数，不做舍入。
fn decimal((numerator, denominator): (i128, i128)) -> String {
    let negative = (numerator < 0) != (denominator < 0);
    let (numerator, denominator) = (numerator.unsigned_abs(), denominator.unsigned_abs().max(1));
    let whole = numerator / denominator;
    let mut remainder = numerator % denominator;
    let mut digits = String::new();
    let mut steps = 0;
    while remainder != 0 && steps < 18 {
        remainder *= 10;
        digits.push(char::from(b'0' + (remainder / denominator) as u8));
        remainder %= denominator;
        steps += 1;
    }
    let sign = if negative && (whole != 0 || !digits.is_empty()) {
        "-"
    } else {
        ""
    };
    if digits.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{digits}")
    }
}

#[cfg(test)]
mod tests {
    use super::{format_dimension, parse_em};

    #[test]
    fn parses_decimal_em_and_mu_as_reduced_em_ratios() {
        assert_eq!(parse_em(" 1.5em "), Some((3, 2)));
        assert_eq!(parse_em("3mu"), Some((1, 6)));
        assert_eq!(parse_em("-.5mu"), Some((-1, 36)));
    }

    #[test]
    fn rejects_unsupported_units_and_more_than_eighteen_digits() {
        assert_eq!(parse_em("2pt"), None);
        assert_eq!(parse_em("0.1234567890123456789em"), None);
    }

    #[test]
    fn preserves_existing_em_mu_selection_and_decimal_truncation() {
        assert_eq!(format_dimension((3, 6)), "9mu");
        assert_eq!(format_dimension((1, 3)), "6mu");
        assert_eq!(format_dimension((1, 7)), "0.142857142857142857em");
        assert_eq!(parse_em(&format_dimension((3, 6))), Some((1, 2)));
    }

    #[test]
    fn checked_formatter_matches_normal_output_for_parseable_dimensions() {
        use super::format_dimension_checked;
        for ratio in [
            (1, 2),
            (3, 6),
            (1, 3),
            (1, 18),
            (3, 2),
            (-3, 2),
            (0, 1),
            (123456789012345678, 1),
        ] {
            let text = format_dimension(ratio);
            assert_eq!(
                format_dimension_checked(ratio).as_deref(),
                Some(text.as_str())
            );
            assert!(parse_em(&text).is_some(), "{ratio:?}");
        }
    }

    #[test]
    fn checked_formatter_rejects_precision_budget_and_overflow() {
        use super::format_dimension_checked;
        assert_eq!(format_dimension_checked((i128::MAX, 3)), None);
        assert_eq!(
            format_dimension_checked((10_i128.pow(38) - 1, 10_i128.pow(38))),
            None
        );
        assert_eq!(format_dimension_checked((10000000000000000000, 1)), None);
        // 未约分的 mu 形式超预算时不得 fallthrough 到 em 候选。
        assert_eq!(format_dimension_checked((300000000000000000, 3)), None);
        assert_eq!(format_dimension_checked((1, 0)), None);
        assert_eq!(format_dimension_checked((1, -2)), None);
    }

    #[test]
    fn checked_formatter_never_accepts_output_that_differs_from_normal_formatting() {
        use super::{format_dimension, format_dimension_checked};
        for (numerator, denominator) in [
            (1, 2),
            (3, 6),
            (1, 3),
            (1, 18),
            (300000000000000000, 3),
            (i128::MAX, 3),
            (10_i128.pow(38) - 1, 10_i128.pow(38)),
        ] {
            if let Some(checked) = format_dimension_checked((numerator, denominator)) {
                assert_eq!(checked, format_dimension((numerator, denominator)));
                assert!(parse_em(&checked).is_some(), "{checked}");
            }
        }
    }
}
