//! 展平段落行内结构、解析格式环境并计算 Caret 的 Unicode 映射输入。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use caret_doc::{FlowContent, Paragraph, ParagraphContent, TextFlow};
pub use caret_font_roles::FontRole;

mod unicode;
pub use unicode::UnicodeBoundaries;
pub(crate) use unicode::{is_mandatory_break_character, is_mandatory_line_break};

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

    /// 应用 Run 自身支持的 shaping 覆盖；局部字体仍由 Font 环境表达。
    pub fn with_run(&self, run: &caret_doc::Run) -> Self {
        let mut style = self.clone();
        if let Some(language) = &run.language {
            style.language = Some(language.clone());
        }
        style
    }
}

/// 返回 Run 声明的下划线样式；正文与数学 TextFlow 使用同一属性解释。
pub fn run_has_underline(run: &caret_doc::Run) -> bool {
    run.props
        .get("underline")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
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
    append_inlines(
        &paragraph.inlines,
        &base,
        settings.inline_environment_templates(),
        &mut spans,
    );
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
    fn append(nodes: &[ParagraphContent], text: &mut String) {
        for node in nodes {
            match node {
                FlowContent::Text(flow) => {
                    for run in &flow.runs {
                        text.push_str(&run.text);
                    }
                }
                FlowContent::Environment(environment) => append(&environment.children, text),
                FlowContent::SoftBreak(_) => text.push('\n'),
                FlowContent::Opaque(_) | FlowContent::Math(_) => text.push('\u{fffc}'),
            }
        }
    }
    let mut text = String::new();
    append(&paragraph.inlines, &mut text);
    text
}

fn append_inlines<'a>(
    nodes: &'a [ParagraphContent],
    style: &ShapingStyle,
    templates: &caret_doc::InlineEnvironmentTemplates,
    output: &mut Vec<StyledSpan<'a>>,
) {
    for node in nodes {
        match node {
            FlowContent::Text(TextFlow { runs, .. }) => {
                for run in runs {
                    if !run.text.is_empty() {
                        output.push(StyledSpan {
                            text: &run.text,
                            style: style.with_run(run),
                            underline: run_has_underline(run),
                        });
                    }
                }
            }
            FlowContent::Environment(environment) => {
                let mut nested = style.clone();
                let text_style = environment
                    .resolved_text_style(templates)
                    .unwrap_or_else(|_| environment.text_style());
                if let Some(bold) = text_style.bold {
                    nested.bold = bold;
                }
                if let Some(italic) = text_style.italic {
                    nested.italic = italic;
                }
                if let Some(font_family) = text_style.font_family {
                    nested.requested_family = Some(font_family);
                }
                append_inlines(&environment.children, &nested, templates, output);
            }
            FlowContent::SoftBreak(_) => output.push(StyledSpan {
                text: "\n",
                style: style.clone(),
                underline: false,
            }),
            FlowContent::Opaque(_) | FlowContent::Math(_) => output.push(StyledSpan {
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

    use caret_doc::{
        DocumentSettings, InlineEnvironment, InlineEnvironmentKind, NodeId, OpaqueNode,
        ParagraphContent, Run, SoftBreak, TextFlow, ThemeSnapshot,
    };

    use super::*;
    use crate::ResolvedDocumentTextSettings;

    fn run(text: &str, language: Option<&str>) -> ParagraphContent {
        ParagraphContent::Text(TextFlow {
            runs: vec![Run {
                text: text.to_owned(),
                language: language.map(str::to_owned),
                props: BTreeMap::new(),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        })
    }

    #[test]
    fn logical_text_keeps_breaks_and_opaque_placeholders_inside_nested_environments() {
        let mut paragraph = Paragraph::empty(NodeId::generate());
        paragraph.inlines = vec![
            run("e", None),
            ParagraphContent::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Bold,
                definition_id: None,
                props: BTreeMap::new(),
                children: vec![
                    run("\u{301}", None),
                    ParagraphContent::SoftBreak(SoftBreak::default()),
                    ParagraphContent::Opaque(OpaqueNode {
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
            ParagraphContent::Environment(InlineEnvironment {
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
    fn theme_font_template_variant_and_instance_override_reach_body_shaping() {
        let settings = ResolvedDocumentTextSettings::from_document_settings(&DocumentSettings {
            theme: Some(ThemeSnapshot {
                format_version: 1,
                source_id: None,
                named_styles: BTreeMap::new(),
                environment_templates: BTreeMap::from([(
                    "body-font".to_owned(),
                    serde_json::json!({
                        "baseKind": "font",
                        "props": {"fontFamily": "Theme Sans"},
                        "variants": {
                            "body-font/serif": {"fontFamily": "Theme Serif"}
                        }
                    }),
                )]),
                default_inline_environment_templates: BTreeMap::from([(
                    "font".to_owned(),
                    serde_json::json!("body-font"),
                )]),
                extra: BTreeMap::new(),
            }),
            ..DocumentSettings::default()
        })
        .unwrap_or_else(|error| panic!("settings rejected: {error}"));
        let mut paragraph = Paragraph::empty(NodeId::generate());
        paragraph.inlines = vec![
            ParagraphContent::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Font,
                definition_id: None,
                props: BTreeMap::new(),
                children: vec![
                    run("outer", None),
                    ParagraphContent::Environment(InlineEnvironment {
                        id: NodeId::generate(),
                        kind: InlineEnvironmentKind::Font,
                        definition_id: Some("body-font/serif".to_owned()),
                        props: BTreeMap::new(),
                        children: vec![run("variant", None)],
                        extra: BTreeMap::new(),
                    }),
                    run("tail", None),
                ],
                extra: BTreeMap::new(),
            }),
            ParagraphContent::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Font,
                definition_id: Some("body-font/serif".to_owned()),
                props: BTreeMap::from([(
                    "fontFamily".to_owned(),
                    serde_json::json!("Instance Mono"),
                )]),
                children: vec![run("instance", None)],
                extra: BTreeMap::new(),
            }),
        ];

        let spans = paragraph_style_spans(&paragraph, &settings, &ShapingStyle::new(1200));

        assert_eq!(spans.len(), 4);
        assert_eq!(
            spans[0].style.requested_family.as_deref(),
            Some("Theme Sans")
        );
        assert_eq!(
            spans[1].style.requested_family.as_deref(),
            Some("Theme Serif")
        );
        assert_eq!(
            spans[2].style.requested_family.as_deref(),
            Some("Theme Sans")
        );
        assert_eq!(
            spans[3].style.requested_family.as_deref(),
            Some("Instance Mono")
        );
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
