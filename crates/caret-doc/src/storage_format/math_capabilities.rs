//! 从已知文档语义路径检查数学载体的能力声明。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{DocumentError, is_known_inline_environment};
use crate::MathNodeKind;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

/// 即使未知子节点使类型解码器将载体保留为不透明数据，也要从 envelope 校验当前数学 schema 的能力声明。
pub(super) fn validate_raw_math_feature_declarations(
    raw: &Value,
    declared: &[String],
) -> Result<(), DocumentError> {
    fn has_current_tree(value: &Value) -> bool {
        value
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .is_some_and(|version| version == crate::MATH_TREE_SCHEMA_VERSION as u64)
    }

    fn add_carrier(kind: &'static str, required: &mut BTreeSet<&'static str>) {
        required.insert("caret.math.content-flow.v2");
        match kind {
            "inlineMath" => {
                required.insert("caret.math.inline");
            }
            "equation" => {
                required.insert("caret.math.equation");
            }
            "align" => {
                required.insert("caret.math.align");
            }
            _ => {}
        }
    }

    fn collect_tree_syntax(tree: &Value, required: &mut BTreeSet<&'static str>) {
        fn parse_attributes(
            value: Option<&Value>,
        ) -> Option<std::collections::BTreeMap<String, String>> {
            let Some(value) = value else {
                return Some(std::collections::BTreeMap::new());
            };
            let object = value.as_object()?;
            object
                .iter()
                .map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                .collect()
        }

        fn child_kinds(children: &[Value]) -> Vec<MathNodeKind> {
            children
                .iter()
                .map(|flow| {
                    let first_math =
                        flow.get("content")
                            .and_then(Value::as_array)
                            .and_then(|content| {
                                content.iter().find(|item| {
                                    item.get("type").and_then(Value::as_str) == Some("math")
                                })
                            });
                    match first_math
                        .and_then(|item| item.get("kind"))
                        .and_then(Value::as_str)
                        .and_then(|name| {
                            serde_json::from_value::<MathNodeKind>(Value::String(name.to_owned()))
                                .ok()
                        }) {
                        Some(
                            special @ (MathNodeKind::Placeholder
                            | MathNodeKind::Hline
                            | MathNodeKind::Intertext),
                        ) => special,
                        _ => MathNodeKind::Row,
                    }
                })
                .collect()
        }

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
                    let Some(kind_name) = object.get("kind").and_then(Value::as_str) else {
                        return;
                    };
                    let Ok(kind) =
                        serde_json::from_value::<MathNodeKind>(Value::String(kind_name.to_owned()))
                    else {
                        return;
                    };
                    let Some(attributes) = parse_attributes(object.get("attributes")) else {
                        return;
                    };
                    let text = match object.get("text") {
                        None | Some(Value::Null) => None,
                        Some(Value::String(text)) => Some(text.as_str()),
                        _ => return,
                    };
                    let children = match object.get("children") {
                        None => &[][..],
                        Some(Value::Array(children)) => children.as_slice(),
                        _ => return,
                    };
                    if !crate::model::math_node_attributes_supported(kind, text, &attributes)
                        || !crate::model::math_node_structure_supported(
                            parent,
                            kind,
                            text,
                            &child_kinds(children),
                        )
                    {
                        return;
                    }
                    let has_attributes = !attributes.is_empty();
                    if crate::model::math_node_requires_syntax(kind, has_attributes) {
                        required.insert("caret.math.syntax.v1");
                    }
                    for child in children {
                        visit_flow(child, Some(kind), required);
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

        if has_current_tree(tree)
            && let Some(root) = tree.get("root")
        {
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
                Some("inlineMath") if object.get("mathTree").is_some_and(has_current_tree) => {
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
            Some("inlineMath") if object.get("mathTree").is_some_and(has_current_tree) => {
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
            Some("equation") if object.get("mathTree").is_some_and(has_current_tree) => {
                add_carrier("equation", required);
                if let Some(tree) = object.get("mathTree") {
                    collect_tree_syntax(tree, required);
                }
            }
            Some("align") => {
                let has_current_cell =
                    object
                        .get("rows")
                        .and_then(Value::as_array)
                        .is_some_and(|rows| {
                            rows.iter().any(|row| {
                                row.get("cells")
                                    .and_then(Value::as_array)
                                    .is_some_and(|cells| cells.iter().any(has_current_tree))
                            })
                        });
                if has_current_cell {
                    add_carrier("align", required);
                    if let Some(rows) = object.get("rows").and_then(Value::as_array) {
                        for cell in rows
                            .iter()
                            .filter_map(|row| row.get("cells").and_then(Value::as_array))
                            .flatten()
                            .filter(|cell| has_current_tree(cell))
                        {
                            collect_tree_syntax(cell, required);
                        }
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
    let declared = declared.iter().map(String::as_str).collect::<HashSet<_>>();
    for feature in required {
        if !declared.contains(feature) {
            return Err(DocumentError::InvalidDocument(format!(
                "required math capability is missing: {feature}"
            )));
        }
    }
    Ok(())
}
