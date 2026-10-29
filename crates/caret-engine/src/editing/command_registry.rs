//! 维护命令别名、稳定 ID、显示名称和当前可执行状态。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandCandidate {
    pub alias: &'static str,
    pub command_id: &'static str,
    pub label: &'static str,
    pub available: bool,
}

const CANDIDATES: &[CommandCandidate] = &[
    CommandCandidate {
        alias: "\\chapter",
        command_id: "para.heading",
        label: "Chapter",
        available: false,
    },
    CommandCandidate {
        alias: "\\section",
        command_id: "para.heading",
        label: "Section",
        available: false,
    },
    CommandCandidate {
        alias: "\\subsection",
        command_id: "para.heading",
        label: "Subsection",
        available: false,
    },
    CommandCandidate {
        alias: "\\subsubsection",
        command_id: "para.heading",
        label: "Subsubsection",
        available: false,
    },
    CommandCandidate {
        alias: "\\itemize",
        command_id: "para.bullets",
        label: "Bulleted list",
        available: false,
    },
    CommandCandidate {
        alias: "\\enumerate",
        command_id: "para.numbering",
        label: "Numbered list",
        available: false,
    },
    CommandCandidate {
        alias: "\\newpage",
        command_id: "page.break.insert",
        label: "Page break",
        available: false,
    },
    CommandCandidate {
        alias: "\\math",
        command_id: "insert.inline_math",
        label: "Inline math",
        available: false,
    },
    CommandCandidate {
        alias: "\\equation",
        command_id: "insert.equation",
        label: "Single-line equation",
        available: false,
    },
    CommandCandidate {
        alias: "\\align",
        command_id: "insert.align",
        label: "Multiline equation",
        available: false,
    },
    CommandCandidate {
        alias: "\\frac",
        command_id: "insert.equation.fraction",
        label: "Fraction",
        available: false,
    },
    CommandCandidate {
        alias: "\\sqrt",
        command_id: "insert.equation.radical",
        label: "Square root",
        available: false,
    },
    CommandCandidate {
        alias: "\\table",
        command_id: "insert.table",
        label: "Table",
        available: false,
    },
    CommandCandidate {
        alias: "\\newline",
        command_id: "text.soft_break",
        label: "Line break",
        available: true,
    },
    CommandCandidate {
        alias: "\\newparagraph",
        command_id: "text.split_paragraph",
        label: "New paragraph",
        available: true,
    },
];

pub const fn candidates() -> &'static [CommandCandidate] {
    CANDIDATES
}
