//! 为界面提供由 Rust 文档会话生成的只读布局快照。

use caret_doc::Document;
use caret_doc::NodeId;
use caret_engine::{
    Command, Direction, DocumentLayoutDeltaView, DocumentLayoutRuntime, DocumentLayoutSnapshot,
    PageCaretView, PageGlyphRasterizer, RasterizedPageGlyphs, Selection, Session, VisualAffinity,
};

mod composition;
mod navigation;
mod selection;
#[cfg(test)]
mod tests;

/// 当前文档会话与可复用的字体/布局运行时。
pub struct DocumentViewRuntime {
    session: Session,
    layout: DocumentLayoutRuntime,
    glyph_rasterizer: PageGlyphRasterizer,
    last_snapshot: Option<DocumentLayoutSnapshot>,
    preferred_visual_x_pt: Option<f32>,
    anchor_visual_affinity: VisualAffinity,
    composition_id: Option<u64>,
    next_composition_id: u64,
    preedit_cursor_position_utf16: usize,
    composition_preview_session: Option<Session>,
    composition_snapshot: Option<DocumentLayoutSnapshot>,
    composition_range: Option<Selection>,
    last_interaction_context: Option<(u64, caret_doc::NodeId)>,
}

impl DocumentViewRuntime {
    /// 创建空白文档会话并初始化一次系统字体目录。
    pub fn new_blank() -> Result<Self, String> {
        Self::new(Document::blank())
    }

    fn new(document: Document) -> Result<Self, String> {
        let session = Session::new(document).map_err(|error| error.to_string())?;
        let layout = DocumentLayoutRuntime::new_system().map_err(|error| error.to_string())?;
        Ok(Self {
            session,
            layout,
            glyph_rasterizer: PageGlyphRasterizer::default(),
            last_snapshot: None,
            preferred_visual_x_pt: None,
            anchor_visual_affinity: VisualAffinity::Downstream,
            composition_id: None,
            next_composition_id: 1,
            preedit_cursor_position_utf16: 0,
            composition_preview_session: None,
            composition_snapshot: None,
            composition_range: None,
            last_interaction_context: None,
        })
    }

    /// 按当前会话和字体代际生成界面快照 JSON。
    pub fn layout_snapshot_json(&mut self) -> Result<String, String> {
        self.layout_snapshot_json_with_delta(None)
    }

    fn layout_snapshot_json_with_delta(
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
            "surroundingText": context.text,
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
                serde_json::Value::String(paragraph.text.clone()),
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
        let serialized = serde_json::to_string(&serde_json::json!({
            "documentGeneration": snapshot.document_generation.to_string(),
            "fontGeneration": snapshot.font_generation.to_string(),
            "selectionRects": rectangles,
            "inputMethodContext": input_method_context,
            "caretGeometry": {
                "pageIndex": caret.page_index,
                "xPt": caret.x_pt,
                "y0Pt": caret.y0_pt,
                "y1Pt": caret.y1_pt,
            },
        }))
        .map_err(|error| format!("layout.interaction_serialize: {error}"))?;
        self.last_interaction_context = Some(context_key);
        Ok(serialized)
    }

    /// 将 Qt 直接输入的文本交给 Rust Session，再返回新布局快照。
    pub fn insert_text_json(&mut self, text: String) -> Result<String, String> {
        self.insert_text_with_response(text, false)
    }

    /// 为 Qt 普通单段输入返回完整分页和当前段落布局的增量快照。
    pub fn insert_text_delta_json(&mut self, text: String) -> Result<String, String> {
        self.insert_text_with_response(text, true)
    }

    fn insert_text_with_response(
        &mut self,
        text: String,
        allow_delta: bool,
    ) -> Result<String, String> {
        self.base_snapshot()?;
        let selection = self.session.selection();
        let reusable_paragraph_id = (selection.anchor.paragraph_id == selection.focus.paragraph_id)
            .then(|| selection.focus.paragraph_id.clone());
        self.session
            .run(Command::InsertText(text))
            .map_err(|error| format!("text.insert: {error}"))?;
        self.anchor_visual_affinity = self.session.selection_visual_affinity();
        self.preferred_visual_x_pt = None;
        let mut page_start_index = None;
        let mut base_generation = None;
        if let Some(paragraph_id) = reusable_paragraph_id.as_ref() {
            let previous = self
                .last_snapshot
                .as_ref()
                .ok_or_else(|| "layout.snapshot_unavailable".to_owned())?;
            base_generation = Some(previous.document_generation);
            let updated = self
                .layout
                .layout_session_reusing(&self.session, previous, paragraph_id)
                .map_err(|error| error.to_string())?;
            page_start_index = DocumentLayoutDeltaView::first_affected_page_index(
                previous,
                &updated,
                paragraph_id,
            );
            if page_start_index.is_none() {
                return Err("layout.changed_paragraph_page_unavailable".to_owned());
            }
            self.last_snapshot = Some(updated);
        } else {
            self.last_snapshot = None;
        }
        match reusable_paragraph_id.as_ref().filter(|_| allow_delta) {
            Some(paragraph_id) => self.layout_snapshot_json_with_delta(Some((
                paragraph_id,
                page_start_index.ok_or_else(|| "layout.changed_page_unavailable".to_owned())?,
                base_generation
                    .ok_or_else(|| "layout.delta_base_generation_unavailable".to_owned())?,
            ))),
            None => self.layout_snapshot_json(),
        }
    }

    /// 将纯文本剪贴板内容作为一次原子事务插入，换行会拆为段落。
    pub fn paste_text_json(&mut self, text: String) -> Result<String, String> {
        self.run_edit_command(Command::PasteText(text), "clipboard.paste")
    }

    /// 在 Rust Session 中拆分当前段落并返回新布局。
    pub fn split_paragraph_json(&mut self) -> Result<String, String> {
        self.run_edit_command(Command::SplitParagraph, "text.split_paragraph")
    }

    /// 在当前插入点删除前一个字素；位于段首时与前一段合并。
    pub fn delete_backward_json(&mut self) -> Result<String, String> {
        self.run_edit_command(Command::Delete(Direction::Backward), "text.delete_backward")
    }

    /// 在当前段落内插入软换行，不创建新段落。
    pub fn insert_soft_break_json(&mut self) -> Result<String, String> {
        self.run_edit_command(Command::InsertSoftBreak, "text.soft_break")
    }

    fn run_edit_command(
        &mut self,
        command: Command,
        error_context: &str,
    ) -> Result<String, String> {
        let selection = self.session.selection();
        let reusable_paragraph_id = (selection.anchor.paragraph_id == selection.focus.paragraph_id)
            .then(|| selection.focus.paragraph_id.clone());
        let can_reuse_layout =
            matches!(&command, Command::SplitParagraph | Command::InsertSoftBreak)
                || matches!(&command, Command::PasteText(_)) && reusable_paragraph_id.is_some();
        if can_reuse_layout {
            self.base_snapshot()?;
        }
        self.session
            .run(command)
            .map_err(|error| format!("{error_context}: {error}"))?;
        self.anchor_visual_affinity = self.session.selection_visual_affinity();
        self.preferred_visual_x_pt = None;
        if can_reuse_layout && let Some(paragraph_id) = reusable_paragraph_id.as_ref() {
            let previous = self
                .last_snapshot
                .as_ref()
                .ok_or_else(|| "layout.snapshot_unavailable".to_owned())?;
            let updated = self
                .layout
                .layout_session_reusing(&self.session, previous, paragraph_id)
                .map_err(|error| error.to_string())?;
            self.last_snapshot = Some(updated);
        }
        self.layout_snapshot_json()
    }

    fn composition_preview_delta_for(
        &self,
        updated: &DocumentLayoutSnapshot,
        range: &Selection,
    ) -> Option<(NodeId, usize, u64)> {
        let selection = self.session.selection();
        if selection.anchor.paragraph_id != selection.focus.paragraph_id {
            return None;
        }
        if range.anchor.paragraph_id != range.focus.paragraph_id {
            return None;
        }
        let previous = self.last_snapshot.as_ref()?;
        let paragraph_id = range.anchor.paragraph_id.clone();
        let mut page_start_index = first_changed_page_index(previous, updated, &paragraph_id)?;
        if let Some(previous_preview) = self.composition_snapshot.as_ref() {
            let preview_page_start =
                first_changed_page_index(previous_preview, updated, &paragraph_id)?;
            page_start_index = page_start_index.min(preview_page_start);
        }
        Some((paragraph_id, page_start_index, previous.document_generation))
    }

    /// 返回当前 Rust 光标位置在最新布局中的页面几何。
    pub fn current_caret_json(&mut self) -> Result<String, String> {
        let display_session = self
            .composition_preview_session
            .as_ref()
            .unwrap_or(&self.session);
        let position = display_session.selection().focus.clone();
        let visual_affinity = display_session.selection_visual_affinity();
        let caret = self
            .current_snapshot()?
            .caret_geometry(&position, visual_affinity)
            .map(PageCaretView::from);
        serde_json::to_string(&caret).map_err(|error| format!("layout.caret_serialize: {error}"))
    }

    /// 栅格化指定页；调用方只请求当前可见页，并提供页面原点的设备像素坐标。
    pub fn rasterize_page_glyphs(
        &mut self,
        page_index: usize,
        px_per_pt: f64,
        page_origin_device_px: [f64; 2],
        default_foreground: [u8; 4],
    ) -> Result<RasterizedPageGlyphs, String> {
        let page = self
            .current_snapshot()?
            .page_glyphs(page_index)
            .map_err(|error| error.to_string())?;
        self.glyph_rasterizer
            .rasterize(&page, px_per_pt, page_origin_device_px, default_foreground)
            .map_err(|error| error.to_string())
    }

    /// 在当前已发布或惰性重建的快照中命中，并返回稳定的 Rust 文档位置。
    pub fn hit_test_json(
        &mut self,
        page_index: i32,
        x_pt: f64,
        y_pt: f64,
    ) -> Result<String, String> {
        let hit = match usize::try_from(page_index) {
            Ok(page_index) => self
                .current_snapshot()?
                .hit_test_view(page_index, x_pt, y_pt),
            Err(_) => None,
        };
        serde_json::to_string(&hit).map_err(|error| format!("layout.hit_test_serialize: {error}"))
    }

    /// 在 Rust 会话中提交页面命中的插入点，并返回提交位置与输入代际。
    pub fn place_caret_at_page_json(
        &mut self,
        page_index: i32,
        x_pt: f64,
        y_pt: f64,
    ) -> Result<String, String> {
        let page_index = match usize::try_from(page_index) {
            Ok(page_index) => page_index,
            Err(_) => return Ok("null".to_owned()),
        };
        let (hit, result) = {
            let snapshot = self.current_snapshot()?;
            let Some(hit) = snapshot.hit_test(page_index, x_pt, y_pt) else {
                return Ok("null".to_owned());
            };
            (hit.clone(), snapshot.view_for_hit(&hit))
        };
        let position = hit.position;
        self.session
            .set_selection_with_visual_affinity(
                Some(Selection {
                    anchor: position.clone(),
                    focus: position,
                }),
                hit.visual_affinity,
            )
            .map_err(|error| format!("selection.set: {error}"))?;
        self.anchor_visual_affinity = hit.visual_affinity;
        self.preferred_visual_x_pt = None;
        serde_json::to_string(&result)
            .map_err(|error| format!("layout.place_caret_serialize: {error}"))
    }

    fn current_snapshot(&mut self) -> Result<&DocumentLayoutSnapshot, String> {
        if self.composition_preview_session.is_some() {
            return self
                .composition_snapshot
                .as_ref()
                .ok_or_else(|| "ime.composition_layout_unavailable".to_owned());
        }
        self.base_snapshot()
    }

    fn relayout_reusing_paragraph(
        &mut self,
        previous: &DocumentLayoutSnapshot,
        changed_paragraph_id: &NodeId,
    ) -> Result<(), String> {
        let snapshot = self
            .layout
            .layout_session_reusing(&self.session, previous, changed_paragraph_id)
            .map_err(|error| error.to_string())?;
        self.last_snapshot = Some(snapshot);
        Ok(())
    }

    fn base_snapshot(&mut self) -> Result<&DocumentLayoutSnapshot, String> {
        let font_generation = self.layout.font_generation();
        let current = self
            .last_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.is_current_for(&self.session, font_generation));
        if !current {
            self.last_snapshot = Some(
                self.layout
                    .layout_session(&self.session)
                    .map_err(|error| error.to_string())?,
            );
        }
        self.last_snapshot
            .as_ref()
            .ok_or_else(|| "layout.snapshot_unavailable".to_owned())
    }

    fn composition_snapshot_for(
        &mut self,
        preview_session: &Session,
        composition_range: &Selection,
    ) -> Result<DocumentLayoutSnapshot, String> {
        self.base_snapshot()?;
        let changed_paragraph_id = composition_range.anchor.paragraph_id.clone();
        let mut snapshot = self
            .layout
            .layout_session_reusing(
                preview_session,
                self.last_snapshot
                    .as_ref()
                    .ok_or_else(|| "layout.snapshot_unavailable".to_owned())?,
                &changed_paragraph_id,
            )
            .map_err(|error| error.to_string())?;
        snapshot.session_id = Some(self.session.session_id().clone());
        snapshot.document_generation = self.session.document_generation();
        Ok(snapshot)
    }
}

fn first_changed_page_index(
    previous: &DocumentLayoutSnapshot,
    current: &DocumentLayoutSnapshot,
    paragraph_id: &NodeId,
) -> Option<usize> {
    DocumentLayoutDeltaView::first_affected_page_index(previous, current, paragraph_id)
}
