//! 根据文档内容推导所需功能标识。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

impl Document {
    pub fn required_features(&self) -> Vec<String> {
        let mut features = std::collections::BTreeSet::new();
        features.insert("caret.paragraph.layout-settings".to_owned());
        features.insert("caret.paragraph.content-flow.v1".to_owned());
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
                || paragraph.inlines.iter().any(paragraph_content_has_language)
        }
        Block::Environment(environment) => {
            environment.title.as_ref().is_some_and(|title| {
                title.language.is_some() || title.inlines.iter().any(paragraph_content_has_language)
            }) || environment.children.iter().any(block_has_language)
        }
        Block::Equation(equation) => math_flow_has_language(&equation.math_tree.root),
        Block::Align(align) => align
            .rows
            .iter()
            .flat_map(|row| &row.cells)
            .any(|tree| math_flow_has_language(&tree.root)),
        Block::ExplicitBreak(_) | Block::Opaque(_) => false,
    }
}

fn paragraph_content_has_language(content: &ParagraphContent) -> bool {
    match content {
        ParagraphContent::Text(text) => text.runs.iter().any(|run| run.language.is_some()),
        ParagraphContent::Environment(environment) => environment
            .children
            .iter()
            .any(paragraph_content_has_language),
        ParagraphContent::Math(math) => math_flow_has_language(&math.math_tree.root),
        ParagraphContent::SoftBreak(_) | ParagraphContent::Opaque(_) => false,
    }
}

fn math_flow_has_language(flow: &MathFlow) -> bool {
    flow.content.iter().any(|content| match content {
        MathContent::Text(text) => text.runs.iter().any(|run| run.language.is_some()),
        MathContent::Math(node) => node.children.iter().any(math_flow_has_language),
        MathContent::Environment(environment) => {
            environment.children.iter().any(math_content_has_language)
        }
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
    })
}

fn math_content_has_language(content: &MathContent) -> bool {
    match content {
        MathContent::Text(text) => text.runs.iter().any(|run| run.language.is_some()),
        MathContent::Math(node) => node.children.iter().any(math_flow_has_language),
        MathContent::Environment(environment) => {
            environment.children.iter().any(math_content_has_language)
        }
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => false,
    }
}

fn collect_block_features(block: &Block, features: &mut std::collections::BTreeSet<String>) {
    match block {
        Block::Paragraph(paragraph) => {
            if paragraph.language.is_some() {
                features.insert("caret.text.language-tags".to_owned());
            }
            for content in &paragraph.inlines {
                collect_paragraph_content_features(content, features);
            }
        }
        Block::Equation(equation) => {
            features.insert("caret.math.equation".to_owned());
            features.insert("caret.math.content-flow.v1".to_owned());
            collect_math_flow_features(&equation.math_tree.root, features);
        }
        Block::Align(align) => {
            features.insert("caret.math.align".to_owned());
            features.insert("caret.math.content-flow.v1".to_owned());
            for tree in align.rows.iter().flat_map(|row| &row.cells) {
                collect_math_flow_features(&tree.root, features);
            }
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
                for content in &title.inlines {
                    collect_paragraph_content_features(content, features);
                }
            }
            for child in &environment.children {
                collect_block_features(child, features);
            }
        }
        Block::ExplicitBreak(_) | Block::Opaque(_) => {}
    }
}

fn collect_paragraph_content_features(
    content: &ParagraphContent,
    features: &mut std::collections::BTreeSet<String>,
) {
    match content {
        ParagraphContent::Text(text) => collect_text_flow_features(text, features),
        ParagraphContent::Math(math) => {
            features.insert("caret.math.inline".to_owned());
            features.insert("caret.math.content-flow.v1".to_owned());
            collect_math_flow_features(&math.math_tree.root, features);
        }
        ParagraphContent::Environment(environment) => {
            collect_environment_features(environment, features);
            for child in &environment.children {
                collect_paragraph_content_features(child, features);
            }
        }
        ParagraphContent::SoftBreak(_) | ParagraphContent::Opaque(_) => {}
    }
}

fn collect_math_content_features(
    content: &MathContent,
    features: &mut std::collections::BTreeSet<String>,
) {
    match content {
        MathContent::Text(text) => collect_text_flow_features(text, features),
        MathContent::Math(node) => {
            if !node.attributes.is_empty()
                || matches!(
                    node.kind,
                    MathNodeKind::SizedDelimiter
                        | MathNodeKind::Matrix
                        | MathNodeKind::Substack
                        | MathNodeKind::Intertext
                        | MathNodeKind::Hline
                        | MathNodeKind::Sum
                        | MathNodeKind::Integral
                        | MathNodeKind::Product
                        | MathNodeKind::Limit
                        | MathNodeKind::OverUnder
                        | MathNodeKind::CancelTo
                        | MathNodeKind::Color
                        | MathNodeKind::TextColor
                        | MathNodeKind::ColorBox
                        | MathNodeKind::FColorBox
                        | MathNodeKind::Strut
                        | MathNodeKind::Phantom
                )
            {
                features.insert("caret.math.syntax.v1".to_owned());
            }
            for child in &node.children {
                collect_math_flow_features(child, features);
            }
        }
        MathContent::Environment(environment) => {
            collect_environment_features(environment, features);
            for child in &environment.children {
                collect_math_content_features(child, features);
            }
        }
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => {}
    }
}

fn collect_text_flow_features(text: &TextFlow, features: &mut std::collections::BTreeSet<String>) {
    for run in &text.runs {
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
}

fn collect_environment_features(
    environment: &InlineEnvironment<impl Sized>,
    features: &mut std::collections::BTreeSet<String>,
) {
    features.insert(environment.kind.required_feature().to_owned());
    if environment.definition_id.is_some() {
        features.insert("caret.env.templates".to_owned());
    }
    if environment.kind.supports_enabled() && environment.props.contains_key("enabled") {
        features.insert("caret.env.explicit-format-state".to_owned());
    }
}

fn collect_math_flow_features(flow: &MathFlow, features: &mut std::collections::BTreeSet<String>) {
    for content in &flow.content {
        collect_math_content_features(content, features);
    }
}
