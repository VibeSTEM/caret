//! 维护命令别名、稳定 ID、显示名称和当前可执行状态。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::borrow::Cow;

use super::command::command_ids;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCandidate {
    pub alias: Cow<'static, str>,
    pub command_id: &'static str,
    pub label: Cow<'static, str>,
    pub available: bool,
    pub value: Option<String>,
    pub has_children: bool,
}

#[derive(Debug, Clone, Copy)]
struct RegisteredCandidate {
    alias: &'static str,
    command_id: &'static str,
    label: &'static str,
    available: bool,
}

impl RegisteredCandidate {
    fn to_candidate(self) -> CommandCandidate {
        CommandCandidate {
            alias: Cow::Borrowed(self.alias),
            command_id: self.command_id,
            label: Cow::Borrowed(self.label),
            available: self.available,
            value: None,
            has_children: false,
        }
    }
}

const CANDIDATES: &[RegisteredCandidate] = &[
    RegisteredCandidate {
        alias: "\\chapter",
        command_id: "para.heading",
        label: "Chapter",
        available: false,
    },
    RegisteredCandidate {
        alias: "\\section",
        command_id: command_ids::STRUCT_SECTION_CREATE,
        label: "Section",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\subsection",
        command_id: "para.heading",
        label: "Subsection",
        available: false,
    },
    RegisteredCandidate {
        alias: "\\subsubsection",
        command_id: "para.heading",
        label: "Subsubsection",
        available: false,
    },
    RegisteredCandidate {
        alias: "\\itemize",
        command_id: "para.bullets",
        label: "Bulleted list",
        available: false,
    },
    RegisteredCandidate {
        alias: "\\enumerate",
        command_id: "para.numbering",
        label: "Numbered list",
        available: false,
    },
    RegisteredCandidate {
        alias: "\\newpage",
        command_id: "page.break.insert",
        label: "Page break",
        available: false,
    },
    RegisteredCandidate {
        alias: "\\math",
        command_id: command_ids::INSERT_INLINE_MATH,
        label: "Inline math",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\equation",
        command_id: command_ids::INSERT_EQUATION,
        label: "Single-line equation",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\align",
        command_id: command_ids::INSERT_ALIGN,
        label: "Multiline equation",
        available: true,
    },
    // 046 布局完成前不暴露表格入口：命令本身在 Session 已可用，但 \table
    // 候选与 native 键位保持禁用，避免插入不可见/不可排版的表格。
    RegisteredCandidate {
        alias: "\\table",
        command_id: command_ids::INSERT_TABLE,
        label: "Table",
        available: false,
    },
    RegisteredCandidate {
        alias: "\\newline",
        command_id: command_ids::TEXT_SOFT_BREAK,
        label: "Line break",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\newparagraph",
        command_id: command_ids::TEXT_SPLIT_PARAGRAPH,
        label: "New paragraph",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\singlecharspace",
        command_id: command_ids::TEXT_INSERT_ONE_CHARACTER_WIDTH_SPACE,
        label: "One-character-width space",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\doublecharspace",
        command_id: command_ids::TEXT_INSERT_TWO_CHARACTER_WIDTH_SPACE,
        label: "Two-character-width space",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\firstlineindent",
        command_id: command_ids::PARAGRAPH_FIRST_LINE_INDENT_SET,
        label: "First-line indent",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\bold",
        command_id: command_ids::TEXT_FORMAT_BOLD,
        label: "Bold",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\italic",
        command_id: command_ids::TEXT_FORMAT_ITALIC,
        label: "Italic",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\underline",
        command_id: command_ids::TEXT_FORMAT_UNDERLINE,
        label: "Underline",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\font",
        command_id: command_ids::TEXT_FORMAT_FONT,
        label: "Font family",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\defaultfont",
        command_id: "document.default_font",
        label: "Default font",
        available: true,
    },
];

pub fn candidates() -> Vec<CommandCandidate> {
    CANDIDATES
        .iter()
        .copied()
        .map(RegisteredCandidate::to_candidate)
        .collect()
}

/// 公式编辑中 `\` 子命令的稳定 ID；候选的 `value` 是插入到公式源码的 LaTeX 模板。
pub const MATH_INSERT_COMMAND_ID: &str = "math.insert";
/// 在数学 TextFlow 命令面板中插入或拆分为 InlineMath 的稳定 ID。
pub const MATH_SPLIT_INLINE_COMMAND_ID: &str = "math.split_inline";

/// 公式内子命令：别名、显示名称与模板；模板中的第一个 `{}` 是插入后的光标位置。
const MATH_CANDIDATES: &[(&str, &str, &str)] = &[
    ("\\frac", "Fraction", "\\frac{}{}"),
    ("\\sqrt", "Square root", "\\sqrt{}"),
    ("\\text", "Text", "\\text{}"),
    ("\\superscript", "Superscript", "^{}"),
    ("\\subscript", "Subscript", "_{}"),
    ("\\sum", "Summation", "\\sum"),
    ("\\int", "Integral", "\\int"),
    ("\\infty", "Infinity", "\\infty"),
    ("\\alpha", "Greek alpha", "\\alpha"),
    ("\\beta", "Greek beta", "\\beta"),
    ("\\gamma", "Greek gamma", "\\gamma"),
    ("\\delta", "Greek delta", "\\delta"),
    ("\\epsilon", "Greek epsilon", "\\epsilon"),
    ("\\zeta", "Greek zeta", "\\zeta"),
    ("\\eta", "Greek eta", "\\eta"),
    ("\\theta", "Greek theta", "\\theta"),
    ("\\iota", "Greek iota", "\\iota"),
    ("\\kappa", "Greek kappa", "\\kappa"),
    ("\\lambda", "Greek lambda", "\\lambda"),
    ("\\mu", "Greek mu", "\\mu"),
    ("\\nu", "Greek nu", "\\nu"),
    ("\\xi", "Greek xi", "\\xi"),
    ("\\pi", "Greek pi", "\\pi"),
    ("\\rho", "Greek rho", "\\rho"),
    ("\\sigma", "Greek sigma", "\\sigma"),
    ("\\tau", "Greek tau", "\\tau"),
    ("\\upsilon", "Greek upsilon", "\\upsilon"),
    ("\\phi", "Greek phi", "\\phi"),
    ("\\chi", "Greek chi", "\\chi"),
    ("\\psi", "Greek psi", "\\psi"),
    ("\\omega", "Greek omega", "\\omega"),
    ("\\Gamma", "Greek capital Gamma", "\\Gamma"),
    ("\\Delta", "Greek capital Delta", "\\Delta"),
    ("\\Theta", "Greek capital Theta", "\\Theta"),
    ("\\Lambda", "Greek capital Lambda", "\\Lambda"),
    ("\\Xi", "Greek capital Xi", "\\Xi"),
    ("\\Pi", "Greek capital Pi", "\\Pi"),
    ("\\Sigma", "Greek capital Sigma", "\\Sigma"),
    ("\\Upsilon", "Greek capital Upsilon", "\\Upsilon"),
    ("\\Phi", "Greek capital Phi", "\\Phi"),
    ("\\Psi", "Greek capital Psi", "\\Psi"),
    ("\\Omega", "Greek capital Omega", "\\Omega"),
];

/// 公式编辑状态下 `\` 命令行列出的子命令。
pub fn math_candidates() -> Vec<CommandCandidate> {
    let mut candidates = candidates()
        .into_iter()
        .filter(|candidate| {
            matches!(
                candidate.command_id,
                command_ids::TEXT_FORMAT_BOLD
                    | command_ids::TEXT_FORMAT_ITALIC
                    | command_ids::TEXT_FORMAT_UNDERLINE
            )
        })
        .collect::<Vec<_>>();
    candidates.extend(
        MATH_CANDIDATES
            .iter()
            .map(|(alias, label, template)| CommandCandidate {
                alias: Cow::Borrowed(alias),
                command_id: MATH_INSERT_COMMAND_ID,
                label: Cow::Borrowed(label),
                available: true,
                value: Some((*template).to_owned()),
                has_children: false,
            }),
    );
    candidates.insert(
        0,
        CommandCandidate {
            alias: Cow::Borrowed("\\math"),
            command_id: MATH_SPLIT_INLINE_COMMAND_ID,
            label: Cow::Borrowed("Inline math"),
            available: true,
            value: None,
            has_children: false,
        },
    );
    candidates
}

#[cfg(test)]
mod tests {
    use super::{CANDIDATES, MATH_CANDIDATES, MATH_INSERT_COMMAND_ID, candidates, math_candidates};
    use std::collections::BTreeSet;

    #[test]
    fn command_completion_translation_sources_match_the_rust_registry() {
        let qml = include_str!("../../../../qml/Canvas/CommandSuggestionPopup.qml");
        let start_index = qml.find("function translationSourcesForLupdateOnly() {");
        assert!(
            start_index.is_some(),
            "generated translation sources should exist"
        );
        let start = start_index.unwrap_or_default();
        let end_index = qml[start..]
            .find("function labelFor(candidate)")
            .map(|offset| start + offset);
        assert!(
            end_index.is_some(),
            "candidate label renderer should follow translation sources"
        );
        let end = end_index.unwrap_or(start);
        let marker = "qsTranslate(\"CommandCompletion\", \"";
        let actual = qml[start..end]
            .split(marker)
            .skip(1)
            .filter_map(|tail| tail.split_once("\")").map(|(label, _)| label))
            .collect::<BTreeSet<_>>();
        let expected = CANDIDATES
            .iter()
            .map(|candidate| candidate.label)
            .chain(MATH_CANDIDATES.iter().map(|(_, label, _)| *label))
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn table_command_is_registered_but_disabled_until_046() {
        let table = candidates()
            .into_iter()
            .find(|candidate| candidate.alias == "\\table")
            .unwrap_or_else(|| panic!("table candidate must be registered"));
        assert_eq!(
            table.command_id,
            super::super::command::command_ids::INSERT_TABLE
        );
        assert!(
            !table.available,
            "table candidate stays disabled until 046 connects the canvas"
        );
    }

    #[test]
    fn math_text_command_is_registered_as_an_insertable_text_island() {
        let candidates = math_candidates();
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.alias == "\\text")
        );
        for candidate in candidates
            .iter()
            .filter(|candidate| candidate.alias == "\\text")
        {
            assert_eq!(candidate.command_id, MATH_INSERT_COMMAND_ID);
            assert_eq!(candidate.value.as_deref(), Some("\\text{}"));
            assert!(candidate.available);
        }
    }
}
