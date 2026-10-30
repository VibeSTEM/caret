//! 根据文档内容推导所需功能标识。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

impl Document {
    pub fn required_features(&self) -> Vec<String> {
        let mut features = std::collections::BTreeSet::new();
        features.insert("caret.paragraph.layout-settings".to_owned());
        features.insert("caret.text.default-font-size".to_owned());
        if self.global_settings.default_language.is_some()
            || self.body.iter().any(block_has_language)
        {
            features.insert("caret.text.language-tags".to_owned());
        }
        if self.global_settings.theme.is_some() {
            features.insert("caret.theme.snapshot".to_owned());
        }
        if self.global_settings.default_font_family.is_some()
            || self.global_settings.default_cjk_font_family.is_some()
            || self.global_settings.default_math_font_family.is_some()
            || self.global_settings.default_code_font_family != DEFAULT_CODE_FONT_FAMILY
            || !self.global_settings.script_font_families.is_empty()
            || !self
                .global_settings
                .language_script_font_families
                .is_empty()
        {
            features.insert("caret.font.family-preferences".to_owned());
        }
        if self
            .global_settings
            .theme
            .as_ref()
            .is_some_and(|theme| !theme.environment_templates.is_empty())
        {
            features.insert("caret.env.templates".to_owned());
        }
        for block in &self.body {
            collect_block_features(block, &mut features);
        }
        features.into_iter().collect()
    }
}

fn block_has_language(block: &Block) -> bool {
    match block {
        Block::Paragraph(paragraph) => {
            paragraph.language.is_some()
                || paragraph.inlines.iter().any(|inline| match inline {
                    InlineNode::Run(run) => run.language.is_some(),
                    InlineNode::Environment(environment) => {
                        environment.children.iter().any(inline_has_language)
                    }
                    InlineNode::Math(_) | InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => false,
                })
        }
        Block::Environment(environment) => {
            environment.title.as_ref().is_some_and(|title| {
                title.language.is_some() || title.inlines.iter().any(inline_has_language)
            }) || environment.children.iter().any(block_has_language)
        }
        Block::Equation(_) | Block::Align(_) | Block::ExplicitBreak(_) | Block::Opaque(_) => false,
    }
}

fn inline_has_language(inline: &InlineNode) -> bool {
    match inline {
        InlineNode::Run(run) => run.language.is_some(),
        InlineNode::Environment(environment) => {
            environment.children.iter().any(inline_has_language)
        }
        InlineNode::Math(_) | InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => false,
    }
}

fn collect_block_features(block: &Block, features: &mut std::collections::BTreeSet<String>) {
    match block {
        Block::Paragraph(paragraph) => {
            if paragraph.language.is_some() {
                features.insert("caret.text.language-tags".to_owned());
            }
            for inline in &paragraph.inlines {
                collect_inline_features(inline, features);
            }
        }
        Block::Equation(_) => {
            features.insert("caret.math.equation".to_owned());
        }
        Block::Align(_) => {
            features.insert("caret.math.align".to_owned());
        }
        Block::Environment(environment) => {
            features.insert(
                match environment.kind {
                    BlockEnvironmentKind::Section => "caret.env.section",
                    BlockEnvironmentKind::Custom => "caret.env.custom",
                }
                .to_owned(),
            );
            if environment.definition_id.is_some() {
                features.insert("caret.env.templates".to_owned());
            }
            if let Some(title) = &environment.title {
                if title.language.is_some() {
                    features.insert("caret.text.language-tags".to_owned());
                }
                for inline in &title.inlines {
                    collect_inline_features(inline, features);
                }
            }
            for child in &environment.children {
                collect_block_features(child, features);
            }
        }
        Block::ExplicitBreak(_) | Block::Opaque(_) => {}
    }
}

fn collect_inline_features(inline: &InlineNode, features: &mut std::collections::BTreeSet<String>) {
    match inline {
        InlineNode::Run(run) => {
            if run.language.is_some() {
                features.insert("caret.text.language-tags".to_owned());
            }
            if let Some(value) = run.props.get("underline") {
                features.insert(
                    if value.is_boolean() {
                        "caret.text.underline"
                    } else {
                        "caret.text.underline.extended"
                    }
                    .to_owned(),
                );
            }
        }
        InlineNode::Math(_) => {
            features.insert("caret.math.inline".to_owned());
        }
        InlineNode::Environment(environment) => {
            features.insert(environment.kind.required_feature().to_owned());
            if environment.definition_id.is_some() {
                features.insert("caret.env.templates".to_owned());
            }
            if environment.kind.supports_enabled() && environment.props.contains_key("enabled") {
                features.insert("caret.env.explicit-format-state".to_owned());
            }
            for child in &environment.children {
                collect_inline_features(child, features);
            }
        }
        InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => {}
    }
}
