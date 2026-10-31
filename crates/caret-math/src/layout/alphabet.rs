//! 数学字母表映射：把拉丁字母、希腊字母与数字映射到 Unicode 数学字母数字符号块。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::context::Variant;

/// 按变体映射字符；无对应字形块的字符原样返回。
pub(crate) fn styled_char(character: char, variant: Variant) -> char {
    use Variant as V;
    let latin = |upper: u32, lower: u32| offset_latin(character, upper, lower);
    let mapped = match variant {
        V::Roman => None,
        V::Normal => {
            if character.is_ascii_alphabetic() {
                italic_latin(character)
            } else {
                greek(character, None, Some(0x1D6FC))
            }
        }
        V::Italic => {
            italic_latin(character).or_else(|| greek(character, Some(0x1D6E2), Some(0x1D6FC)))
        }
        V::Bold => latin(0x1D400, 0x1D41A)
            .or_else(|| digit(character, 0x1D7CE))
            .or_else(|| greek(character, Some(0x1D6A8), Some(0x1D6C2))),
        V::BoldItalic => latin(0x1D468, 0x1D482)
            .or_else(|| digit(character, 0x1D7CE))
            .or_else(|| greek(character, Some(0x1D71C), Some(0x1D736))),
        V::SansSerif => latin(0x1D5A0, 0x1D5BA).or_else(|| digit(character, 0x1D7E2)),
        V::Monospace => latin(0x1D670, 0x1D68A).or_else(|| digit(character, 0x1D7F6)),
        V::DoubleStruck => exception(
            character,
            &[
                ('C', '\u{2102}'),
                ('H', '\u{210D}'),
                ('N', '\u{2115}'),
                ('P', '\u{2119}'),
                ('Q', '\u{211A}'),
                ('R', '\u{211D}'),
                ('Z', '\u{2124}'),
            ],
        )
        .or_else(|| latin(0x1D538, 0x1D552))
        .or_else(|| digit(character, 0x1D7D8)),
        V::Script => exception(
            character,
            &[
                ('B', '\u{212C}'),
                ('E', '\u{2130}'),
                ('F', '\u{2131}'),
                ('H', '\u{210B}'),
                ('I', '\u{2110}'),
                ('L', '\u{2112}'),
                ('M', '\u{2133}'),
                ('R', '\u{211B}'),
                ('e', '\u{212F}'),
                ('g', '\u{210A}'),
                ('o', '\u{2134}'),
            ],
        )
        .or_else(|| latin(0x1D49C, 0x1D4B6)),
        V::Fraktur => exception(
            character,
            &[
                ('C', '\u{212D}'),
                ('H', '\u{210C}'),
                ('I', '\u{2111}'),
                ('R', '\u{211C}'),
                ('Z', '\u{2128}'),
            ],
        )
        .or_else(|| latin(0x1D504, 0x1D51E)),
    };
    mapped.unwrap_or(character)
}

fn italic_latin(character: char) -> Option<char> {
    // 斜体小写 h 位于字母型符号块（U+210E）。
    if character == 'h' {
        return Some('\u{210E}');
    }
    offset_latin(character, 0x1D434, 0x1D44E)
}

fn offset_latin(character: char, upper: u32, lower: u32) -> Option<char> {
    let code = match character {
        'A'..='Z' => upper + (character as u32 - 'A' as u32),
        'a'..='z' => lower + (character as u32 - 'a' as u32),
        _ => return None,
    };
    char::from_u32(code)
}

fn digit(character: char, base: u32) -> Option<char> {
    character
        .to_digit(10)
        .and_then(|value| char::from_u32(base + value))
}

/// 希腊字母：大写 Α–Ω（U+0391–U+03A9）与小写 α–ω（U+03B1–U+03C9）连续映射。
fn greek(character: char, upper: Option<u32>, lower: Option<u32>) -> Option<char> {
    let code = character as u32;
    match code {
        0x0391..=0x03A9 => upper.and_then(|base| char::from_u32(base + code - 0x0391)),
        0x03B1..=0x03C9 => lower.and_then(|base| char::from_u32(base + code - 0x03B1)),
        _ => None,
    }
}

fn exception(character: char, table: &[(char, char)]) -> Option<char> {
    table
        .iter()
        .find(|(from, _)| *from == character)
        .map(|(_, to)| *to)
}
