//! 使用 ICU4X 和 Unicode 分段库计算 Caret 固定版本的字素与行断边界。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Range;

use icu_locale_core::{LanguageIdentifier, langid};
use icu_segmenter::{LineSegmenter, LineSegmenterBorrowed, options::LineBreakOptions};

thread_local! {
    static LINE_SEGMENTER: RefCell<LineSegmenterBorrowed<'static>> = RefCell::new({
        LineSegmenter::new_17_for_non_complex_scripts(Default::default())
    });
    static CHINESE_LINE_SEGMENTER: RefCell<LineSegmenterBorrowed<'static>> = RefCell::new({
        let language: LanguageIdentifier = langid!("zh");
        let mut options = LineBreakOptions::default();
        options.strictness = Some(icu_segmenter::options::LineBreakStrictness::Normal);
        options.content_locale = Some(&language);
        LineSegmenter::new_17_for_non_complex_scripts(options)
    });
    static DICTIONARY_LINE_SEGMENTER: RefCell<LineSegmenterBorrowed<'static>> = RefCell::new({
        let mut segmenter = LineSegmenter::new_17_for_non_complex_scripts(Default::default());
        segmenter.load_dictionary();
        segmenter
    });
}

/// 由固定 Unicode 17 数据计算的段落字素与行断边界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnicodeBoundaries {
    /// 扩展字素簇起止位置，使用 UTF-8 字节偏移，含 0 与字符串长度。
    pub grapheme: Vec<usize>,
    /// UAX #14 行断机会，使用 UTF-8 字节偏移，含 0 与字符串长度。
    pub line_break: Vec<usize>,
}

impl UnicodeBoundaries {
    /// 对原文计算 Unicode 17 字素和行断边界，不规范化输入。
    pub fn new(text: &str) -> Self {
        Self::new_with_language(text, None)
    }

    /// 按文档语言计算边界；泰语、老挝语、高棉语和缅甸语使用 ICU4X 内置词典。
    pub fn new_with_language(text: &str, language: Option<&str>) -> Self {
        let mut grapheme = Vec::new();
        let mut line_break = Vec::new();
        let dictionary_ranges = if language.is_some_and(uses_dictionary) && !text.is_empty() {
            std::iter::once(0..text.len()).collect()
        } else {
            Vec::new()
        };
        let chinese_ranges = if language.is_some_and(is_chinese) && !text.is_empty() {
            std::iter::once(0..text.len()).collect()
        } else {
            Vec::new()
        };
        Self::fill(
            text,
            &mut grapheme,
            &mut line_break,
            &dictionary_ranges,
            &chinese_ranges,
        );
        Self {
            grapheme,
            line_break,
        }
    }

    pub(super) fn fill(
        text: &str,
        grapheme: &mut Vec<usize>,
        line_break: &mut Vec<usize>,
        dictionary_ranges: &[Range<usize>],
        chinese_ranges: &[Range<usize>],
    ) {
        use unicode_segmentation::UnicodeSegmentation;

        grapheme.clear();
        grapheme.push(0);
        grapheme.extend(
            text.grapheme_indices(true)
                .skip(1)
                .map(|(offset, _)| offset),
        );
        if grapheme.last().copied() != Some(text.len()) {
            grapheme.push(text.len());
        }
        line_break.clear();
        let line_break_text = replace_complex_context_for_line_break(text);
        let mut generic_breaks = Vec::new();
        LINE_SEGMENTER.with(|segmenter| {
            generic_breaks.extend(segmenter.borrow().segment_str(&line_break_text));
        });
        if chinese_ranges.is_empty() {
            line_break.extend(generic_breaks);
        } else {
            let mut chinese_breaks = Vec::new();
            CHINESE_LINE_SEGMENTER.with(|segmenter| {
                chinese_breaks.extend(segmenter.borrow().segment_str(&line_break_text));
            });
            line_break.extend(
                generic_breaks
                    .into_iter()
                    .filter(|boundary| !is_inside_dictionary_range(*boundary, chinese_ranges)),
            );
            line_break.extend(chinese_breaks.into_iter().filter(|boundary| {
                is_inside_dictionary_range(*boundary, chinese_ranges)
                    && grapheme.binary_search(boundary).is_ok()
            }));
            line_break.sort_unstable();
            line_break.dedup();
        }
        if !dictionary_ranges.is_empty() {
            line_break.retain(|boundary| !is_inside_dictionary_range(*boundary, dictionary_ranges));
            let mut dictionary_breaks = Vec::new();
            DICTIONARY_LINE_SEGMENTER.with(|segmenter| {
                dictionary_breaks.extend(segmenter.borrow().segment_str(text));
            });
            line_break.extend(dictionary_breaks.into_iter().filter(|boundary| {
                is_inside_dictionary_range(*boundary, dictionary_ranges)
                    && grapheme.binary_search(boundary).is_ok()
            }));
            line_break.sort_unstable();
            line_break.dedup();
        }
        line_break.retain(|boundary| {
            chinese_range_at_boundary(*boundary, chinese_ranges).is_none_or(|range| {
                is_mandatory_break(text, *boundary)
                    || !violates_chinese_kinsoku(text, *boundary, range)
            })
        });
        if line_break.first().copied() != Some(0) {
            line_break.insert(0, 0);
        }
        if line_break.last().copied() != Some(text.len()) {
            line_break.push(text.len());
        }
    }
}

/// UAX #14 强制断点不能被中文行首/行尾禁则删除；CRLF 中间仍不是断点。
fn is_mandatory_break(text: &str, boundary: usize) -> bool {
    use icu_properties::{CodePointMapData, props::LineBreak};

    if boundary == 0 || boundary >= text.len() || !text.is_char_boundary(boundary) {
        return false;
    }
    let Some(previous) = text[..boundary].chars().next_back() else {
        return false;
    };
    let Some(next) = text[boundary..].chars().next() else {
        return false;
    };
    if previous == '\r' && next == '\n' {
        return false;
    }
    is_mandatory_line_break(CodePointMapData::<LineBreak>::new().get(previous))
}

/// 判断 ICU 行断类别是否属于 UAX #14 强制断行。
pub(crate) fn is_mandatory_line_break(value: icu_properties::props::LineBreak) -> bool {
    use icu_properties::props::LineBreak;
    matches!(
        value,
        LineBreak::MandatoryBreak
            | LineBreak::CarriageReturn
            | LineBreak::LineFeed
            | LineBreak::NextLine
    )
}

/// 在有序、不重叠的词典语言范围中二分查询断点归属。
fn is_inside_dictionary_range(boundary: usize, ranges: &[Range<usize>]) -> bool {
    let index = ranges.partition_point(|range| range.end <= boundary);
    ranges
        .get(index)
        .is_some_and(|range| range.start < boundary && boundary < range.end)
}

fn chinese_range_at_boundary(boundary: usize, ranges: &[Range<usize>]) -> Option<&Range<usize>> {
    let index = ranges.partition_point(|range| range.end <= boundary);
    ranges
        .get(index)
        .filter(|range| range.start < boundary && boundary < range.end)
}

/// 按 UAX #14 LB1 将 SA 映射为 CM 或 AL，且保持每个码点的 UTF-8 长度以复用偏移。
fn replace_complex_context_for_line_break(text: &str) -> Cow<'_, str> {
    use icu_properties::{
        CodePointMapData,
        props::{GeneralCategory, LineBreak},
    };

    let line_break = CodePointMapData::<LineBreak>::new();
    if !text
        .chars()
        .any(|character| line_break.get(character) == LineBreak::ComplexContext)
    {
        return Cow::Borrowed(text);
    }
    let general_category = CodePointMapData::<GeneralCategory>::new();
    Cow::Owned(
        text.chars()
            .map(|character| {
                if line_break.get(character) != LineBreak::ComplexContext {
                    return character;
                }
                let is_mark = matches!(
                    general_category.get(character),
                    GeneralCategory::NonspacingMark | GeneralCategory::SpacingMark
                );
                match (is_mark, character.len_utf8()) {
                    (true, 2) => '\u{0300}',
                    (true, 3) => '\u{081c}',
                    (true, 4) => '\u{101fd}',
                    (false, 2) => 'À',
                    (false, 3) => '\u{0800}',
                    (false, 4) => '\u{10000}',
                    _ => character,
                }
            })
            .collect(),
    )
}

pub(super) fn uses_dictionary(language: &str) -> bool {
    dictionary_language(language).is_some()
}

pub(super) fn dictionary_language(language: &str) -> Option<&str> {
    let primary = language.split('-').next()?;
    ["th", "lo", "km", "my"]
        .into_iter()
        .any(|candidate| primary.eq_ignore_ascii_case(candidate))
        .then_some(primary)
}

pub(super) fn is_chinese(language: &str) -> bool {
    language
        .split('-')
        .next()
        .unwrap_or_default()
        .eq_ignore_ascii_case("zh")
}

fn violates_chinese_kinsoku(text: &str, boundary: usize, range: &Range<usize>) -> bool {
    if boundary == 0 || boundary >= text.len() || boundary < range.start || boundary > range.end {
        return false;
    }
    let Some(previous) = text[..boundary].chars().next_back() else {
        return false;
    };
    let Some(next) = text[boundary..].chars().next() else {
        return false;
    };
    let previous_start = boundary - previous.len_utf8();
    let next_end = boundary + next.len_utf8();
    (previous_start >= range.start && is_chinese_line_end_prohibited(previous))
        || (next_end <= range.end && is_chinese_line_start_prohibited(next))
}

fn is_chinese_line_start_prohibited(character: char) -> bool {
    matches!(
        character,
        '，' | '。'
            | '、'
            | '；'
            | '：'
            | '？'
            | '！'
            | '）'
            | '］'
            | '｝'
            | '》'
            | '」'
            | '』'
            | '】'
            | '〕'
            | '〉'
            | '’'
            | '”'
    )
}

fn is_chinese_line_end_prohibited(character: char) -> bool {
    matches!(
        character,
        '（' | '［' | '｛' | '《' | '「' | '『' | '【' | '〔' | '〈' | '“' | '‘'
    )
}
