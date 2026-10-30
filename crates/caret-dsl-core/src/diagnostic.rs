//! 共享结构化诊断及源码位置；不依赖具体 grammar 或输出格式。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::fmt::{Display, Formatter};
use std::ops::Deref;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    /// 原始源码中的 UTF-8 字节范围；点诊断的起止位置相同。
    pub start_offset: usize,
    pub end_offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostics {
    pub diagnostics: Vec<Diagnostic>,
}

impl Diagnostics {
    pub(crate) fn one(code: &str, message: impl Into<String>, offset: usize, source: &str) -> Self {
        Self::one_span(code, message, offset, offset, source)
    }

    pub(crate) fn one_span(
        code: &str,
        message: impl Into<String>,
        start_offset: usize,
        end_offset: usize,
        source: &str,
    ) -> Self {
        let start_offset = start_offset.min(source.len());
        let end_offset = end_offset.clamp(start_offset, source.len());
        let (line, column) = source_position(source, start_offset);
        Self {
            diagnostics: vec![Diagnostic {
                code: code.to_owned(),
                message: message.into(),
                start_offset,
                end_offset,
                line,
                column,
            }],
        }
    }
}

impl Display for Diagnostics {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                formatter.write_str("\n")?;
            }
            write!(
                formatter,
                "{} at {}:{}: {}",
                diagnostic.code, diagnostic.line, diagnostic.column, diagnostic.message
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostics {}

impl Display for Diagnostic {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} at {}:{}: {}",
            self.code, self.line, self.column, self.message
        )
    }
}

pub(crate) fn push_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    code: &str,
    message: impl Into<String>,
    offset: usize,
    source: &SourcePositions<'_>,
) {
    push_diagnostic_span(diagnostics, code, message, offset, offset, source);
}

pub(crate) fn push_diagnostic_span(
    diagnostics: &mut Vec<Diagnostic>,
    code: &str,
    message: impl Into<String>,
    start_offset: usize,
    end_offset: usize,
    source: &SourcePositions<'_>,
) {
    let start_offset = start_offset.min(source.len());
    let end_offset = end_offset.clamp(start_offset, source.len());
    let (line, column) = source.position(start_offset);
    diagnostics.push(Diagnostic {
        code: code.to_owned(),
        message: message.into(),
        start_offset,
        end_offset,
        line,
        column,
    });
}

/// 一次解析共享的位置索引；字符列同时扣除多字节 UTF-8 的额外字节数，
/// 避免大量诊断反复扫描同一行或文件前缀。
pub(crate) struct SourcePositions<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
    utf8_corrections: Vec<(usize, usize)>,
}

impl<'a> SourcePositions<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0];
        let mut utf8_corrections = Vec::new();
        let mut extra_bytes = 0;
        for (offset, character) in source.char_indices() {
            if character == '\n' {
                line_starts.push(offset + 1);
            }
            if character.len_utf8() > 1 {
                extra_bytes += character.len_utf8() - 1;
                utf8_corrections.push((offset + character.len_utf8(), extra_bytes));
            }
        }
        Self {
            source,
            line_starts,
            utf8_corrections,
        }
    }

    fn position(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.source.len());
        let line_index = self.line_starts.partition_point(|start| *start <= offset) - 1;
        let line_start = self.line_starts[line_index];
        let extra_bytes = self.extra_bytes_before(offset) - self.extra_bytes_before(line_start);
        (line_index + 1, offset - line_start - extra_bytes + 1)
    }

    fn extra_bytes_before(&self, offset: usize) -> usize {
        let count = self
            .utf8_corrections
            .partition_point(|(end, _)| *end <= offset);
        count
            .checked_sub(1)
            .map_or(0, |index| self.utf8_corrections[index].1)
    }
}

impl Deref for SourcePositions<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.source
    }
}

pub(crate) fn source_position(source: &str, offset: usize) -> (usize, usize) {
    let clamped = offset.min(source.len());
    let before = &source[..clamped];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(1, |last| last.chars().count() + 1);
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::{SourcePositions, source_position};

    #[test]
    fn indexed_positions_match_character_columns_at_all_utf8_boundaries() {
        for source in ["", "ASCII\n", "首行😀\r\nsecond 中e\u{301}\n末尾"] {
            let positions = SourcePositions::new(source);
            for offset in source
                .char_indices()
                .map(|(offset, _)| offset)
                .chain([source.len(), usize::MAX])
            {
                assert_eq!(positions.position(offset), source_position(source, offset));
            }
        }
    }
}
