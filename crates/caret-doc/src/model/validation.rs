//! 校验文档设置、节点树和语言标签。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use std::collections::HashSet;

pub const MAX_DOCUMENT_NESTING_DEPTH: usize = 64;

impl Document {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.page_sections.len() != 1 {
            return Err(ModelError::InvalidDocument(format!(
                "M0 requires exactly one PageSection, got {}",
                self.page_sections.len()
            )));
        }
        let mut ids = HashSet::new();
        if let Some(language) = &self.global_settings.default_language {
            validate_language_tag(language)?;
        }
        if self.global_settings.default_font_size_centipoints == 0 {
            return Err(ModelError::InvalidDocument(
                "default font size must be positive".to_owned(),
            ));
        }
        validate_font_family(
            self.global_settings.default_font_family.as_deref(),
            "defaultFontFamily",
        )?;
        validate_font_family(
            self.global_settings.default_cjk_font_family.as_deref(),
            "defaultCjkFontFamily",
        )?;
        validate_font_family(
            self.global_settings.default_math_font_family.as_deref(),
            "defaultMathFontFamily",
        )?;
        if self
            .global_settings
            .default_code_font_family
            .trim()
            .is_empty()
        {
            return Err(ModelError::InvalidDocument(
                "defaultCodeFontFamily must not be empty".to_owned(),
            ));
        }
        self.global_settings.paragraph_layout.validate()?;
        validate_unique_script_keys(self.global_settings.script_font_families.keys())?;
        validate_unique_script_keys(self.global_settings.language_script_font_families.keys())?;
        for family in self.global_settings.script_font_families.values() {
            validate_font_family(Some(family), "scriptFontFamilies")?;
        }
        for languages in self.global_settings.language_script_font_families.values() {
            validate_unique_language_keys(languages.keys())?;
            for family in languages.values() {
                validate_font_family(Some(family), "languageScriptFontFamilies")?;
            }
            for language in languages.keys() {
                validate_language_tag(language)?;
            }
        }
        for section in &self.page_sections {
            insert_id(&mut ids, &section.id)?;
            section.geometry().map_err(ModelError::from)?;
        }
        let mut count = 0usize;
        for block in &self.body {
            validate_block(block, 0, &mut ids, &mut count)?;
        }
        Ok(())
    }
}

fn validate_font_family(family: Option<&str>, field: &str) -> Result<(), ModelError> {
    if family.is_some_and(|family| family.trim().is_empty()) {
        return Err(ModelError::InvalidDocument(format!(
            "font family in {field} must not be empty"
        )));
    }
    Ok(())
}

fn validate_unique_script_keys<'a>(
    keys: impl Iterator<Item = &'a String>,
) -> Result<(), ModelError> {
    let mut normalized = HashSet::new();
    for key in keys {
        if key.len() != 4 || !key.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return Err(ModelError::InvalidDocument(format!(
                "invalid Unicode Script key: {key}"
            )));
        }
        if !normalized.insert(key.to_ascii_lowercase()) {
            return Err(ModelError::InvalidDocument(format!(
                "duplicate Unicode Script key: {key}"
            )));
        }
    }
    Ok(())
}

fn validate_unique_language_keys<'a>(
    keys: impl Iterator<Item = &'a String>,
) -> Result<(), ModelError> {
    let mut normalized = HashSet::new();
    for key in keys {
        if !normalized.insert(key.to_ascii_lowercase()) {
            return Err(ModelError::InvalidDocument(format!(
                "duplicate BCP 47 language key: {key}"
            )));
        }
    }
    Ok(())
}

impl ParagraphLayoutStyle {
    fn validate(&self) -> Result<(), ModelError> {
        validate_line_spacing(&self.line_spacing)?;
        Ok(())
    }
}

impl ParagraphLayoutOverrides {
    fn validate(&self) -> Result<(), ModelError> {
        if let Some(spacing) = &self.line_spacing {
            validate_line_spacing(spacing)?;
        }
        Ok(())
    }
}

fn validate_line_spacing(spacing: &ParagraphLineSpacing) -> Result<(), ModelError> {
    match spacing {
        ParagraphLineSpacing::Multiple { percent: 0, .. }
        | ParagraphLineSpacing::AtLeast {
            min_centipoints: 0, ..
        }
        | ParagraphLineSpacing::Exactly { centipoints: 0, .. } => {
            return Err(ModelError::InvalidDocument(
                "paragraph line spacing must be positive".to_owned(),
            ));
        }
        _ => {}
    }
    Ok(())
}

fn insert_id<'a>(ids: &mut HashSet<&'a str>, id: &'a NodeId) -> Result<(), ModelError> {
    if id.0.is_empty() || id.0.len() > 128 || id.0.chars().any(char::is_control) {
        return Err(ModelError::InvalidNodeId(id.0.clone()));
    }
    if !ids.insert(id.0.as_str()) {
        return Err(ModelError::DuplicateNodeId(id.0.clone()));
    }
    Ok(())
}

fn validate_block<'a>(
    block: &'a Block,
    depth: usize,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
) -> Result<(), ModelError> {
    validate_depth(depth, node_count)?;
    match block {
        Block::Paragraph(paragraph) => validate_paragraph(paragraph, depth, ids, node_count),
        Block::Equation(equation) => {
            insert_id(ids, &equation.id)?;
            validate_math_tree(&equation.math_tree, ids, node_count)
        }
        Block::Align(align) => {
            insert_id(ids, &align.id)?;
            if align.source.len() > MAX_MATH_SOURCE_BYTES {
                return Err(ModelError::InvalidDocument(
                    "math source exceeds byte limit".to_owned(),
                ));
            }
            for row in &align.rows {
                validate_depth(depth + 1, node_count)?;
                insert_id(ids, &row.id)?;
                for cell in &row.cells {
                    validate_math_tree(cell, ids, node_count)?;
                }
            }
            Ok(())
        }
        Block::Environment(environment) => {
            insert_id(ids, &environment.id)?;
            if environment.kind == BlockEnvironmentKind::Custom
                && environment
                    .props
                    .get("displayName")
                    .is_some_and(|value| !custom_display_name_is_valid(value))
            {
                return Err(ModelError::InvalidDocument(
                    "custom environment displayName must be non-empty text up to 128 characters"
                        .to_owned(),
                ));
            }
            for child in &environment.children {
                validate_block(child, depth + 1, ids, node_count)?;
            }
            if let Some(title) = &environment.title {
                validate_paragraph(title, depth + 1, ids, node_count)?;
            }
            Ok(())
        }
        Block::ExplicitBreak(_) | Block::Opaque(_) => Ok(()),
    }
}

pub(crate) fn custom_display_name_is_valid(value: &Value) -> bool {
    value.as_str().is_some_and(|name| {
        !name.trim().is_empty()
            && name.chars().count() <= 128
            && !name.chars().any(char::is_control)
    })
}

fn validate_paragraph<'a>(
    paragraph: &'a Paragraph,
    depth: usize,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
) -> Result<(), ModelError> {
    validate_depth(depth, node_count)?;
    insert_id(ids, &paragraph.id)?;
    if let Some(language) = &paragraph.language {
        validate_language_tag(language)?;
    }
    if let Some(layout) = &paragraph.layout {
        layout.validate()?;
    }
    for property in ["fontFamily", "bold", "italic"] {
        if paragraph.props.contains_key(property) {
            return Err(ModelError::ForbiddenDirectFormat {
                node: "Paragraph".to_owned(),
                property: property.to_owned(),
            });
        }
    }
    for inline in &paragraph.inlines {
        validate_inline(inline, depth + 1, ids, node_count)?;
    }
    Ok(())
}

fn validate_inline<'a>(
    inline: &'a InlineNode,
    depth: usize,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
) -> Result<(), ModelError> {
    validate_depth(depth, node_count)?;
    match inline {
        InlineNode::Run(run) => {
            if let Some(language) = &run.language {
                validate_language_tag(language)?;
            }
            for property in ["fontFamily", "bold", "italic"] {
                if run.props.contains_key(property) {
                    return Err(ModelError::ForbiddenDirectFormat {
                        node: "Run".to_owned(),
                        property: property.to_owned(),
                    });
                }
            }
            Ok(())
        }
        InlineNode::Math(math) => {
            insert_id(ids, &math.id)?;
            validate_math_tree(&math.math_tree, ids, node_count)
        }
        InlineNode::Environment(environment) => {
            insert_id(ids, &environment.id)?;
            if environment.kind.supports_enabled()
                && environment
                    .props
                    .get("enabled")
                    .is_some_and(|value| !value.is_boolean())
            {
                return Err(ModelError::InvalidDocument(
                    "Bold/Italic enabled must be boolean".to_owned(),
                ));
            }
            if environment.kind.supports_font_family()
                && environment
                    .props
                    .get("fontFamily")
                    .is_some_and(|value| !value.is_string())
            {
                return Err(ModelError::InvalidDocument(
                    "Font fontFamily must be a string".to_owned(),
                ));
            }
            for child in &environment.children {
                validate_inline(child, depth + 1, ids, node_count)?;
            }
            Ok(())
        }
        InlineNode::SoftBreak(_) | InlineNode::Opaque(_) => Ok(()),
    }
}

fn validate_math_tree<'a>(
    tree: &'a MathTree,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
) -> Result<(), ModelError> {
    if tree.schema_version != MATH_TREE_SCHEMA_VERSION {
        return Err(ModelError::InvalidDocument(format!(
            "unsupported math tree schema version: {}",
            tree.schema_version
        )));
    }
    if tree.source.len() > MAX_MATH_SOURCE_BYTES {
        return Err(ModelError::InvalidDocument(
            "math source exceeds byte limit".to_owned(),
        ));
    }
    validate_math_node(&tree.root, &tree.source, 0, ids, node_count)
}

fn validate_math_node<'a>(
    node: &'a MathNode,
    source: &str,
    depth: usize,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
) -> Result<(), ModelError> {
    if depth >= MAX_MATH_TREE_DEPTH {
        return Err(ModelError::NestingLimit);
    }
    validate_depth(depth, node_count)?;
    insert_id(ids, &node.id)?;
    if let Some(range) = node.source_range.as_ref() {
        let start = range.start as usize;
        let end = range.end as usize;
        if start > end
            || end > source.len()
            || !source.is_char_boundary(start)
            || !source.is_char_boundary(end)
        {
            return Err(ModelError::InvalidDocument(
                "invalid math source byte range".to_owned(),
            ));
        }
    }
    for child in &node.children {
        validate_math_node(child, source, depth + 1, ids, node_count)?;
    }
    Ok(())
}

fn validate_language_tag(tag: &str) -> Result<(), ModelError> {
    if is_valid_language_tag(tag) {
        Ok(())
    } else {
        Err(ModelError::InvalidLanguageTag(tag.to_owned()))
    }
}

pub fn is_valid_language_tag(tag: &str) -> bool {
    const GRANDFATHERED: &[&str] = &[
        "art-lojban",
        "cel-gaulish",
        "en-gb-oed",
        "i-ami",
        "i-bnn",
        "i-default",
        "i-enochian",
        "i-hak",
        "i-klingon",
        "i-lux",
        "i-mingo",
        "i-navajo",
        "i-pwn",
        "i-tao",
        "i-tay",
        "i-tsu",
        "no-bok",
        "no-nyn",
        "sgn-be-fr",
        "sgn-be-nl",
        "sgn-ch-de",
        "zh-guoyu",
        "zh-hakka",
        "zh-min",
        "zh-min-nan",
        "zh-xiang",
    ];
    if GRANDFATHERED
        .iter()
        .any(|known| tag.eq_ignore_ascii_case(known))
    {
        return true;
    }
    if !tag.is_ascii() {
        return false;
    }
    let subtags: Vec<&str> = tag.split('-').collect();
    if subtags.is_empty()
        || subtags.iter().any(|subtag| {
            subtag.is_empty()
                || subtag.len() > 8
                || !subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    {
        return false;
    }
    if subtags[0].eq_ignore_ascii_case("x") {
        return subtags.len() > 1;
    }
    let language = subtags[0];
    if !(2..=8).contains(&language.len())
        || !language.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return false;
    }

    let mut index = 1;
    if language.len() <= 3 {
        let mut extlang_count = 0;
        while index < subtags.len()
            && extlang_count < 3
            && subtags[index].len() == 3
            && subtags[index]
                .bytes()
                .all(|byte| byte.is_ascii_alphabetic())
        {
            index += 1;
            extlang_count += 1;
        }
    }
    if index < subtags.len()
        && subtags[index].len() == 4
        && subtags[index]
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic())
    {
        index += 1;
    }
    if index < subtags.len()
        && ((subtags[index].len() == 2
            && subtags[index]
                .bytes()
                .all(|byte| byte.is_ascii_alphabetic()))
            || (subtags[index].len() == 3
                && subtags[index].bytes().all(|byte| byte.is_ascii_digit())))
    {
        index += 1;
    }
    let mut variants = std::collections::BTreeSet::new();
    while index < subtags.len()
        && ((5..=8).contains(&subtags[index].len())
            || (subtags[index].len() == 4 && subtags[index].as_bytes()[0].is_ascii_digit()))
    {
        if !variants.insert(subtags[index].to_ascii_lowercase()) {
            return false;
        }
        index += 1;
    }

    let mut extensions = std::collections::BTreeSet::new();
    while index < subtags.len() && subtags[index].len() == 1 {
        let singleton = subtags[index].to_ascii_lowercase();
        if singleton == "x" || !extensions.insert(singleton) {
            break;
        }
        index += 1;
        let extension_start = index;
        while index < subtags.len() && (2..=8).contains(&subtags[index].len()) {
            index += 1;
        }
        if index == extension_start {
            return false;
        }
    }
    if index < subtags.len() && subtags[index].eq_ignore_ascii_case("x") {
        index += 1;
        if index == subtags.len() {
            return false;
        }
        while index < subtags.len() && (1..=8).contains(&subtags[index].len()) {
            index += 1;
        }
    }
    index == subtags.len()
}

fn validate_depth(depth: usize, node_count: &mut usize) -> Result<(), ModelError> {
    if depth > MAX_DOCUMENT_NESTING_DEPTH {
        return Err(ModelError::NestingLimit);
    }
    *node_count = node_count.saturating_add(1);
    if *node_count > 2_000_000 {
        return Err(ModelError::InvalidDocument(
            "node limit exceeded".to_owned(),
        ));
    }
    Ok(())
}
