//! 序列化完整布局与轻量交互快照。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::DocumentViewRuntime;
use caret_doc::NodeId;
use caret_engine::{
    Affinity, DocumentLayoutDeltaView, PageCaretGeometry, Pos, Selection, StructureClass,
    StructureKind, StructurePosition,
};
use serde_json::{Value, json};

fn command_line_state(session: &caret_engine::Session) -> Value {
    let code_font_family = &session.document().global_settings.default_code_font_family;
    let effective_code_font_family = if code_font_family == caret_doc::DEFAULT_FONT_FAMILY {
        caret_code::BUILTIN_FONT_FAMILY
    } else {
        code_font_family
    };
    session
        .command_line()
        .map(|command_line| {
            let candidates = session
                .command_line_candidates()
                .into_iter()
                .map(|candidate| {
                    json!({
                        "alias": candidate.alias,
                        "commandId": candidate.command_id,
                        "available": candidate.available,
                        "value": candidate.value,
                        "hasChildren": candidate.has_children,
                    })
                })
                .collect::<Vec<_>>();
            let query_prefix = command_line
                .query
                .get(..command_line.cursor)
                .unwrap_or_default();
            let cursor_utf16 =
                command_line.prefix().encode_utf16().count() + query_prefix.encode_utf16().count();
            json!({
                "active": true,
                "mode": match command_line.mode {
                    caret_engine::CommandLineMode::Command => "command",
                    caret_engine::CommandLineMode::Math => "math",
                    caret_engine::CommandLineMode::FontFamily => "fontFamily",
                    caret_engine::CommandLineMode::DefaultFontTarget => "defaultFontTarget",
                    caret_engine::CommandLineMode::DefaultFontScope => "defaultFontScope",
                    caret_engine::CommandLineMode::DefaultFontLanguage => "defaultFontLanguage",
                    caret_engine::CommandLineMode::DefaultFontFamily => "defaultFontFamily",
                },
                "prefix": command_line.prefix(),
                "query": &command_line.query,
                "cursor": command_line.cursor,
                "cursorUtf16": cursor_utf16,
                "preedit": &command_line.preedit,
                "preeditCursorUtf16": command_line.preedit_cursor_utf16,
                "codeFontFamily": effective_code_font_family,
                "selected": command_line.selected,
                "candidates": candidates,
            })
        })
        .unwrap_or_else(|| {
            json!({
                "active": false,
                "mode": "command",
                "prefix": "\\",
                "query": "",
                "cursor": 0,
                "cursorUtf16": 1,
                "codeFontFamily": effective_code_font_family,
                "selected": 0,
                "candidates": [],
            })
        })
}

pub(super) fn current_structure_state(session: &caret_engine::Session) -> Value {
    if session.block_environment_exit().is_some() {
        return Value::Array(Vec::new());
    }
    let focus = &session.selection().focus;
    let stack = session.structure_index().at(&StructurePosition {
        paragraph_id: focus.paragraph_id.clone(),
        offset_utf8: focus.offset,
    });
    let has_block_environment = stack.iter().any(|entry| entry.kind.is_block_environment());
    let mut entries = Vec::with_capacity(stack.len());
    let mut previous_kind = None;
    for entry in stack {
        if has_block_environment && entry.kind.class() == StructureClass::Paragraph {
            continue;
        }
        if previous_kind == Some(entry.kind) {
            continue;
        }
        previous_kind = Some(entry.kind);
        let (kind, level) = match entry.kind {
            StructureKind::Paragraph => ("paragraph", None),
            StructureKind::Heading { level } => ("heading", Some(level)),
            StructureKind::Section => ("section", None),
            StructureKind::Custom => ("custom", None),
            StructureKind::Equation => ("equation", None),
            StructureKind::Align => ("align", None),
        };
        let mut value = json!({ "id": entry.id, "kind": kind });
        if let Some(level) = level {
            value["level"] = json!(level);
        }
        entries.push(value);
    }
    if let Some(caret_engine::MathEdit {
        target: caret_engine::MathEditTarget::Block { block_id, align },
        ..
    }) = session.math_edit()
    {
        let kind = if *align { "align" } else { "equation" };
        if !entries
            .iter()
            .any(|entry| entry["id"] == block_id.to_string())
        {
            entries.push(json!({ "id": block_id, "kind": kind }));
        }
    }
    Value::Array(entries)
}

fn current_block_environment_id(session: &caret_engine::Session) -> Option<NodeId> {
    if session.block_environment_exit().is_some() || session.selected_block_environment().is_some()
    {
        return None;
    }
    let focus = &session.selection().focus;
    let stack = session.structure_index().at(&StructurePosition {
        paragraph_id: focus.paragraph_id.clone(),
        offset_utf8: focus.offset,
    });
    stack
        .iter()
        .rev()
        .find(|entry| entry.kind.is_block_environment())
        .map(|entry| entry.id.clone())
}

/// 将最终选区一次性投影为页面矩形；文本布局负责文字几何，结构环境复用环境范围。
fn selection_rectangles_json(
    selection: &caret_engine::Selection,
    structure_selection: Option<(&caret_engine::Pos, &caret_engine::Pos)>,
    selected_environment_ids: &[NodeId],
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Value {
    let mut rectangles = if let Some((anchor, focus)) = structure_selection {
        snapshot
            .structure_selection_geometry(anchor, focus, selected_environment_ids)
            .unwrap_or_default()
    } else {
        snapshot.selection_geometry(selection).unwrap_or_default()
    }
    .into_iter()
    .map(|rectangle| {
        json!({
            "pageIndex": rectangle.page_index,
            "x0Pt": rectangle.x0_pt,
            "y0Pt": rectangle.y0_pt,
            "x1Pt": rectangle.x1_pt,
            "y1Pt": rectangle.y1_pt,
        })
    })
    .collect::<Vec<_>>();
    rectangles.extend(
        snapshot
            .environment_ranges
            .iter()
            .filter(|range| selected_environment_ids.contains(&range.environment_id))
            .map(|range| {
                json!({
                    "pageIndex": range.page_index,
                    "x0Pt": range.x0_pt,
                    "y0Pt": range.y0_pt,
                    "x1Pt": range.x1_pt,
                    "y1Pt": range.y1_pt,
                })
            }),
    );
    Value::Array(rectangles)
}

fn block_environment_exit_caret_x(
    block_environment_exit: Option<&(NodeId, caret_engine::Direction)>,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
    page_index: usize,
) -> Option<f64> {
    let (environment_id, direction) = block_environment_exit?;
    let mut ranges = snapshot
        .environment_ranges
        .iter()
        .filter(|range| &range.environment_id == environment_id)
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.page_index);
    match direction {
        caret_engine::Direction::Backward => ranges
            .into_iter()
            .find(|range| range.page_index == page_index)
            .map(|range| range.x0_pt),
        caret_engine::Direction::Forward => ranges
            .into_iter()
            .rev()
            .find(|range| range.page_index == page_index)
            .map(|range| range.x1_pt),
    }
}

/// 将虚拟环境外停靠点应用到所有返回路径共享的光标几何。
pub(super) fn caret_geometry_with_environment_exit(
    block_environment_exit: Option<(NodeId, caret_engine::Direction)>,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
    mut caret: PageCaretGeometry,
) -> PageCaretGeometry {
    caret.x_pt =
        block_environment_exit_caret_x(block_environment_exit.as_ref(), snapshot, caret.page_index)
            .unwrap_or(caret.x_pt);
    caret
}

fn block_environment_highlight_rects(
    environment_id: Option<&NodeId>,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Value {
    let Some(environment_id) = environment_id else {
        return Value::Array(Vec::new());
    };
    Value::Array(
        snapshot
            .environment_ranges
            .iter()
            .filter(|range| &range.environment_id == environment_id)
            .map(|range| {
                json!({
                    "pageIndex": range.page_index,
                    "x0Pt": range.x0_pt,
                    "y0Pt": range.y0_pt,
                    "x1Pt": range.x1_pt,
                    "y1Pt": range.y1_pt,
                })
            })
            .collect(),
    )
}

/// 将当前最内层行内环境投影为布局层选区矩形。
fn inline_environment_highlight_rects(
    range: Option<&caret_engine::InlineSpan>,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Value {
    let Some(range) = range else {
        return Value::Array(Vec::new());
    };
    let selection = Selection {
        anchor: Pos {
            story: range.story.clone(),
            paragraph_id: range.paragraph_id.clone(),
            offset: range.start_utf8,
            affinity: Affinity::Downstream,
        },
        focus: Pos {
            story: range.story.clone(),
            paragraph_id: range.paragraph_id.clone(),
            offset: range.end_utf8,
            affinity: Affinity::Upstream,
        },
    };
    Value::Array(
        snapshot
            .selection_geometry(&selection)
            .unwrap_or_default()
            .into_iter()
            .map(|rectangle| {
                json!({
                    "pageIndex": rectangle.page_index,
                    "x0Pt": rectangle.x0_pt,
                    "y0Pt": rectangle.y0_pt,
                    "x1Pt": rectangle.x1_pt,
                    "y1Pt": rectangle.y1_pt,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests;

impl DocumentViewRuntime {
    /// 只序列化命令查询，按键过滤不读取或重建布局快照。
    pub fn command_line_state_json(&self) -> Result<String, String> {
        serde_json::to_string(&command_line_state(&self.session))
            .map_err(|error| format!("cmdline.serialize: {error}"))
    }

    /// 按当前会话和字体代际生成界面快照 JSON。
    pub fn layout_snapshot_json(&mut self) -> Result<String, String> {
        self.layout_snapshot_json_with_delta(None)
    }

    /// 发布输入法取消结果，使界面只清除 Rust 实际取消的预编辑状态。
    pub(super) fn layout_snapshot_json_after_composition_cancel(
        &mut self,
    ) -> Result<String, String> {
        let snapshot = self.layout_snapshot_json()?;
        let mut view: Value = serde_json::from_str(&snapshot)
            .map_err(|error| format!("layout.serialize: {error}"))?;
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert("compositionCancelled".to_owned(), Value::Bool(true));
        serde_json::to_string(&view).map_err(|error| format!("layout.serialize: {error}"))
    }

    pub(super) fn layout_snapshot_json_with_delta(
        &mut self,
        changed_paragraph: Option<(&NodeId, usize, u64)>,
    ) -> Result<String, String> {
        if self.composition_snapshot.is_none() {
            self.base_snapshot()?;
        }
        let composing = self.composition_preview_session.is_some();
        let highlight_session = self
            .composition_preview_session
            .as_mut()
            .unwrap_or(&mut self.session);
        let environment_view = highlight_session
            .current_environment_view()
            .map_err(|error| format!("layout.inline_environment: {error}"))?;
        let inline_environment = environment_view
            .innermost_inline()
            .and_then(|entry| entry.inline_range.as_ref());
        let focus_environment_id = current_block_environment_id(highlight_session);
        let selection = self.session.selection().clone();
        let selected_environment_ids = self.session.selected_block_environments().to_vec();
        let structure_selection = self
            .session
            .structure_selection()
            .map(|(anchor, focus)| (anchor.clone(), focus.clone()));
        let snapshot = if composing {
            self.composition_snapshot
                .as_ref()
                .ok_or_else(|| "ime.composition_layout_unavailable".to_owned())?
        } else {
            self.last_snapshot
                .as_ref()
                .ok_or_else(|| "layout.snapshot_unavailable".to_owned())?
        };
        let mut view = match changed_paragraph {
            Some((paragraph_id, page_start_index, base_generation)) => serde_json::to_value(
                DocumentLayoutDeltaView::for_paragraph(
                    snapshot,
                    paragraph_id,
                    page_start_index,
                    base_generation,
                )
                .ok_or_else(|| "layout.changed_paragraph_unavailable".to_owned())?,
            ),
            None => serde_json::to_value(caret_engine::DocumentLayoutView::from(snapshot)),
        }
        .map_err(|error| format!("layout.serialize: {error}"))?;
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert("commandLine".to_owned(), command_line_state(&self.session));
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "currentStructure".to_owned(),
                current_structure_state(&self.session),
            );
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "blockEnvironmentHighlightRects".to_owned(),
                block_environment_highlight_rects(focus_environment_id.as_ref(), snapshot),
            );
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "inlineEnvironmentHighlightRects".to_owned(),
                inline_environment_highlight_rects(inline_environment, snapshot),
            );
        if composing && changed_paragraph.is_some() {
            view.as_object_mut()
                .ok_or_else(|| "layout.invalid_view_object".to_owned())?
                .insert(
                    "compositionPreview".to_owned(),
                    serde_json::Value::Bool(true),
                );
        }
        let rectangles = if composing {
            Value::Array(Vec::new())
        } else {
            selection_rectangles_json(
                &selection,
                structure_selection
                    .as_ref()
                    .map(|(anchor, focus)| (anchor, focus)),
                &selected_environment_ids,
                snapshot,
            )
        };
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert("selectionRects".to_owned(), rectangles);
        let composition_rectangles = if composing {
            self.composition_range
                .as_ref()
                .and_then(|range| snapshot.composition_geometry(range))
                .unwrap_or_default()
                .into_iter()
                .map(|rectangle| {
                    serde_json::json!({
                        "pageIndex": rectangle.page_index,
                        "x0Pt": rectangle.x0_pt,
                        "y0Pt": rectangle.y0_pt,
                        "x1Pt": rectangle.x1_pt,
                        "y1Pt": rectangle.y1_pt,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "compositionRects".to_owned(),
                serde_json::Value::Array(composition_rectangles),
            );
        let context = self
            .last_snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot.paragraph_by_id(&self.session.selection().focus.paragraph_id)
            })
            .ok_or_else(|| "ime.unknown_paragraph".to_owned())?;
        let logical_selection = self.session.selection();
        let cursor_position = context
            .utf16_offset(logical_selection.focus.offset)
            .ok_or_else(|| "ime.cursor_position: invalid UTF-8 boundary".to_owned())?;
        let anchor_position =
            if logical_selection.anchor.paragraph_id == logical_selection.focus.paragraph_id {
                context
                    .utf16_offset(logical_selection.anchor.offset)
                    .ok_or_else(|| "ime.anchor_position: invalid UTF-8 boundary".to_owned())?
            } else {
                cursor_position
            };
        let input_method_context = serde_json::json!({
            "surroundingText": context.text.as_ref(),
            "cursorPosition": cursor_position,
            "anchorPosition": anchor_position,
        });
        self.last_interaction_context =
            Some((snapshot.document_generation, context.id.clone(), false));
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert("inputMethodContext".to_owned(), input_method_context);
        serde_json::to_string(&view).map_err(|error| format!("layout.serialize: {error}"))
    }

    /// 只序列化 caret、选区与 IME 上下文，供不改变正文布局的交互更新使用。
    pub fn interaction_state_json(&mut self) -> Result<String, String> {
        let selection = self.session.selection().clone();
        let visual_affinity = self.session.selection_visual_affinity();
        let command_input_context = self.session.command_line();
        let context_key = (
            self.session.document_generation(),
            selection.focus.paragraph_id.clone(),
            command_input_context.is_some(),
        );
        let include_surrounding_text = self.last_interaction_context.as_ref() != Some(&context_key);
        let command_line = command_line_state(&self.session);
        let command_input_context = command_input_context.map(|line| {
            let mut surrounding_text = line.prefix().to_owned();
            surrounding_text.push_str(&line.query);
            (
                surrounding_text,
                line.prefix().encode_utf16().count()
                    + line.query[..line.cursor].encode_utf16().count(),
            )
        });
        let current_structure = current_structure_state(&self.session);
        let environment_view = self
            .composition_preview_session
            .as_mut()
            .unwrap_or(&mut self.session)
            .current_environment_view()
            .map_err(|error| format!("layout.inline_environment: {error}"))?;
        let inline_environment = environment_view
            .innermost_inline()
            .and_then(|entry| entry.inline_range.as_ref());
        let highlight_session = self
            .composition_preview_session
            .as_ref()
            .unwrap_or(&self.session);
        let focus_environment_id = current_block_environment_id(highlight_session);
        let block_environment_exit = self.session.block_environment_exit();
        let selected_environment_ids = self.session.selected_block_environments().to_vec();
        let structure_selection = self
            .session
            .structure_selection()
            .map(|(anchor, focus)| (anchor.clone(), focus.clone()));
        let snapshot = self.current_snapshot()?;
        let inline_environment_rectangles =
            inline_environment_highlight_rects(inline_environment, snapshot);
        let rectangles = selection_rectangles_json(
            &selection,
            structure_selection
                .as_ref()
                .map(|(anchor, focus)| (anchor, focus)),
            &selected_environment_ids,
            snapshot,
        );
        let focus = &selection.focus;
        let paragraph = snapshot
            .paragraph_by_id(&focus.paragraph_id)
            .ok_or_else(|| "ime.unknown_paragraph".to_owned())?;
        let cursor_position = paragraph
            .utf16_offset(focus.offset)
            .ok_or_else(|| "ime.cursor_position: invalid UTF-8 boundary".to_owned())?;
        let anchor_position = if selection.anchor.paragraph_id == focus.paragraph_id {
            paragraph
                .utf16_offset(selection.anchor.offset)
                .ok_or_else(|| "ime.anchor_position: invalid UTF-8 boundary".to_owned())?
        } else {
            cursor_position
        };
        let mut input_method_context = serde_json::Map::new();
        if include_surrounding_text {
            input_method_context.insert(
                "surroundingText".to_owned(),
                serde_json::Value::String(paragraph.text.to_string()),
            );
        }
        input_method_context.insert(
            "cursorPosition".to_owned(),
            serde_json::Value::from(cursor_position),
        );
        input_method_context.insert(
            "anchorPosition".to_owned(),
            serde_json::Value::from(anchor_position),
        );
        if let Some((query, cursor_utf16)) = command_input_context {
            input_method_context.insert(
                "surroundingText".to_owned(),
                serde_json::Value::String(query),
            );
            input_method_context.insert(
                "cursorPosition".to_owned(),
                serde_json::Value::from(cursor_utf16),
            );
            input_method_context.insert(
                "anchorPosition".to_owned(),
                serde_json::Value::from(cursor_utf16),
            );
        }
        let caret = snapshot
            .caret_geometry(focus, visual_affinity)
            .ok_or_else(|| "layout.invalid_caret_geometry".to_owned())?;
        let caret = caret_geometry_with_environment_exit(block_environment_exit, snapshot, caret);
        let font_size_centipoints = snapshot
            .font_size_centipoints_at(focus, visual_affinity)
            .ok_or_else(|| "layout.invalid_caret_font_size".to_owned())?;
        let serialized = serde_json::to_string(&serde_json::json!({
            "documentGeneration": snapshot.document_generation.to_string(),
            "fontGeneration": snapshot.font_generation.to_string(),
            "selectionRects": rectangles,
            "inputMethodContext": input_method_context,
            "commandLine": command_line,
            "currentStructure": current_structure,
            "blockEnvironmentHighlightRects": block_environment_highlight_rects(focus_environment_id.as_ref(), snapshot),
            "inlineEnvironmentHighlightRects": inline_environment_rectangles,
            "caretGeometry": {
                "pageIndex": caret.page_index,
                "xPt": caret.x_pt,
                "y0Pt": caret.y0_pt,
                "y1Pt": caret.y1_pt,
                "fontSizeCentipoints": font_size_centipoints,
            },
        }))
        .map_err(|error| format!("layout.interaction_serialize: {error}"))?;
        self.last_interaction_context = Some(context_key);
        Ok(serialized)
    }
}
