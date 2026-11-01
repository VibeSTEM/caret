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

impl MathTree {
    /// 校验独立数学树的 schema、深度、源范围、节点属性、操作数形状和树内 ID 唯一性。
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_math_tree(self, &mut HashSet::new(), &mut 0)
    }
}

impl Align {
    /// 校验独立对齐公式及其所有行/cell 的 schema、资源预算和全局唯一 ID。
    pub fn validate(&self) -> Result<(), ModelError> {
        let mut node_count = 0;
        validate_depth(0, &mut node_count)?;
        validate_align(self, 0, &mut HashSet::new(), &mut node_count)
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
        Block::Align(align) => validate_align(align, depth, ids, node_count),
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

fn validate_align<'a>(
    align: &'a Align,
    depth: usize,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
) -> Result<(), ModelError> {
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
    for content in &paragraph.inlines {
        validate_paragraph_content(content, depth + 1, ids, node_count)?;
    }
    Ok(())
}

fn validate_paragraph_content<'a>(
    content: &'a ParagraphContent,
    depth: usize,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
) -> Result<(), ModelError> {
    validate_depth(depth, node_count)?;
    match content {
        ParagraphContent::Text(text) => {
            if text.source_range.is_some() || !text.source_map.is_empty() {
                return Err(ModelError::InvalidDocument(
                    "TextFlow source mapping is only valid inside a math tree".to_owned(),
                ));
            }
            validate_text_flow(text, None)?;
            for _ in &text.runs {
                count_document_node(node_count)?;
            }
            Ok(())
        }
        ParagraphContent::Math(math) => {
            insert_id(ids, &math.id)?;
            validate_math_tree(&math.math_tree, ids, node_count)
        }
        ParagraphContent::Environment(environment) => {
            if environment.source_range.is_some() {
                return Err(ModelError::InvalidDocument(
                    "sourceRange is only valid inside a math tree".to_owned(),
                ));
            }
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
                validate_paragraph_content(child, depth + 1, ids, node_count)?;
            }
            Ok(())
        }
        ParagraphContent::SoftBreak(_) | ParagraphContent::Opaque(_) => Ok(()),
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
    let mut budget = MathValidationBudget::default();
    add_math_extra_bytes(&tree.extra, &mut budget.payload_bytes)?;
    validate_math_flow(
        &tree.root,
        &tree.source,
        0,
        None,
        ids,
        node_count,
        &mut budget,
    )
}

fn validate_text_flow(text: &TextFlow, source: Option<&str>) -> Result<(), ModelError> {
    if let (Some(source), Some(range)) = (source, text.source_range.as_ref()) {
        validate_math_range(range, source)?;
    }
    let text_length = text.runs.iter().try_fold(0usize, |length, run| {
        length
            .checked_add(run.text.len())
            .ok_or(ModelError::InvalidDocument(
                "text flow length exceeds addressable size".to_owned(),
            ))
    })?;
    let mut previous_text_end = 0usize;
    let mut previous_source_end = None;
    if !text.source_map.is_empty() && text.source_range.is_none() {
        return Err(ModelError::InvalidDocument(
            "TextFlow sourceMap requires a sourceRange".to_owned(),
        ));
    }
    if text.source_map.len() > MAX_MATH_TREE_NODES {
        return Err(ModelError::InvalidDocument(
            "TextFlow sourceMap exceeds node limit".to_owned(),
        ));
    }
    // 映射按显示字节递增；游标只向前扫描，避免每个边界都从首个 Run 重新遍历。
    let mut run_index = 0usize;
    let mut run_start = 0usize;
    for entry in &text.source_map {
        let (text_start, text_end) = (entry.text_start as usize, entry.text_end as usize);
        if text_start > text_end
            || text_end > text_length
            || text_start < previous_text_end
            || !text_flow_char_boundary(&text.runs, text_start, &mut run_index, &mut run_start)
            || !text_flow_char_boundary(&text.runs, text_end, &mut run_index, &mut run_start)
            || text_start != previous_text_end
            || source.is_none()
        {
            return Err(ModelError::InvalidDocument(
                "invalid TextFlow source map range".to_owned(),
            ));
        }
        let Some(source) = source else {
            return Err(ModelError::InvalidDocument(
                "TextFlow sourceMap is only valid inside a math tree".to_owned(),
            ));
        };
        validate_math_range(&entry.source_range, source)?;
        if text.source_range.as_ref().is_some_and(|flow_range| {
            entry.source_range.start < flow_range.start || entry.source_range.end > flow_range.end
        }) {
            return Err(ModelError::InvalidDocument(
                "TextFlow sourceMap must stay inside its sourceRange".to_owned(),
            ));
        }
        if previous_source_end.is_some_and(|end| entry.source_range.start < end) {
            return Err(ModelError::InvalidDocument(
                "TextFlow source map ranges must follow source order".to_owned(),
            ));
        }
        previous_text_end = text_end;
        previous_source_end = Some(entry.source_range.end);
    }
    if !text.source_map.is_empty()
        && previous_text_end != text_length
        && !(text_length == 0 && text.source_map.len() == 1)
    {
        return Err(ModelError::InvalidDocument(
            "TextFlow sourceMap must cover all display text".to_owned(),
        ));
    }
    for run in &text.runs {
        validate_run(run)?;
    }
    Ok(())
}

fn text_flow_char_boundary(
    runs: &[Run],
    offset: usize,
    run_index: &mut usize,
    run_start: &mut usize,
) -> bool {
    while let Some(run) = runs.get(*run_index) {
        let Some(run_end) = run_start.checked_add(run.text.len()) else {
            return false;
        };
        if offset <= run_end {
            return run.text.is_char_boundary(offset.saturating_sub(*run_start));
        }
        *run_start = run_end;
        *run_index += 1;
    }
    offset == *run_start
}

fn count_text_flow_source_map(
    text: &TextFlow,
    budget: &mut MathValidationBudget,
) -> Result<(), ModelError> {
    for entry in &text.source_map {
        count_math_node(budget)?;
        budget.payload_bytes = budget
            .payload_bytes
            .saturating_add(std::mem::size_of::<TextFlowSourceMapEntry>());
        add_math_extra_bytes(&entry.source_range.extra, &mut budget.payload_bytes)?;
        add_math_extra_bytes(&entry.extra, &mut budget.payload_bytes)?;
    }
    Ok(())
}

fn validate_run(run: &Run) -> Result<(), ModelError> {
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

#[derive(Default)]
struct MathValidationBudget {
    node_count: usize,
    payload_bytes: usize,
    content_bytes: usize,
}

fn validate_math_flow<'a>(
    flow: &'a MathFlow,
    source: &str,
    depth: usize,
    parent: Option<MathNodeKind>,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
    budget: &mut MathValidationBudget,
) -> Result<(), ModelError> {
    if depth > MAX_MATH_TREE_DEPTH {
        return Err(ModelError::NestingLimit);
    }
    budget.node_count = budget.node_count.saturating_add(1);
    budget.payload_bytes = budget.payload_bytes.saturating_add(flow.id.0.len());
    add_math_extra_bytes(&flow.extra, &mut budget.payload_bytes)?;
    validate_depth(depth, node_count)?;
    insert_id(ids, &flow.id)?;
    if let Some(range) = flow.source_range.as_ref() {
        add_math_extra_bytes(&range.extra, &mut budget.payload_bytes)?;
        validate_math_range(range, source)?;
    }
    if budget.node_count > MAX_MATH_TREE_NODES || budget.payload_bytes > MAX_MATH_TREE_PAYLOAD_BYTES
    {
        return Err(ModelError::InvalidDocument(
            "math tree exceeds resource limits".to_owned(),
        ));
    }
    for content in &flow.content {
        match content {
            MathContent::Text(text) => {
                validate_depth(depth + 1, node_count)?;
                count_math_node(budget)?;
                add_math_extra_bytes(&text.extra, &mut budget.payload_bytes)?;
                if let Some(range) = &text.source_range {
                    add_math_extra_bytes(&range.extra, &mut budget.payload_bytes)?;
                }
                count_text_flow_source_map(text, budget)?;
                validate_text_flow(text, Some(source))?;
                for run in &text.runs {
                    count_document_node(node_count)?;
                    count_math_node(budget)?;
                    budget.payload_bytes = budget
                        .payload_bytes
                        .saturating_add(run.text.len())
                        .saturating_add(run.language.as_ref().map_or(0, String::len));
                    add_math_extra_bytes(&run.props, &mut budget.payload_bytes)?;
                    add_math_extra_bytes(&run.extra, &mut budget.payload_bytes)?;
                    budget.content_bytes = budget.content_bytes.saturating_add(run.text.len());
                }
            }
            MathContent::Math(node) => {
                validate_math_node(node, source, depth + 1, parent, ids, node_count, budget)?
            }
            MathContent::Environment(_) => {
                validate_math_content(content, source, depth + 1, ids, node_count, budget)?;
            }
            MathContent::SoftBreak(value) => {
                validate_depth(depth + 1, node_count)?;
                count_math_node(budget)?;
                add_math_extra_bytes(&value.extra, &mut budget.payload_bytes)?;
            }
            MathContent::Opaque(_) => count_math_node(budget)?,
        }
        if budget.content_bytes > MAX_MATH_SOURCE_BYTES
            || budget.payload_bytes > MAX_MATH_TREE_PAYLOAD_BYTES
        {
            return Err(ModelError::InvalidDocument(
                "math tree exceeds resource limits".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_math_content<'a>(
    content: &'a MathContent,
    source: &str,
    depth: usize,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
    budget: &mut MathValidationBudget,
) -> Result<(), ModelError> {
    if depth > MAX_MATH_TREE_DEPTH {
        return Err(ModelError::NestingLimit);
    }
    validate_depth(depth, node_count)?;
    match content {
        MathContent::Math(node) => {
            validate_math_node(node, source, depth, None, ids, node_count, budget)
        }
        MathContent::Text(text) => {
            count_math_node(budget)?;
            add_math_extra_bytes(&text.extra, &mut budget.payload_bytes)?;
            if let Some(range) = &text.source_range {
                add_math_extra_bytes(&range.extra, &mut budget.payload_bytes)?;
            }
            count_text_flow_source_map(text, budget)?;
            validate_text_flow(text, Some(source))?;
            for run in &text.runs {
                count_document_node(node_count)?;
                count_math_node(budget)?;
                budget.payload_bytes = budget
                    .payload_bytes
                    .saturating_add(run.text.len())
                    .saturating_add(run.language.as_ref().map_or(0, String::len));
                budget.content_bytes = budget.content_bytes.saturating_add(run.text.len());
                add_math_extra_bytes(&run.props, &mut budget.payload_bytes)?;
                add_math_extra_bytes(&run.extra, &mut budget.payload_bytes)?;
            }
            if budget.content_bytes > MAX_MATH_SOURCE_BYTES {
                return Err(ModelError::InvalidDocument(
                    "math tree content exceeds byte limit".to_owned(),
                ));
            }
            Ok(())
        }
        MathContent::Environment(environment) => {
            count_math_node(budget)?;
            insert_id(ids, &environment.id)?;
            budget.payload_bytes = budget
                .payload_bytes
                .saturating_add(environment.id.0.len())
                .saturating_add(environment.definition_id.as_ref().map_or(0, String::len));
            if let Some(range) = &environment.source_range {
                add_math_extra_bytes(&range.extra, &mut budget.payload_bytes)?;
                validate_math_range(range, source)?;
            }
            add_math_extra_bytes(&environment.extra, &mut budget.payload_bytes)?;
            add_math_extra_bytes(&environment.props, &mut budget.payload_bytes)?;
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
                validate_math_content(child, source, depth + 1, ids, node_count, budget)?;
            }
            Ok(())
        }
        MathContent::SoftBreak(value) => {
            count_math_node(budget)?;
            add_math_extra_bytes(&value.extra, &mut budget.payload_bytes)?;
            Ok(())
        }
        MathContent::Opaque(_) => count_math_node(budget),
    }
}

fn validate_math_node<'a>(
    node: &'a MathNode,
    source: &str,
    depth: usize,
    parent: Option<MathNodeKind>,
    ids: &mut HashSet<&'a str>,
    node_count: &mut usize,
    budget: &mut MathValidationBudget,
) -> Result<(), ModelError> {
    if depth > MAX_MATH_TREE_DEPTH {
        return Err(ModelError::NestingLimit);
    }
    count_math_node(budget)?;
    budget.payload_bytes = budget
        .payload_bytes
        .saturating_add(node.id.0.len())
        .saturating_add(node.text.as_ref().map_or(0, String::len))
        .saturating_add(node.attributes.iter().fold(0usize, |bytes, (key, value)| {
            bytes.saturating_add(key.len()).saturating_add(value.len())
        }));
    budget.content_bytes = budget
        .content_bytes
        .saturating_add(node.text.as_ref().map_or(0, String::len));
    if budget.content_bytes > MAX_MATH_SOURCE_BYTES {
        return Err(ModelError::InvalidDocument(
            "math tree content exceeds byte limit".to_owned(),
        ));
    }
    add_math_extra_bytes(&node.extra, &mut budget.payload_bytes)?;
    if budget.payload_bytes > MAX_MATH_TREE_PAYLOAD_BYTES {
        return Err(ModelError::InvalidDocument(
            "math tree payload exceeds byte limit".to_owned(),
        ));
    }
    validate_depth(depth, node_count)?;
    let child_kinds = node
        .children
        .iter()
        .map(|flow| {
            match flow.content.iter().find_map(|content| match content {
                MathContent::Math(node) => Some(node.kind),
                _ => None,
            }) {
                Some(
                    kind @ (MathNodeKind::Placeholder
                    | MathNodeKind::Hline
                    | MathNodeKind::Intertext),
                ) => kind,
                _ => MathNodeKind::Row,
            }
        })
        .collect::<Vec<_>>();
    if !super::math::math_node_attributes_supported(
        node.kind,
        node.text.as_deref(),
        &node.attributes,
    ) || !super::math::math_node_structure_supported(
        parent,
        node.kind,
        node.text.as_deref(),
        &child_kinds,
    ) {
        return Err(ModelError::InvalidDocument(
            "invalid math node attributes".to_owned(),
        ));
    }
    insert_id(ids, &node.id)?;
    if let Some(range) = node.source_range.as_ref() {
        add_math_extra_bytes(&range.extra, &mut budget.payload_bytes)?;
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
        validate_math_flow(
            child,
            source,
            depth,
            Some(node.kind),
            ids,
            node_count,
            budget,
        )?;
    }
    Ok(())
}

fn count_math_node(budget: &mut MathValidationBudget) -> Result<(), ModelError> {
    budget.node_count = budget.node_count.saturating_add(1);
    if budget.node_count > MAX_MATH_TREE_NODES {
        return Err(ModelError::InvalidDocument(
            "math tree exceeds node limit".to_owned(),
        ));
    }
    Ok(())
}

fn validate_math_range(range: &MathSourceRange, source: &str) -> Result<(), ModelError> {
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
    Ok(())
}

fn add_math_extra_bytes(
    extra: &std::collections::BTreeMap<String, Value>,
    total: &mut usize,
) -> Result<(), ModelError> {
    *total = total.saturating_add(extra.len());
    if *total > MAX_MATH_TREE_PAYLOAD_BYTES {
        return Err(ModelError::InvalidDocument(
            "math tree payload exceeds byte limit".to_owned(),
        ));
    }
    let mut pending = extra
        .iter()
        .map(|(key, value)| {
            *total = total.saturating_add(key.len());
            (value, 1usize)
        })
        .collect::<Vec<_>>();
    while let Some((value, depth)) = pending.pop() {
        if depth > MAX_DOCUMENT_NESTING_DEPTH {
            return Err(ModelError::NestingLimit);
        }
        match value {
            Value::String(text) => *total = total.saturating_add(text.len()),
            Value::Array(values) => {
                *total = total.saturating_add(values.len());
                if *total > MAX_MATH_TREE_PAYLOAD_BYTES {
                    return Err(ModelError::InvalidDocument(
                        "math tree payload exceeds byte limit".to_owned(),
                    ));
                }
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(values) => {
                *total = total.saturating_add(values.len());
                if *total > MAX_MATH_TREE_PAYLOAD_BYTES {
                    return Err(ModelError::InvalidDocument(
                        "math tree payload exceeds byte limit".to_owned(),
                    ));
                }
                for (key, value) in values {
                    *total = total.saturating_add(key.len());
                    pending.push((value, depth + 1));
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {
                *total = total.saturating_add(1);
            }
        }
        if *total > MAX_MATH_TREE_PAYLOAD_BYTES {
            return Err(ModelError::InvalidDocument(
                "math tree payload exceeds byte limit".to_owned(),
            ));
        }
    }
    if *total > MAX_MATH_TREE_PAYLOAD_BYTES {
        return Err(ModelError::InvalidDocument(
            "math tree payload exceeds byte limit".to_owned(),
        ));
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
    count_document_node(node_count)
}

fn count_document_node(node_count: &mut usize) -> Result<(), ModelError> {
    *node_count = node_count.saturating_add(1);
    if *node_count > 2_000_000 {
        return Err(ModelError::InvalidDocument(
            "node limit exceeded".to_owned(),
        ));
    }
    Ok(())
}
