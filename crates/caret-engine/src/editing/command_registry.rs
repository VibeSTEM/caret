//! 维护命令别名、稳定 ID、显示名称和当前可执行状态。

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
        available: false,
    },
    RegisteredCandidate {
        alias: "\\equation",
        command_id: "insert.equation",
        label: "Single-line equation",
        available: false,
    },
    RegisteredCandidate {
        alias: "\\align",
        command_id: "insert.align",
        label: "Multiline equation",
        available: false,
    },
    RegisteredCandidate {
        alias: "\\frac",
        command_id: "insert.equation.fraction",
        label: "Fraction",
        available: false,
    },
    RegisteredCandidate {
        alias: "\\sqrt",
        command_id: "insert.equation.radical",
        label: "Square root",
        available: false,
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
