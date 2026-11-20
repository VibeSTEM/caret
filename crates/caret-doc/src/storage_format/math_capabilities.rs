//! 从已知文档语义路径检查数学载体的能力声明。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{DocumentError, is_known_inline_environment};
use crate::{Document, MathNodeKind};
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

/// 缺失必需数学能力时的统一格式错误；raw 扫描与类型路径共用同一文案。
pub(super) fn missing_math_capability(feature: &str) -> DocumentError {
    DocumentError::InvalidDocument(format!("required math capability is missing: {feature}"))
}

/// 推导 raw 文档使用的数学能力。
///
/// 类型解码器可能因未知子节点把整个载体保留为不透明数据，此时仍须从 envelope 校验
/// 当前数学 schema 的能力声明，因此不能复用 `Document::required_features` 的类型推导。
/// `open` 已由 `validate_current_math_encoding` 拒绝不匹配的 `MathTree.schemaVersion`，
/// 所以这里沿已知结构遍历到的树都是当前版本，不再按版本过滤。
pub(super) fn raw_math_capabilities(raw: &Value) -> BTreeSet<&'static str> {
    fn add_carrier(kind: &'static str, required: &mut BTreeSet<&'static str>) {
        required.insert(Document::MATH_FEATURE_CONTENT_FLOW_V2);
        match kind {
            "inlineMath" => {
                required.insert(Document::MATH_FEATURE_INLINE);
            }
            "equation" => {
                required.insert(Document::MATH_FEATURE_EQUATION);
            }
            "align" => {
                required.insert(Document::MATH_FEATURE_ALIGN);
            }
            _ => {}
        }
    }

    fn collect_tree_syntax(tree: &Value, required: &mut BTreeSet<&'static str>) {
        fn visit_content(
            content: &Value,
            parent: Option<MathNodeKind>,
            required: &mut BTreeSet<&'static str>,
        ) {
            let Some(object) = content.as_object() else {
                return;
            };
            match object.get("type").and_then(Value::as_str) {
                Some("math") => {
                    // raw 能力扫描保留自己的节点边界（与解码器不同）：按首个数学内容宽松推断类别，
                    // 解码器不能解释的节点不产生语法要求。不要替换为解码器判定。
                    let Some(kind) = MathNodeKind::supported_for_capability_scan(object, parent)
                    else {
                        return;
                    };
                    let has_attributes = object
                        .get("attributes")
                        .and_then(Value::as_object)
                        .is_some_and(|attributes| !attributes.is_empty());
                    if crate::model::math_node_requires_syntax(kind, has_attributes) {
                        required.insert(Document::MATH_FEATURE_SYNTAX_V1);
                    }
                    if let Some(children) = object.get("children").and_then(Value::as_array) {
                        for child in children {
                            visit_flow(child, Some(kind), required);
                        }
                    }
                }
                Some("environment")
                    if object.get("scope").and_then(Value::as_str) == Some("inline")
                        && is_known_inline_environment(object) =>
                {
                    if let Some(children) = object.get("children").and_then(Value::as_array) {
                        for child in children {
                            visit_content(child, None, required);
                        }
                    }
                }
                _ => {}
            }
        }

        fn visit_flow(
            flow: &Value,
            parent: Option<MathNodeKind>,
            required: &mut BTreeSet<&'static str>,
        ) {
            let Some(contents) = flow.get("content").and_then(Value::as_array) else {
                return;
            };
            for content in contents {
                visit_content(content, parent, required);
            }
        }

        if let Some(root) = tree.get("root") {
            visit_flow(root, None, required);
        }
    }

    fn visit_paragraph(paragraph: &Value, required: &mut BTreeSet<&'static str>) {
        let Some(inlines) = paragraph.get("inlines").and_then(Value::as_array) else {
            return;
        };
        for inline in inlines {
            let Some(object) = inline.as_object() else {
                continue;
            };
            match object.get("type").and_then(Value::as_str) {
                Some("inlineMath") => {
                    add_carrier("inlineMath", required);
                    if let Some(tree) = object.get("mathTree") {
                        collect_tree_syntax(tree, required);
                    }
                }
                Some("environment")
                    if object.get("scope").and_then(Value::as_str) == Some("inline")
                        && is_known_inline_environment(object) =>
                {
                    if let Some(children) = object.get("children").and_then(Value::as_array) {
                        for child in children {
                            visit_inline(child, required);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn visit_inline(inline: &Value, required: &mut BTreeSet<&'static str>) {
        let Some(object) = inline.as_object() else {
            return;
        };
        match object.get("type").and_then(Value::as_str) {
            Some("inlineMath") => {
                add_carrier("inlineMath", required);
                if let Some(tree) = object.get("mathTree") {
                    collect_tree_syntax(tree, required);
                }
            }
            Some("environment")
                if object.get("scope").and_then(Value::as_str) == Some("inline")
                    && is_known_inline_environment(object) =>
            {
                if let Some(children) = object.get("children").and_then(Value::as_array) {
                    for child in children {
                        visit_inline(child, required);
                    }
                }
            }
            _ => {}
        }
    }

    fn visit_block(block: &Value, required: &mut BTreeSet<&'static str>) {
        let Some(object) = block.as_object() else {
            return;
        };
        match object.get("type").and_then(Value::as_str) {
            Some("paragraph") => visit_paragraph(block, required),
            Some("equation") => {
                add_carrier("equation", required);
                if let Some(tree) = object.get("mathTree") {
                    collect_tree_syntax(tree, required);
                }
            }
            Some("align") => {
                add_carrier("align", required);
                if let Some(rows) = object.get("rows").and_then(Value::as_array) {
                    for cell in rows
                        .iter()
                        .filter_map(|row| row.get("cells").and_then(Value::as_array))
                        .flatten()
                    {
                        collect_tree_syntax(cell, required);
                    }
                }
            }
            Some("environment")
                if object.get("scope").and_then(Value::as_str) == Some("block")
                    && matches!(
                        object.get("kind").and_then(Value::as_str),
                        Some("section" | "custom")
                    ) =>
            {
                if let Some(title) = object.get("title") {
                    visit_paragraph(title, required);
                }
                if let Some(children) = object.get("children").and_then(Value::as_array) {
                    for child in children {
                        visit_block(child, required);
                    }
                }
            }
            _ => {}
        }
    }

    let mut required = BTreeSet::new();
    if let Some(body) = raw
        .get("document")
        .and_then(|document| document.get("body"))
        .and_then(Value::as_array)
    {
        for block in body {
            visit_block(block, &mut required);
        }
    }
    required
}

/// 即使未知子节点使类型解码器将载体保留为不透明数据，也要从 envelope 校验当前数学 schema 的能力声明。
pub(super) fn validate_raw_math_feature_declarations(
    raw: &Value,
    declared: &[String],
) -> Result<(), DocumentError> {
    let declared = declared.iter().map(String::as_str).collect::<HashSet<_>>();
    for feature in raw_math_capabilities(raw) {
        if Document::requires_explicit_math_declaration(feature) && !declared.contains(feature) {
            return Err(missing_math_capability(feature));
        }
    }
    Ok(())
}
