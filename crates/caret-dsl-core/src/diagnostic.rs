//! 共享结构化诊断及源码位置；不依赖具体 grammar 或输出格式。

use std::fmt::{Display, Formatter};

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
    source: &str,
) {
    push_diagnostic_span(diagnostics, code, message, offset, offset, source);
}

pub(crate) fn push_diagnostic_span(
    diagnostics: &mut Vec<Diagnostic>,
    code: &str,
    message: impl Into<String>,
    start_offset: usize,
    end_offset: usize,
    source: &str,
) {
    let start_offset = start_offset.min(source.len());
    let end_offset = end_offset.clamp(start_offset, source.len());
    let (line, column) = source_position(source, start_offset);
    diagnostics.push(Diagnostic {
        code: code.to_owned(),
        message: message.into(),
        start_offset,
        end_offset,
        line,
        column,
    });
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
