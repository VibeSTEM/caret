//! 展平段落行内结构、解析格式环境并计算 Caret 的 Unicode 映射输入。

use caret_doc::{InlineEnvironmentKind, InlineNode, Paragraph};

mod unicode;
pub use unicode::UnicodeBoundaries;

/// 传入 Parley 的段落基础样式；格式环境和 Run 语言在行内范围上覆盖它。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapingStyle {
    pub requested_family: Option<String>,
    pub font_size_centipoints: u16,
    pub bold: bool,
    pub italic: bool,
    pub language: Option<String>,
}

impl ShapingStyle {
    /// 创建基础排版样式；默认字号由文档设置提供，必须显式传入。
    pub const fn new(font_size_centipoints: u16) -> Self {
        Self {
            requested_family: None,
            font_size_centipoints,
            bold: false,
            italic: false,
            language: None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct StyledSpan<'a> {
    pub(crate) text: &'a str,
    pub(crate) style: ShapingStyle,
}

pub(crate) fn paragraph_style_spans<'a>(
    paragraph: &'a Paragraph,
    settings: &crate::ResolvedDocumentTextSettings,
    base_style: &ShapingStyle,
) -> Vec<StyledSpan<'a>> {
    let mut base = base_style.clone();
    base.language = paragraph.language.clone().or_else(|| {
        settings
            .default_language()
            .map(str::to_owned)
            .or_else(|| base_style.language.clone())
    });
    let mut spans = Vec::new();
    append_inlines(&paragraph.inlines, &base, &mut spans);
    spans
}

pub(crate) fn boundaries_for_spans(text: &str, spans: &[StyledSpan<'_>]) -> UnicodeBoundaries {
    let mut dictionary_ranges = Vec::new();
    let mut chinese_ranges = Vec::new();
    let mut byte_offset = 0usize;
    for span in spans {
        let end = byte_offset + span.text.len();
        if !span.text.is_empty()
            && span
                .style
                .language
                .as_deref()
                .is_some_and(unicode::uses_dictionary)
        {
            dictionary_ranges.push(byte_offset..end);
        }
        if !span.text.is_empty()
            && span
                .style
                .language
                .as_deref()
                .is_some_and(unicode::is_chinese)
        {
            chinese_ranges.push(byte_offset..end);
        }
        byte_offset = end;
    }
    let mut result = UnicodeBoundaries {
        grapheme: Vec::new(),
        line_break: Vec::new(),
    };
    if byte_offset != text.len() {
        return UnicodeBoundaries::new(text);
    }
    UnicodeBoundaries::fill(
        text,
        &mut result.grapheme,
        &mut result.line_break,
        &dictionary_ranges,
        &chinese_ranges,
    );
    result
}

/// 返回段落逻辑 UTF-8 文本；格式环境不插入字符，软换行和不透明节点各占一个码点。
pub fn paragraph_text(paragraph: &Paragraph) -> String {
    fn append(nodes: &[InlineNode], text: &mut String) {
        for node in nodes {
            match node {
                InlineNode::Run(run) => text.push_str(&run.text),
                InlineNode::Environment(environment) => append(&environment.children, text),
                InlineNode::SoftBreak(_) => text.push('\n'),
                InlineNode::Opaque(_) => text.push('\u{fffc}'),
            }
        }
    }
    let mut text = String::new();
    append(&paragraph.inlines, &mut text);
    text
}

fn append_inlines<'a>(
    nodes: &'a [InlineNode],
    style: &ShapingStyle,
    output: &mut Vec<StyledSpan<'a>>,
) {
    for node in nodes {
        match node {
            InlineNode::Run(run) => {
                let mut run_style = style.clone();
                if let Some(language) = &run.language {
                    run_style.language = Some(language.clone());
                }
                if !run.text.is_empty() {
                    output.push(StyledSpan {
                        text: &run.text,
                        style: run_style,
                    });
                }
            }
            InlineNode::Environment(environment) => {
                let mut nested = style.clone();
                match environment.kind {
                    InlineEnvironmentKind::Bold => {
                        nested.bold = environment
                            .props
                            .get("enabled")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(true);
                    }
                    InlineEnvironmentKind::Italic => {
                        nested.italic = environment
                            .props
                            .get("enabled")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(true);
                    }
                    InlineEnvironmentKind::Font => {
                        if let Some(family) = environment
                            .props
                            .get("fontFamily")
                            .and_then(serde_json::Value::as_str)
                        {
                            nested.requested_family = Some(family.to_owned());
                        }
                    }
                }
                append_inlines(&environment.children, &nested, output);
            }
            InlineNode::SoftBreak(_) => output.push(StyledSpan {
                text: "\n",
                style: style.clone(),
            }),
            InlineNode::Opaque(_) => output.push(StyledSpan {
                text: "\u{fffc}",
                style: style.clone(),
            }),
        }
    }
}
