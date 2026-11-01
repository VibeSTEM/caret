//! 展平段落行内结构、解析格式环境并计算 Caret 的 Unicode 映射输入。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{InlineEnvironmentKind, InlineNode, Paragraph};

mod unicode;
pub use unicode::UnicodeBoundaries;
pub(crate) use unicode::is_mandatory_line_break;

/// 传入 Parley 的段落基础样式；格式环境和 Run 语言在行内范围上覆盖它。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapingStyle {
    pub requested_family: Option<String>,
    /// 代码内容请求文档代码字体；普通正文按 Script 字体规则选择。
    pub font_role: FontRole,
    pub font_size_centipoints: u16,
    pub bold: bool,
    pub italic: bool,
    pub language: Option<String>,
}

/// 文字的语义字体角色；代码块等宽策略由布局层统一解析。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontRole {
    #[default]
    Body,
    Code,
}

impl ShapingStyle {
    /// 创建基础排版样式；默认字号由文档设置提供，必须显式传入。
    pub const fn new(font_size_centipoints: u16) -> Self {
        Self {
            requested_family: None,
            font_role: FontRole::Body,
            font_size_centipoints,
            bold: false,
            italic: false,
            language: None,
        }
    }

    /// 创建使用文档等宽代码字体的基础塑形样式。
    pub const fn code(font_size_centipoints: u16) -> Self {
        Self {
            requested_family: None,
            font_role: FontRole::Code,
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
    pub(crate) underline: bool,
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
    append_inlines(&paragraph.inlines, &base, false, &mut spans);
    spans
}

pub(crate) fn boundaries_for_spans(text: &str, spans: &[StyledSpan<'_>]) -> UnicodeBoundaries {
    let mut dictionary_ranges = Vec::new();
    let mut chinese_ranges = Vec::new();
    let mut previous_dictionary_language: Option<&str> = None;
    let mut previous_span_is_chinese = false;
    let mut byte_offset = 0usize;
    for span in spans {
        let end = byte_offset + span.text.len();
        if !span.text.is_empty() {
            let dictionary_language = span
                .style
                .language
                .as_deref()
                .and_then(unicode::dictionary_language);
            if let Some(language) = dictionary_language {
                append_coalesced_range(
                    &mut dictionary_ranges,
                    byte_offset,
                    end,
                    previous_dictionary_language
                        .is_some_and(|previous| previous.eq_ignore_ascii_case(language)),
                );
            }
            previous_dictionary_language = dictionary_language;

            let span_is_chinese = span
                .style
                .language
                .as_deref()
                .is_some_and(unicode::is_chinese);
            if span_is_chinese {
                append_coalesced_range(
                    &mut chinese_ranges,
                    byte_offset,
                    end,
                    previous_span_is_chinese,
                );
            }
            previous_span_is_chinese = span_is_chinese;
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

fn append_coalesced_range(
    ranges: &mut Vec<std::ops::Range<usize>>,
    start: usize,
    end: usize,
    merge: bool,
) {
    if merge
        && let Some(previous) = ranges.last_mut()
        && previous.end == start
    {
        previous.end = end;
    } else {
        ranges.push(start..end);
    }
}

/// 返回段落逻辑 UTF-8 文本；格式环境不插入字符，软换行和不透明节点各占一个码点。
pub fn paragraph_text(paragraph: &Paragraph) -> String {
    fn append(nodes: &[InlineNode], text: &mut String) {
        for node in nodes {
            match node {
                InlineNode::Run(run) => text.push_str(&run.text),
                InlineNode::Environment(environment) => append(&environment.children, text),
                InlineNode::SoftBreak(_) => text.push('\n'),
                InlineNode::Opaque(_) | InlineNode::Math(_) => text.push('\u{fffc}'),
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
    underline: bool,
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
                        underline,
                    });
                }
            }
            InlineNode::Environment(environment) => {
                let mut nested = style.clone();
                let mut nested_underline = underline;
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
                    InlineEnvironmentKind::Underline => {
                        nested_underline = environment
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
                append_inlines(&environment.children, &nested, nested_underline, output);
            }
            InlineNode::SoftBreak(_) => output.push(StyledSpan {
                text: "\n",
                style: style.clone(),
                underline: false,
            }),
            InlineNode::Opaque(_) | InlineNode::Math(_) => output.push(StyledSpan {
                text: "\u{fffc}",
                style: style.clone(),
                underline: false,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use caret_doc::{DocumentSettings, InlineEnvironment, NodeId, OpaqueNode, Run, SoftBreak};

    use super::*;
    use crate::ResolvedDocumentTextSettings;

    fn run(text: &str, language: Option<&str>) -> InlineNode {
        InlineNode::Run(Run {
            text: text.to_owned(),
            language: language.map(str::to_owned),
            props: BTreeMap::new(),
            extra: BTreeMap::new(),
        })
    }

    #[test]
    fn logical_text_keeps_breaks_and_opaque_placeholders_inside_nested_environments() {
        let mut paragraph = Paragraph::empty(NodeId::generate());
        paragraph.inlines = vec![
            run("e", None),
            InlineNode::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Bold,
                definition_id: None,
                props: BTreeMap::new(),
                children: vec![
                    run("\u{301}", None),
                    InlineNode::SoftBreak(SoftBreak::default()),
                    InlineNode::Opaque(OpaqueNode {
                        raw: serde_json::json!({"type": "future_object", "text": "hidden"}),
                    }),
                ],
                extra: BTreeMap::new(),
            }),
        ];

        assert_eq!(paragraph_text(&paragraph), "e\u{301}\n\u{fffc}");
        assert!(!crate::paragraph_has_empty_line(&paragraph));
    }

    #[test]
    fn nested_disabled_format_and_run_language_override_only_their_local_span() {
        let settings = ResolvedDocumentTextSettings::from_document_settings(&DocumentSettings {
            default_language: Some("zh-Hant".to_owned()),
            ..DocumentSettings::default()
        })
        .unwrap_or_else(|error| panic!("settings rejected: {error}"));
        let mut paragraph = Paragraph::empty(NodeId::generate());
        paragraph.inlines = vec![
            run("outer", None),
            InlineNode::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Italic,
                definition_id: None,
                props: BTreeMap::from([("enabled".to_owned(), serde_json::json!(false))]),
                children: vec![run("local", Some("en-GB"))],
                extra: BTreeMap::new(),
            }),
            run("restored", None),
        ];
        let spans = paragraph_style_spans(
            &paragraph,
            &settings,
            &ShapingStyle {
                italic: true,
                bold: true,
                language: Some("fr".to_owned()),
                ..ShapingStyle::new(1200)
            },
        );

        assert_eq!(spans.len(), 3);
        assert!(spans.iter().all(|span| span.style.bold));
        assert!(spans[0].style.italic);
        assert!(!spans[1].style.italic);
        assert!(spans[2].style.italic);
        assert_eq!(spans[0].style.language.as_deref(), Some("zh-Hant"));
        assert_eq!(spans[1].style.language.as_deref(), Some("en-GB"));
        assert_eq!(spans[2].style.language.as_deref(), Some("zh-Hant"));
    }

    #[test]
    fn inconsistent_span_lengths_fall_back_to_the_complete_logical_text() {
        let text = "e\u{301}中文";
        let spans = [StyledSpan {
            text: "e",
            style: ShapingStyle::new(1200),
            underline: false,
        }];

        assert_eq!(
            boundaries_for_spans(text, &spans),
            UnicodeBoundaries::new(text)
        );
    }

    #[test]
    fn code_font_role_is_preserved_through_paragraph_style_inheritance() {
        let settings =
            ResolvedDocumentTextSettings::from_document_settings(&DocumentSettings::default())
                .unwrap_or_else(|error| panic!("settings rejected: {error}"));
        let mut paragraph = Paragraph::empty(NodeId::generate());
        paragraph.inlines = vec![run("let answer = 42;", None)];

        let spans = paragraph_style_spans(&paragraph, &settings, &ShapingStyle::code(1100));

        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.font_role, FontRole::Code);
        assert_eq!(spans[0].style.font_size_centipoints, 1100);
    }
}
