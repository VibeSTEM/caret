//! 冲突与显式覆盖前的本机恢复 envelope 原子存储。

use crate::storage::atomic_file::{self, AtomicFileError};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use fs4::FileExt;
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::fs::{self, File, OpenOptions};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ENVELOPE_SCHEMA: u32 = 1;
const RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// 恢复副本负载种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryKind {
    LocalDocument,
    ExternalVersion,
}

/// 单文件恢复信封；负载 Base64 编码以兼容原始外部文件字节。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryEnvelope {
    pub schema: u32,
    pub kind: RecoveryKind,
    pub original_path: String,
    pub baseline_digest: Option<String>,
    pub created_unix_seconds: u64,
    #[serde(default)]
    pub updated_unix_seconds: u64,
    pub payload_base64: String,
}

/// 启动恢复列表所需的元数据，不加载可能很大的文档负载。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryEntry {
    pub id: String,
    pub kind: RecoveryKind,
    pub original_path: String,
    pub baseline_digest: Option<String>,
    pub created_unix_seconds: u64,
    pub updated_unix_seconds: u64,
}

/// 进程级共享租约；持有期间其他实例不会清理恢复目录中的过期条目。
pub struct RecoveryProcessLease {
    _file: File,
}

/// 活动恢复会话的跨进程条目租约；持有期间其他实例不能恢复或删除该项。
pub struct RecoverySessionLease {
    id: String,
    directory: PathBuf,
    _file: File,
}

fn active_session_ids() -> &'static Mutex<std::collections::HashSet<String>> {
    static ACTIVE: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

impl Drop for RecoverySessionLease {
    fn drop(&mut self) {
        if let Ok(mut ids) = active_session_ids().lock() {
            ids.remove(&active_session_key(&self.directory, &self.id));
        }
    }
}

impl RecoverySessionLease {
    /// 返回此会话租约持有的恢复条目标识。
    pub fn id(&self) -> &str {
        &self.id
    }

    fn belongs_to(&self, directory: &Path) -> bool {
        self.directory == directory
    }
}

fn active_session_key(directory: &Path, id: &str) -> String {
    format!("{}::{id}", directory.display())
}

#[derive(Deserialize)]
struct RecoveryMetadata {
    schema: u32,
    kind: RecoveryKind,
    original_path: String,
    baseline_digest: Option<String>,
    created_unix_seconds: u64,
    #[serde(default)]
    updated_unix_seconds: u64,
    #[serde(rename = "payload_base64")]
    _payload: IgnoredAny,
}

/// 恢复存储错误；不允许失败时静默丢弃冲突内容。
#[derive(Debug)]
pub enum RecoveryError {
    Io(std::io::Error),
    Busy,
    InvalidEnvelope(String),
    MissingParent,
    MissingFileName,
}

impl Display for RecoveryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "recovery.io_error: {error}"),
            Self::Busy => write!(formatter, "recovery.entry_busy"),
            Self::InvalidEnvelope(error) => write!(formatter, "recovery.invalid_envelope: {error}"),
            Self::MissingParent => write!(formatter, "recovery.missing_parent"),
            Self::MissingFileName => write!(formatter, "recovery.missing_filename"),
        }
    }
}

impl std::error::Error for RecoveryError {}

impl From<std::io::Error> for RecoveryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<AtomicFileError> for RecoveryError {
    fn from(error: AtomicFileError) -> Self {
        match error {
            AtomicFileError::Io(error) => Self::Io(error),
            AtomicFileError::MissingParent => Self::MissingParent,
            AtomicFileError::MissingFileName => Self::MissingFileName,
        }
    }
}

/// 独立管理 AppLocalDataLocation/recovery，不混入普通 `.cdoc` 存储策略。
#[derive(Debug, Clone)]
pub struct DocumentRecoveryStorage {
    directory: PathBuf,
}

impl DocumentRecoveryStorage {
    /// 创建恢复目录服务；宿主应传入 Qt AppLocalDataLocation 下的 `recovery`。
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// 获取恢复目录共享租约；清理程序使用同一锁的排他模式。
    pub fn acquire_process_lease(&self) -> Result<RecoveryProcessLease, RecoveryError> {
        fs::create_dir_all(&self.directory)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.directory.join(".recovery.lock"))?;
        FileExt::lock_shared(&file)?;
        Ok(RecoveryProcessLease { _file: file })
    }

    /// 将文档或外部文件字节原子保存为单文件 envelope，返回稳定条目标识。
    pub fn store(
        &self,
        kind: RecoveryKind,
        original_path: &Path,
        baseline_digest: Option<String>,
        payload: &[u8],
    ) -> Result<String, RecoveryError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let created = now.as_secs();
        let id = format!(
            "{:032x}-{:08x}-{:016x}",
            now.as_nanos(),
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        );
        let envelope = RecoveryEnvelope {
            schema: ENVELOPE_SCHEMA,
            kind,
            original_path: original_path.to_string_lossy().into_owned(),
            baseline_digest,
            created_unix_seconds: created,
            updated_unix_seconds: created,
            payload_base64: STANDARD.encode(payload),
        };
        let bytes = serde_json::to_vec(&envelope)
            .map_err(|error| RecoveryError::InvalidEnvelope(error.to_string()))?;
        atomic_file::write(&self.directory.join(format!("{id}.json")), &bytes)?;
        Ok(id)
    }

    /// 为当前文档会话写入并立即锁定恢复副本。
    pub fn store_for_session(
        &self,
        original_path: &Path,
        baseline_digest: Option<String>,
        payload: &[u8],
    ) -> Result<(String, RecoverySessionLease), RecoveryError> {
        let _operation_lock = self.lock_operations()?;
        let id = self.store(
            RecoveryKind::LocalDocument,
            original_path,
            baseline_digest,
            payload,
        )?;
        match self.acquire_entry_lease(&id) {
            Ok(lease) => Ok((id, lease)),
            Err(error) => {
                let _ = fs::remove_file(self.directory.join(format!("{id}.json")));
                Err(error)
            }
        }
    }

    /// 读取并校验恢复 envelope 与其二进制负载。
    pub fn load(&self, id: &str) -> Result<(RecoveryEnvelope, Vec<u8>), RecoveryError> {
        validate_id(id)?;
        let _operation_lock = self.lock_operations()?;
        let _entry_lease = self.lock_entry(id, false)?;
        self.load_unlocked(id)
    }

    fn load_unlocked(&self, id: &str) -> Result<(RecoveryEnvelope, Vec<u8>), RecoveryError> {
        let bytes = fs::read(self.directory.join(format!("{id}.json")))?;
        let envelope: RecoveryEnvelope = serde_json::from_slice(&bytes)
            .map_err(|error| RecoveryError::InvalidEnvelope(error.to_string()))?;
        if envelope.schema != ENVELOPE_SCHEMA {
            return Err(RecoveryError::InvalidEnvelope(
                "unsupported schema".to_owned(),
            ));
        }
        let payload = STANDARD
            .decode(&envelope.payload_base64)
            .map_err(|error| RecoveryError::InvalidEnvelope(error.to_string()))?;
        Ok((envelope, payload))
    }

    /// 锁定恢复来源并读取正文；验证完成前不移除原条目。
    pub fn load_for_restore(
        &self,
        id: &str,
    ) -> Result<(RecoveryEnvelope, Vec<u8>, RecoverySessionLease), RecoveryError> {
        validate_id(id)?;
        let _operation_lock = self.lock_operations()?;
        let lease = self.acquire_entry_lease(id)?;
        let (envelope, payload) = self.load_unlocked(id)?;
        if envelope.kind != RecoveryKind::LocalDocument {
            return Err(RecoveryError::InvalidEnvelope(
                "entry is not a local document".to_owned(),
            ));
        }

        Ok((envelope, payload, lease))
    }

    /// 为已验证的恢复会话创建独立 envelope，并保持其条目锁直到会话结束。
    pub fn fork_for_session(
        &self,
        source: &RecoveryEnvelope,
        payload: &[u8],
    ) -> Result<(String, RecoverySessionLease), RecoveryError> {
        let _operation_lock = self.lock_operations()?;
        let session_id = self.store(
            RecoveryKind::LocalDocument,
            Path::new(&source.original_path),
            source.baseline_digest.clone(),
            payload,
        )?;
        match self.acquire_entry_lease(&session_id) {
            Ok(lease) => Ok((session_id, lease)),
            Err(error) => {
                let _ = fs::remove_file(self.directory.join(format!("{session_id}.json")));
                Err(error)
            }
        }
    }

    /// 仅允许持有该条目租约的会话删除自己的恢复副本。
    pub fn remove_leased(&self, lease: &RecoverySessionLease) -> Result<(), RecoveryError> {
        if !lease.belongs_to(&self.directory) {
            return Err(RecoveryError::InvalidEnvelope(
                "recovery lease belongs to another store".to_owned(),
            ));
        }
        let _operation_lock = self.lock_operations()?;
        self.remove_file_unlocked(lease.id())
    }

    fn remove_file_unlocked(&self, id: &str) -> Result<(), RecoveryError> {
        validate_id(id)?;
        match fs::remove_file(self.directory.join(format!("{id}.json"))) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(RecoveryError::Io(error)),
        }
    }

    fn lock_operations(&self) -> Result<File, RecoveryError> {
        fs::create_dir_all(&self.directory)?;
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.directory.join(".recovery-operations.lock"))?;
        match FileExt::try_lock(&lock_file) {
            Ok(()) => Ok(lock_file),
            Err(fs4::TryLockError::WouldBlock) => Err(RecoveryError::Busy),
            Err(fs4::TryLockError::Error(error)) => Err(RecoveryError::Io(error)),
        }
    }

    fn acquire_entry_lease(&self, id: &str) -> Result<RecoverySessionLease, RecoveryError> {
        let key = active_session_key(&self.directory, id);
        if active_session_ids()
            .lock()
            .map_err(|_| RecoveryError::Busy)?
            .contains(&key)
        {
            return Err(RecoveryError::Busy);
        }
        let lock_directory = self.directory.join(".active");
        fs::create_dir_all(&lock_directory)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_directory.join(format!("{id}.lock")))?;
        match FileExt::try_lock(&file) {
            Ok(()) => {
                let mut active = active_session_ids()
                    .lock()
                    .map_err(|_| RecoveryError::Busy)?;
                if !active.insert(key) {
                    return Err(RecoveryError::Busy);
                }
                Ok(RecoverySessionLease {
                    id: id.to_owned(),
                    directory: self.directory.clone(),
                    _file: file,
                })
            }
            Err(fs4::TryLockError::WouldBlock) => Err(RecoveryError::Busy),
            Err(fs4::TryLockError::Error(error)) => Err(RecoveryError::Io(error)),
        }
    }

    /// 为已有恢复条目建立会话租约，避免其他应用实例同时恢复或删除。
    pub fn acquire_session_lease(&self, id: &str) -> Result<RecoverySessionLease, RecoveryError> {
        validate_id(id)?;
        if !self.directory.join(format!("{id}.json")).is_file() {
            return Err(RecoveryError::InvalidEnvelope(
                "recovery entry does not exist".to_owned(),
            ));
        }
        self.acquire_entry_lease(id)
    }

    fn lock_entry(
        &self,
        id: &str,
        allow_owned_session: bool,
    ) -> Result<Option<RecoverySessionLease>, RecoveryError> {
        if allow_owned_session
            && active_session_ids()
                .lock()
                .map_err(|_| RecoveryError::Busy)?
                .contains(&active_session_key(&self.directory, id))
        {
            return Ok(None);
        }
        self.acquire_entry_lease(id).map(Some)
    }

    fn entry_is_active(&self, id: &str) -> bool {
        if active_session_ids()
            .lock()
            .is_ok_and(|active| active.contains(&active_session_key(&self.directory, id)))
        {
            return true;
        }
        let path = self.directory.join(".active").join(format!("{id}.lock"));
        let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
            return false;
        };
        match FileExt::try_lock(&file) {
            Ok(()) => false,
            Err(fs4::TryLockError::WouldBlock | fs4::TryLockError::Error(_)) => true,
        }
    }

    /// 将用户确认的外部原始版本原子导出到显式选择的路径。
    pub fn export_external(&self, id: &str, target: &Path) -> Result<(), RecoveryError> {
        let (envelope, payload) = self.load(id)?;
        if envelope.kind != RecoveryKind::ExternalVersion {
            return Err(RecoveryError::InvalidEnvelope(
                "entry is not an external version".to_owned(),
            ));
        }
        atomic_file::write(target, &payload)?;
        Ok(())
    }

    /// 列出可读取的恢复条目标识；无效条目保留以便诊断，不阻塞其他条目。
    pub fn list(&self) -> Result<Vec<String>, RecoveryError> {
        let mut ids = Vec::new();
        if !self.directory.exists() {
            return Ok(ids);
        }
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
                && let Some(id) = path.file_stem().and_then(|name| name.to_str())
                && validate_id(id).is_ok()
            {
                ids.push(id.to_owned());
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// 流式读取恢复项元数据；忽略可能很大的负载，避免启动时复制正文。
    pub fn list_entries(&self) -> Result<Vec<RecoveryEntry>, RecoveryError> {
        let mut entries = Vec::new();
        for id in self.list()? {
            if self.entry_is_active(&id) {
                continue;
            }
            let Ok(file) = fs::File::open(self.directory.join(format!("{id}.json"))) else {
                continue;
            };
            let Ok(metadata) = serde_json::from_reader::<_, RecoveryMetadata>(BufReader::new(file))
            else {
                continue;
            };
            if metadata.schema != ENVELOPE_SCHEMA {
                continue;
            }
            entries.push(RecoveryEntry {
                id,
                kind: metadata.kind,
                original_path: metadata.original_path,
                baseline_digest: metadata.baseline_digest,
                created_unix_seconds: metadata.created_unix_seconds,
                updated_unix_seconds: metadata
                    .updated_unix_seconds
                    .max(metadata.created_unix_seconds),
            });
        }
        Ok(entries)
    }

    /// 更新同一恢复项的内容、最后更新时间和稳定标识。
    pub fn update(
        &self,
        id: &str,
        kind: RecoveryKind,
        original_path: &Path,
        baseline_digest: Option<String>,
        payload: &[u8],
    ) -> Result<(), RecoveryError> {
        validate_id(id)?;
        let _operation_lock = self.lock_operations()?;
        let _entry_lease = self.lock_entry(id, true)?;
        let mut envelope: RecoveryEnvelope =
            serde_json::from_slice(&fs::read(self.directory.join(format!("{id}.json")))?)
                .map_err(|error| RecoveryError::InvalidEnvelope(error.to_string()))?;
        if envelope.schema != ENVELOPE_SCHEMA {
            return Err(RecoveryError::InvalidEnvelope(
                "unsupported schema".to_owned(),
            ));
        }
        envelope.kind = kind;
        envelope.original_path = original_path.to_string_lossy().into_owned();
        envelope.baseline_digest = baseline_digest;
        envelope.updated_unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        envelope.payload_base64 = STANDARD.encode(payload);
        let bytes = serde_json::to_vec(&envelope)
            .map_err(|error| RecoveryError::InvalidEnvelope(error.to_string()))?;
        atomic_file::write(&self.directory.join(format!("{id}.json")), &bytes)?;
        Ok(())
    }

    /// 显式丢弃恢复条目。
    pub fn remove(&self, id: &str) -> Result<(), RecoveryError> {
        validate_id(id)?;
        let _operation_lock = self.lock_operations()?;
        let _entry_lease = self.lock_entry(id, false)?;
        self.remove_file_unlocked(id)
    }

    /// 仅删除过期的 Caret envelope；清理错误由启动方记录并忽略。
    pub fn purge_expired(&self, now_unix_seconds: u64) -> Result<usize, RecoveryError> {
        if !self.directory.exists() {
            return Ok(0);
        }
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.directory.join(".recovery.lock"))?;
        match FileExt::try_lock(&lock_file) {
            Ok(()) => {}
            Err(fs4::TryLockError::WouldBlock) => return Ok(0),
            Err(fs4::TryLockError::Error(error)) => return Err(RecoveryError::Io(error)),
        }
        let expired_ids: Vec<_> = self
            .list_entries()?
            .into_iter()
            .filter(|entry| {
                now_unix_seconds.saturating_sub(entry.updated_unix_seconds) > RETENTION.as_secs()
            })
            .map(|entry| entry.id)
            .collect();
        let mut removed = 0;
        for id in expired_ids {
            match fs::remove_file(self.directory.join(format!("{id}.json"))) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(RecoveryError::Io(error)),
            }
        }
        Ok(removed)
    }
}

fn validate_id(id: &str) -> Result<(), RecoveryError> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return Err(RecoveryError::InvalidEnvelope(
            "invalid identifier".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
