//! 为界面提供由 Rust 文档会话生成的只读布局快照。

use caret_doc::Document;
use caret_engine::{
    DocumentLayoutRuntime, DocumentLayoutSnapshot, PageGlyphRasterizer, Selection, Session,
    VisualAffinity,
};

mod composition;
mod editing;
mod hit_test;
mod layout;
mod navigation;
mod selection;
mod snapshot;
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
    last_interaction_context: Option<(u64, caret_doc::NodeId, bool)>,
}

impl DocumentViewRuntime {
    /// 创建空白文档会话并初始化一次系统字体目录。
    pub fn new_blank() -> Result<Self, String> {
        Self::new(Document::blank())
    }

    pub(crate) fn new(document: Document) -> Result<Self, String> {
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

    pub(crate) fn from_document(document: Document) -> Result<Self, String> {
        Self::new(document)
    }

    /// 当前文档内容修订代际；导航与绘制状态变化不推进此值。
    pub fn document_generation(&self) -> u64 {
        self.session.document_generation()
    }

    pub fn history_state_id(&self) -> u64 {
        self.session.history_state_id()
    }

    pub fn can_undo(&self) -> bool {
        self.session.composition().is_none() && self.session.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.session.composition().is_none() && self.session.can_redo()
    }

    /// 保存快照前切断连续输入合并，不推进文档 revision 或保存状态。
    pub fn begin_save_checkpoint(&mut self) {
        self.session.begin_save_checkpoint();
    }

    /// 当前文档树的只读视图，供应用服务捕获保存快照。
    pub fn document(&self) -> &Document {
        self.session.document()
    }
}
