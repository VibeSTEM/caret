//! 序列化完整布局与轻量交互快照。
use super::DocumentViewRuntime;
use caret_doc::NodeId;
use caret_engine::{DocumentLayoutDeltaView, StructureKind, StructurePosition};
use serde_json::{Value, json};

fn command_line_state(session: &caret_engine::Session) -> Value {
    session
        .command_line()
        .map(|command_line| {
            let candidates = command_line
                .candidates()
                .into_iter()
                .map(|candidate| {
                    json!({
                        "alias": candidate.alias,
                        "commandId": candidate.command_id,
                        "available": candidate.available,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "active": true,
                "query": &command_line.query,
                "cursor": command_line.cursor,
                "selected": command_line.selected,
                "candidates": candidates,
            })
        })
        .unwrap_or_else(|| {
            json!({
                "active": false,
                "query": "",
                "cursor": 0,
                "selected": 0,
                "candidates": [],
            })
        })
}

fn current_structure_state(session: &caret_engine::Session) -> Value {
    let focus = &session.selection().focus;
    let stack = session.structure_index().at(&StructurePosition {
        paragraph_id: focus.paragraph_id.clone(),
        offset_utf8: focus.offset,
    });
    Value::Array(
        stack
            .into_iter()
            .map(|entry| match entry.kind {
                StructureKind::Heading { level } => json!({
                    "id": entry.id,
                    "kind": "heading",
                    "level": level,
                }),
                StructureKind::Paragraph => json!({
                    "id": entry.id,
                    "kind": "paragraph",
                }),
                StructureKind::Section => json!({
                    "id": entry.id,
                    "kind": "section",
                }),
                StructureKind::Showcase => json!({
                    "id": entry.id,
                    "kind": "showcase",
                }),
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
        if composing && changed_paragraph.is_some() {
            view.as_object_mut()
                .ok_or_else(|| "layout.invalid_view_object".to_owned())?
                .insert(
                    "compositionPreview".to_owned(),
                    serde_json::Value::Bool(true),
                );
        }
        let rectangles = if composing {
            Vec::new()
        } else {
            snapshot
                .selection_geometry(self.session.selection())
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
        };
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "selectionRects".to_owned(),
                serde_json::Value::Array(rectangles),
            );
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
        self.last_interaction_context = Some((snapshot.document_generation, context.id.clone()));
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert("inputMethodContext".to_owned(), input_method_context);
        serde_json::to_string(&view).map_err(|error| format!("layout.serialize: {error}"))
    }

    /// 只序列化 caret、选区与 IME 上下文，供不改变正文布局的交互更新使用。
    pub fn interaction_state_json(&mut self) -> Result<String, String> {
        let selection = self.session.selection().clone();
        let visual_affinity = self.session.selection_visual_affinity();
        let context_key = (
            self.session.document_generation(),
            selection.focus.paragraph_id.clone(),
        );
        let include_surrounding_text = self.last_interaction_context.as_ref() != Some(&context_key);
        let command_line = command_line_state(&self.session);
        let current_structure = current_structure_state(&self.session);
        let snapshot = self.current_snapshot()?;
        let rectangles = snapshot
            .selection_geometry(&selection)
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
            .collect::<Vec<_>>();
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
        let caret = snapshot
            .caret_geometry(focus, visual_affinity)
            .ok_or_else(|| "layout.invalid_caret_geometry".to_owned())?;
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
