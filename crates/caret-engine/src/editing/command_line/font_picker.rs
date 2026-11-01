//! 管理本机字体目录、文档字体目标及弹窗层级候选。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::CommandLineMode;
use crate::editing::command_registry::CommandCandidate;
use caret_doc::{DocumentFontTarget, is_valid_language_tag};
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FontPicker {
    families: Vec<String>,
    targets: Vec<(String, DocumentFontTarget)>,
    script: Option<String>,
    target: Option<DocumentFontTarget>,
}

impl FontPicker {
    pub(super) fn local_family(families: Vec<String>) -> Self {
        Self {
            families,
            targets: Vec::new(),
            script: None,
            target: None,
        }
    }

    pub(super) fn default_targets(
        targets: Vec<(String, DocumentFontTarget)>,
        families: Vec<String>,
    ) -> Self {
        Self {
            families,
            targets,
            script: None,
            target: None,
        }
    }

    pub(super) fn default_family(&self, target: DocumentFontTarget) -> Self {
        Self {
            families: self.families.clone(),
            targets: self.targets.clone(),
            script: self.script.clone(),
            target: Some(target),
        }
    }

    pub(super) fn set_script(&mut self, script: String) {
        self.script = Some(script);
    }

    pub(super) fn script(&self) -> String {
        self.script.clone().unwrap_or_default()
    }

    pub(super) fn return_to_targets(&mut self) -> Option<DocumentFontTarget> {
        self.target.take()
    }

    pub(super) fn take_target(&mut self) -> Option<DocumentFontTarget> {
        self.target.take()
    }

    pub(super) fn target(&self) -> Option<&DocumentFontTarget> {
        self.target.as_ref()
    }

    pub(super) fn target_prefix(&self) -> String {
        self.target
            .as_ref()
            .map_or_else(String::new, |target| match target {
                DocumentFontTarget::Script(script) => script.clone(),
                DocumentFontTarget::ScriptLanguage { script, language } => {
                    format!("{script}:{language}")
                }
            })
    }

    pub(super) fn target_for_candidate(
        &self,
        candidate: &CommandCandidate,
    ) -> Option<DocumentFontTarget> {
        if candidate.command_id == "document.default_font.script" {
            return self.script.clone().map(DocumentFontTarget::Script);
        }
        if candidate.command_id == "document.default_font.language" {
            return Some(DocumentFontTarget::ScriptLanguage {
                script: self.script.clone()?,
                language: candidate.value.clone()?,
            });
        }
        let key = candidate.value.as_deref()?;
        let script = key.strip_prefix("script:")?;
        if let Some((script, language)) = script.split_once(":language:") {
            return Some(DocumentFontTarget::ScriptLanguage {
                script: script.to_owned(),
                language: language.to_owned(),
            });
        }
        Some(DocumentFontTarget::Script(script.to_owned()))
    }

    fn target_key_for(target: &DocumentFontTarget) -> String {
        match target {
            DocumentFontTarget::Script(script) => format!("script:{script}"),
            DocumentFontTarget::ScriptLanguage { script, language } => {
                format!("script:{script}:language:{language}")
            }
        }
    }

    pub(super) fn candidates(
        &self,
        mode: CommandLineMode,
        query: &str,
        mut is_available: impl FnMut(&CommandCandidate) -> bool,
    ) -> Vec<CommandCandidate> {
        let mut candidates = match mode {
            CommandLineMode::FontFamily => self
                .families
                .iter()
                .filter(|family| family.to_lowercase().starts_with(&query.to_lowercase()))
                .map(|family| CommandCandidate {
                    alias: Cow::Owned(family.clone()),
                    command_id: "text.format.font_family",
                    label: Cow::Borrowed("Font family"),
                    available: true,
                    value: Some(family.clone()),
                    has_children: false,
                })
                .collect(),
            CommandLineMode::DefaultFontTarget => {
                let mut values = vec![close_candidate()];
                values.extend(
                    self.targets
                        .iter()
                        .filter(|(label, _)| {
                            query.is_empty() || label.to_lowercase().contains(&query.to_lowercase())
                        })
                        .map(|(label, target)| CommandCandidate {
                            alias: Cow::Owned(label.clone()),
                            command_id: "document.default_font.target",
                            label: Cow::Borrowed("Script"),
                            available: true,
                            value: Some(Self::target_key_for(target)),
                            has_children: true,
                        }),
                );
                values
            }
            CommandLineMode::DefaultFontScope => {
                vec![
                    close_candidate(),
                    CommandCandidate {
                        alias: Cow::Borrowed("Script"),
                        command_id: "document.default_font.script",
                        label: Cow::Borrowed("Script font"),
                        available: true,
                        value: None,
                        has_children: false,
                    },
                    CommandCandidate {
                        alias: Cow::Borrowed("Script+"),
                        command_id: "document.default_font.script_language_scope",
                        label: Cow::Borrowed("Script + language override"),
                        available: true,
                        value: None,
                        has_children: true,
                    },
                ]
            }
            CommandLineMode::DefaultFontLanguage => {
                let mut values = vec![close_candidate()];
                if is_valid_language_tag(query) {
                    values.push(CommandCandidate {
                        alias: Cow::Owned(query.to_owned()),
                        command_id: "document.default_font.language",
                        label: Cow::Borrowed("Language override"),
                        available: true,
                        value: Some(query.to_owned()),
                        has_children: true,
                    });
                }
                values
            }
            CommandLineMode::DefaultFontFamily => {
                let mut values = vec![close_candidate()];
                values.extend(
                    self.families
                        .iter()
                        .filter(|family| family.to_lowercase().starts_with(&query.to_lowercase()))
                        .map(|family| CommandCandidate {
                            alias: Cow::Owned(family.clone()),
                            command_id: "document.default_font.family",
                            label: Cow::Borrowed("Font family"),
                            available: true,
                            value: Some(family.clone()),
                            has_children: false,
                        }),
                );
                values
            }
            CommandLineMode::Command | CommandLineMode::Math => Vec::new(),
        };
        candidates.retain_mut(|candidate| {
            candidate.available &= is_available(candidate);
            candidate.available
        });
        candidates
    }
}

fn close_candidate() -> CommandCandidate {
    CommandCandidate {
        alias: Cow::Borrowed("Close"),
        command_id: "command_line.close_font_picker",
        label: Cow::Borrowed("Close font picker"),
        available: true,
        value: None,
        has_children: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_candidates_use_document_language_tag_rules() {
        let picker = FontPicker::default_targets(Vec::new(), Vec::new());
        let candidates = picker.candidates(CommandLineMode::DefaultFontLanguage, "en-a", |_| true);
        assert!(
            candidates
                .iter()
                .all(|candidate| { candidate.command_id != "document.default_font.language" })
        );

        let candidates =
            picker.candidates(CommandLineMode::DefaultFontLanguage, "x-private", |_| true);
        assert!(candidates.iter().any(|candidate| {
            candidate.command_id == "document.default_font.language"
                && candidate.value.as_deref() == Some("x-private")
        }));
    }
}
