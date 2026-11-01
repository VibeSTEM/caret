//! 正文与结构化行内流共用的 UTF-8 字素边界算法。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use unicode_segmentation::UnicodeSegmentation;

use super::Affinity;

/// 按显示文本计算可用于光标和选区的 Unicode 字素边界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GraphemeMap {
    boundaries: Vec<usize>,
}

impl GraphemeMap {
    /// 构建边界表；调用方先按宿主规则拼接连续的文本片段。
    pub(super) fn new(text: &str) -> Self {
        let mut boundaries = text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        boundaries.push(text.len());
        boundaries.dedup();
        Self { boundaries }
    }

    pub(super) fn into_boundaries(self) -> Vec<usize> {
        self.boundaries
    }

    pub(super) fn is_boundary_in(text: &str, offset: usize) -> bool {
        offset == text.len()
            || text
                .grapheme_indices(true)
                .any(|(start, _)| start == offset)
    }

    pub(super) fn snap_boundaries(
        boundaries: &[usize],
        offset: usize,
        affinity: Affinity,
    ) -> usize {
        match boundaries.binary_search(&offset) {
            Ok(_) => offset,
            Err(index) => match affinity {
                Affinity::Upstream => boundaries
                    .get(index.saturating_sub(1))
                    .copied()
                    .unwrap_or(0),
                Affinity::Downstream => boundaries
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| boundaries.last().copied().unwrap_or(0)),
            },
        }
    }

    pub(super) fn previous_boundary(boundaries: &[usize], offset: usize) -> Option<usize> {
        let index = boundaries.binary_search(&offset).ok()?;
        index
            .checked_sub(1)
            .and_then(|previous| boundaries.get(previous).copied())
    }

    pub(super) fn next_boundary(boundaries: &[usize], offset: usize) -> Option<usize> {
        let index = boundaries.binary_search(&offset).ok()?;
        boundaries.get(index + 1).copied()
    }

    pub(super) fn previous_in(text: &str, offset: usize) -> Option<usize> {
        text.get(..offset)?
            .grapheme_indices(true)
            .next_back()
            .map(|(start, _)| start)
    }

    pub(super) fn next_in(text: &str, offset: usize) -> Option<usize> {
        text.get(offset..)?
            .graphemes(true)
            .next()
            .map(|grapheme| offset + grapheme.len())
    }

    pub(super) fn snap_in(text: &str, offset: usize, affinity: Affinity) -> Option<usize> {
        if offset > text.len() || !text.is_char_boundary(offset) {
            return None;
        }
        if Self::is_boundary_in(text, offset) {
            return Some(offset);
        }
        match affinity {
            Affinity::Upstream => text
                .grapheme_indices(true)
                .map(|(start, _)| start)
                .take_while(|boundary| *boundary < offset)
                .last(),
            Affinity::Downstream => text
                .grapheme_indices(true)
                .map(|(start, grapheme)| start + grapheme.len())
                .find(|boundary| *boundary > offset),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_map_keeps_combining_and_emoji_clusters_atomic() {
        let text = "a\u{301}👩🏽‍💻🇺🇳";
        let map = GraphemeMap::new(text);
        let combining_end = "a\u{301}".len();
        let emoji_end = combining_end + "👩🏽‍💻".len();

        assert_eq!(map.boundaries, [0, combining_end, emoji_end, text.len()]);
        assert!(!GraphemeMap::is_boundary_in(text, "a".len()));
        assert_eq!(
            GraphemeMap::snap_boundaries(&map.boundaries, "a".len(), Affinity::Upstream),
            0
        );
        assert_eq!(
            GraphemeMap::snap_boundaries(&map.boundaries, "a".len(), Affinity::Downstream),
            combining_end
        );
        assert_eq!(
            GraphemeMap::previous_boundary(&map.boundaries, emoji_end),
            Some(combining_end)
        );
        assert_eq!(
            GraphemeMap::next_boundary(&map.boundaries, combining_end),
            Some(emoji_end)
        );
    }
}
