//! 维护命令别名、稳定 ID、显示名称和当前可执行状态。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::borrow::Cow;

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
        command_id: "struct.section.create",
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
        command_id: "insert.inline_math",
        label: "Inline math",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\equation",
        command_id: "insert.equation",
        label: "Single-line equation",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\align",
        command_id: "insert.align",
        label: "Multiline equation",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\table",
        command_id: "insert.table",
        label: "Table",
        available: false,
    },
    RegisteredCandidate {
        alias: "\\newline",
        command_id: "text.soft_break",
        label: "Line break",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\newparagraph",
        command_id: "text.split_paragraph",
        label: "New paragraph",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\singlecharspace",
        command_id: "text.insert.one_character_width_space",
        label: "One-character-width space",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\doublecharspace",
        command_id: "text.insert.two_character_width_space",
        label: "Two-character-width space",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\firstlineindent",
        command_id: "paragraph.first_line_indent.set",
        label: "First-line indent",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\bold",
        command_id: "text.format.bold",
        label: "Bold",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\italic",
        command_id: "text.format.italic",
        label: "Italic",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\underline",
        command_id: "text.format.underline",
        label: "Underline",
        available: true,
    },
    RegisteredCandidate {
        alias: "\\font",
        command_id: "text.format.font",
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

/// 公式内子命令：别名、显示名称与模板；模板中的第一个 `{}` 是插入后的光标位置。
const MATH_CANDIDATES: &[(&str, &str, &str)] = &[
    ("\\frac", "Fraction", "\\frac{}{}"),
    ("\\sqrt", "Square root", "\\sqrt{}"),
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
    MATH_CANDIDATES
        .iter()
        .map(|(alias, label, template)| CommandCandidate {
            alias: Cow::Borrowed(alias),
            command_id: MATH_INSERT_COMMAND_ID,
            label: Cow::Borrowed(label),
            available: true,
            value: Some((*template).to_owned()),
            has_children: false,
        })
        .collect()
}
