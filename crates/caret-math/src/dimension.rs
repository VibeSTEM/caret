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
}
