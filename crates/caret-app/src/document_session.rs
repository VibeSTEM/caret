//! 组合文档编辑会话、文件路径与保存点。

use crate::document_view::DocumentViewRuntime;
use caret_doc::{CdocFile, Document, OpenedDocument, open};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// 一次保存捕获的不可变文档字节与对应修订。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSaveSnapshot {
    pub session_generation: u64,
    pub document_generation: u64,
    pub history_state_id: u64,
    pub bytes: Vec<u8>,
    pub content_digest: String,
    pub baseline_digest: Option<String>,
    pub recovery_id: Option<String>,
    pub recovery_original_path: Option<String>,
    pub recovery_baseline_digest: Option<String>,
}

/// 应用层文档会话；权威持有编辑视图、文件身份和成功保存点。
pub struct DocumentSessionService {
    view: DocumentViewRuntime,
    file: DocumentFileMetadata,
    path: Option<PathBuf>,
    session_generation: u64,
    saved_generation: u64,
    saved_history_state_id: u64,
    has_save_point: bool,
    baseline_digest: Option<String>,
    recovery_id: Option<String>,
    recovery_original_path: Option<String>,
    recovery_baseline_digest: Option<String>,
}

/// 保存文档信封信息，正文只由编辑会话持有，避免打开时复制整棵文档树。
struct DocumentFileMetadata {
    format: String,
    version: u32,
    required_features: Vec<String>,
    media: Vec<caret_doc::Media>,
    extra: std::collections::BTreeMap<String, serde_json::Value>,
}

impl DocumentFileMetadata {
    fn from_file(file: CdocFile) -> (Self, Document) {
        let CdocFile {
            format,
            version,
            required_features,
            media,
            document,
            extra,
        } = file;
        (
            Self {
                format,
                version,
                required_features,
                media,
                extra,
            },
            document,
        )
    }
}

impl DocumentSessionService {
    /// 创建新的未命名文档。
    pub fn new_blank() -> Result<Self, String> {
        let file = CdocFile::new(Document::blank()).map_err(|error| error.to_string())?;
        let (file, document) = DocumentFileMetadata::from_file(file);
        let view = DocumentViewRuntime::from_document(document)?;
        Ok(Self {
            view,
            file,
            path: None,
            session_generation: 1,
            saved_generation: 0,
            saved_history_state_id: 0,
            has_save_point: true,
            baseline_digest: None,
            recovery_id: None,
            recovery_original_path: None,
            recovery_baseline_digest: None,
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

    /// 当前文档内容是否偏离最近一次成功保存点。
    pub fn is_dirty(&self) -> bool {
        !self.has_save_point || self.view.history_state_id() != self.saved_history_state_id
    }

    /// 当前临时恢复会话绑定的恢复项标识。
    pub fn recovery_id(&self) -> Option<&str> {
        self.recovery_id.as_deref()
    }

    /// 恢复文档成功保存到用户路径后清除其恢复项绑定。
    pub fn clear_recovery_id(&mut self) -> Option<String> {
        self.recovery_original_path = None;
        self.recovery_baseline_digest = None;
        self.recovery_id.take()
    }

    /// 绑定冲突时原子写入的本地恢复项；只允许当前会话改变绑定。
    pub fn set_recovery_id(
        &mut self,
        session_generation: u64,
        id: String,
        original_path: String,
        baseline_digest: Option<String>,
    ) -> bool {
        if session_generation != self.session_generation {
            return false;
        }
        self.recovery_id = Some(id);
        self.recovery_original_path = Some(original_path);
        self.recovery_baseline_digest = baseline_digest;
        true
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
        let (file, document) = DocumentFileMetadata::from_file(file);
        let mut candidate_view = DocumentViewRuntime::from_document(document)?;
        let layout_snapshot = candidate_view.layout_snapshot_json()?;
        Ok((
            Self {
                view: candidate_view,
                file,
                path: Some(path.to_path_buf()),
                session_generation: next_session_generation,
                saved_generation: 0,
                saved_history_state_id: 0,
                has_save_point: true,
                baseline_digest: Some(content_digest(bytes)),
                recovery_id: None,
                recovery_original_path: None,
                recovery_baseline_digest: None,
            },
            layout_snapshot,
        ))
    }

    /// 将冲突恢复快照打开为未命名且 dirty 的临时会话。
    pub fn prepare_recovery(
        bytes: &[u8],
        recovery_id: String,
        recovery_original_path: String,
        recovery_baseline_digest: Option<String>,
        next_session_generation: u64,
    ) -> Result<(Self, String), String> {
        let OpenedDocument::Editable(file) = open(bytes).map_err(|error| error.to_string())? else {
            return Err("cdoc.safe_preview_read_only".to_owned());
        };
        let (file, document) = DocumentFileMetadata::from_file(file);
        let mut view = DocumentViewRuntime::from_document(document)?;
        let history_state_id = view.history_state_id();
        let layout_snapshot = view.layout_snapshot_json()?;
        Ok((
            Self {
                view,
                file,
                path: None,
                session_generation: next_session_generation,
                saved_generation: 0,
                saved_history_state_id: history_state_id,
                has_save_point: false,
                baseline_digest: None,
                recovery_id: Some(recovery_id),
                recovery_original_path: Some(recovery_original_path),
                recovery_baseline_digest,
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
        self.install_prepared_session(
            candidate,
            expected_session_generation,
            expected_document_generation,
        )?;
        Ok(layout_snapshot)
    }

    /// 仅安装后台准备好的候选会话；快照可由调用方另行异步解析。
    pub fn install_prepared_session(
        &mut self,
        candidate: Self,
        expected_session_generation: u64,
        expected_document_generation: u64,
    ) -> Result<(), String> {
        if self.session_generation != expected_session_generation
            || self.view.document_generation() != expected_document_generation
        {
            return Err("document.open_stale_session".to_owned());
        }
        *self = candidate;
        Ok(())
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
    pub fn save_snapshot(&mut self) -> Result<DocumentSaveSnapshot, String> {
        self.view.begin_save_checkpoint();
        let file = CdocFile {
            format: self.file.format.clone(),
            version: self.file.version,
            required_features: self.file.required_features.clone(),
            media: self.file.media.clone(),
            document: self.view.document().clone(),
            extra: self.file.extra.clone(),
        };
        let bytes = file.to_bytes().map_err(|error| error.to_string())?;
        let content_digest = content_digest(&bytes);
        Ok(DocumentSaveSnapshot {
            session_generation: self.session_generation,
            document_generation: self.view.document_generation(),
            history_state_id: self.view.history_state_id(),
            bytes,
            content_digest,
            baseline_digest: self.baseline_digest.clone(),
            recovery_id: self.recovery_id.clone(),
            recovery_original_path: self.recovery_original_path.clone(),
            recovery_baseline_digest: self.recovery_baseline_digest.clone(),
        })
    }

    /// 只接受当前会话的成功写入回报；旧会话的迟到回报不改路径或保存点。
    pub fn mark_saved(
        &mut self,
        session_generation: u64,
        document_generation: u64,
        history_state_id: u64,
        path: &Path,
        content_digest: &str,
    ) -> bool {
        if session_generation != self.session_generation {
            return false;
        }
        // 旧修订的迟到写入不能回退当前文档路径或外部文件基线。
        if document_generation < self.saved_generation {
            return true;
        }
        self.saved_generation = document_generation;
        self.saved_history_state_id = history_state_id;
        self.has_save_point = true;
        self.path = Some(path.to_path_buf());
        self.baseline_digest = Some(content_digest.to_owned());
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
            "dirty": self.is_dirty(),
            "historyStateId": self.view.history_state_id().to_string(),
            "canUndo": self.view.can_undo(),
            "canRedo": self.view.can_redo(),
            "fileName": file_name,
            "path": path.and_then(Path::to_str).unwrap_or_default(),
            "recoveryId": self.recovery_id.as_deref().unwrap_or_default(),
        })
        .to_string()
    }
}

pub fn content_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests;
