//! 组合文档编辑会话、文件路径与保存点。

use crate::document_view::DocumentViewRuntime;
use caret_doc::{CdocFile, Document, OpenedDocument, open};
use serde_json::json;
use std::path::{Path, PathBuf};

/// 一次保存捕获的不可变文档字节与对应修订。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSaveSnapshot {
    pub session_generation: u64,
    pub document_generation: u64,
    pub bytes: Vec<u8>,
}

/// 应用层文档会话；权威持有编辑视图、文件身份和成功保存点。
pub struct DocumentSessionService {
    view: DocumentViewRuntime,
    file: CdocFile,
    path: Option<PathBuf>,
    session_generation: u64,
    saved_generation: u64,
}

impl DocumentSessionService {
    /// 创建新的未命名文档。
    pub fn new_blank() -> Result<Self, String> {
        let file = CdocFile::new(Document::blank()).map_err(|error| error.to_string())?;
        let view = DocumentViewRuntime::from_document(file.document().clone())?;
        Ok(Self {
            view,
            file,
            path: None,
            session_generation: 1,
            saved_generation: 0,
        })
    }

    /// 返回文档视图给应用边界转发命令；领域修改仍由其 Rust Session 执行。
    pub fn view_mut(&mut self) -> &mut DocumentViewRuntime {
        &mut self.view
    }

    /// 返回当前会话代际，用于拒绝过期的后台打开结果。
    pub fn session_generation(&self) -> u64 {
        self.session_generation
    }

    /// 返回当前文档修订，用于拒绝过期的后台打开结果。
    pub fn document_generation(&self) -> u64 {
        self.view.document_generation()
    }

    /// 在 worker 中构造、解析并排版候选文档，不修改活动会话。
    pub fn prepare_open(
        bytes: &[u8],
        path: &Path,
        next_session_generation: u64,
    ) -> Result<(Self, String), String> {
        let opened = open(bytes).map_err(|error| error.to_string())?;
        let OpenedDocument::Editable(file) = opened else {
            return Err("cdoc.safe_preview_read_only".to_owned());
        };
        let mut candidate_view = DocumentViewRuntime::from_document(file.document().clone())?;
        let layout_snapshot = candidate_view.layout_snapshot_json()?;
        Ok((
            Self {
                view: candidate_view,
                file,
                path: Some(path.to_path_buf()),
                session_generation: next_session_generation,
                saved_generation: 0,
            },
            layout_snapshot,
        ))
    }

    /// 仅在会话身份和文档修订未改变时原子安装后台准备好的候选文档。
    pub fn install_prepared(
        &mut self,
        candidate: Self,
        layout_snapshot: String,
        expected_session_generation: u64,
        expected_document_generation: u64,
    ) -> Result<String, String> {
        if self.session_generation != expected_session_generation
            || self.view.document_generation() != expected_document_generation
        {
            return Err("document.open_stale_session".to_owned());
        }
        *self = candidate;
        Ok(layout_snapshot)
    }

    /// 同步打开入口供 Rust 集成测试使用；桌面端通过 worker 准备候选文档。
    #[cfg(test)]
    fn open_bytes(
        &mut self,
        bytes: &[u8],
        path: &Path,
        expected_session_generation: u64,
        expected_document_generation: u64,
    ) -> Result<String, String> {
        if self.session_generation != expected_session_generation
            || self.view.document_generation() != expected_document_generation
        {
            return Err("document.open_stale_session".to_owned());
        }
        let next_generation = self
            .session_generation
            .checked_add(1)
            .ok_or_else(|| "document.session_generation_exhausted".to_owned())?;
        let (candidate, snapshot) = Self::prepare_open(bytes, path, next_generation)?;
        self.install_prepared(
            candidate,
            snapshot,
            expected_session_generation,
            expected_document_generation,
        )
    }

    /// 捕获当前文档修订用于后台写入，不改变当前会话或保存点。
    pub fn save_snapshot(&self) -> Result<DocumentSaveSnapshot, String> {
        let file = CdocFile {
            format: self.file.format.clone(),
            version: self.file.version,
            required_features: self.file.required_features.clone(),
            media: self.file.media.clone(),
            document: self.view.document().clone(),
            extra: self.file.extra.clone(),
        };
        let bytes = file.to_bytes().map_err(|error| error.to_string())?;
        Ok(DocumentSaveSnapshot {
            session_generation: self.session_generation,
            document_generation: self.view.document_generation(),
            bytes,
        })
    }

    /// 只接受当前会话的成功写入回报；旧会话的迟到回报不改路径或保存点。
    pub fn mark_saved(
        &mut self,
        session_generation: u64,
        document_generation: u64,
        path: &Path,
    ) -> bool {
        if session_generation != self.session_generation {
            return false;
        }
        self.saved_generation = self.saved_generation.max(document_generation);
        self.path = Some(path.to_path_buf());
        true
    }

    /// 返回适合 ViewModel 投影的文件名、路径与脏状态。
    pub fn status_json(&self) -> String {
        let path = self.path.as_deref();
        let file_name = path
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled");
        json!({
            "sessionGeneration": self.session_generation.to_string(),
            "documentGeneration": self.view.document_generation().to_string(),
            "savedGeneration": self.saved_generation.to_string(),
            "dirty": self.view.document_generation() != self.saved_generation,
            "fileName": file_name,
            "path": path.and_then(Path::to_str).unwrap_or_default(),
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::DocumentSessionService;
    use caret_doc::CdocFile;
    use serde_json::Value;
    use serde_json::json;

    #[test]
    fn document_session_can_be_moved_between_worker_and_gui_threads() {
        fn assert_send<T: Send>() {}
        assert_send::<DocumentSessionService>();
    }

    #[test]
    fn failed_open_preserves_the_active_session_and_save_snapshot() -> Result<(), String> {
        let mut service = DocumentSessionService::new_blank()?;
        let original_snapshot = service.save_snapshot()?;
        let original_status: Value =
            serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;

        let result = service.open_bytes(b"not json", std::path::Path::new("bad.cdoc"), 1, 0);

        assert!(result.is_err());
        assert_eq!(service.save_snapshot()?, original_snapshot);
        let current_status: Value =
            serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
        assert_eq!(current_status, original_status);
        Ok(())
    }

    #[test]
    fn save_completion_only_marks_the_captured_revision_saved() -> Result<(), String> {
        let mut service = DocumentSessionService::new_blank()?;
        service.view_mut().layout_snapshot_json()?;
        service.view_mut().insert_text_json("first".to_owned())?;
        let captured = service.save_snapshot()?;
        service.view_mut().insert_text_json(" later".to_owned())?;

        assert!(service.mark_saved(
            captured.session_generation,
            captured.document_generation,
            std::path::Path::new("document.cdoc"),
        ));
        let status: Value =
            serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
        assert_eq!(status["dirty"], true);
        assert_eq!(status["fileName"], "document.cdoc");
        assert_eq!(
            status["savedGeneration"],
            captured.document_generation.to_string()
        );
        Ok(())
    }

    #[test]
    fn delayed_save_completion_from_replaced_session_is_ignored() -> Result<(), String> {
        let mut service = DocumentSessionService::new_blank()?;
        let stale = service.save_snapshot()?;
        let file =
            CdocFile::new(caret_doc::Document::blank()).map_err(|error| error.to_string())?;
        let bytes = file.to_bytes().map_err(|error| error.to_string())?;
        service.open_bytes(&bytes, std::path::Path::new("opened.cdoc"), 1, 0)?;

        assert!(!service.mark_saved(
            stale.session_generation,
            stale.document_generation,
            std::path::Path::new("stale.cdoc"),
        ));
        let status: Value =
            serde_json::from_str(&service.status_json()).map_err(|error| error.to_string())?;
        assert_eq!(status["fileName"], "opened.cdoc");
        assert_eq!(status["dirty"], false);
        Ok(())
    }

    #[test]
    fn editing_and_saving_preserves_unknown_optional_envelope_fields() -> Result<(), String> {
        let mut file =
            CdocFile::new(caret_doc::Document::blank()).map_err(|error| error.to_string())?;
        file.extra
            .insert("futureEnvelopeData".to_owned(), json!({"retained": true}));
        let bytes = file.to_bytes().map_err(|error| error.to_string())?;
        let mut service = DocumentSessionService::new_blank()?;
        service.open_bytes(&bytes, std::path::Path::new("future.cdoc"), 1, 0)?;
        service.view_mut().insert_text_json("edited".to_owned())?;

        let snapshot = service.save_snapshot()?;
        let saved: serde_json::Value =
            serde_json::from_slice(&snapshot.bytes).map_err(|error| error.to_string())?;
        assert_eq!(saved["futureEnvelopeData"]["retained"], true);
        assert_eq!(saved["document"]["body"][0]["inlines"][0]["text"], "edited");
        Ok(())
    }
}
