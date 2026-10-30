//! Settings 快照的 JSON 读写与无效文件隔离，原子替换复用应用存储机制。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{SettingsError, SettingsService, UserSettings};
use crate::storage::atomic_file::{self, AtomicFileError};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const ISOLATION_NAME_ATTEMPTS: usize = 1000;

/// 设置文件没有被接受。隔离成功时内存快照保持载入前的值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsFileError {
    Io(String),
    Isolated {
        quarantined: PathBuf,
        reason: String,
    },
}

impl fmt::Display for SettingsFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "{message}"),
            Self::Isolated {
                quarantined,
                reason,
            } => {
                write!(
                    formatter,
                    "settings.isolated: {}: {reason}",
                    quarantined.display()
                )
            }
        }
    }
}

impl std::error::Error for SettingsFileError {}

/// 设置提交的候选校验错误或提交点之前的持久化错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsUpdateError {
    Validation(SettingsError),
    Persistence(SettingsFileError),
}

impl fmt::Display for SettingsUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => write!(formatter, "{error}"),
            Self::Persistence(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SettingsUpdateError {}

/// 返回此结果表示目标文件已替换；目录同步警告不撤销已完成的提交。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSaveOutcome {
    pub durability_warning: Option<String>,
}

impl SettingsService {
    /// 文件不存在时保留当前快照；无效内容隔离成功后仍报告诊断，不发布候选。
    pub fn load_file(&mut self, path: &Path) -> Result<(), SettingsFileError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(storage_error(error)),
        };
        let candidate = match serde_json::from_slice::<UserSettings>(&bytes) {
            Ok(candidate) => candidate,
            Err(error) => return Err(isolation_error(path, &bytes, error.to_string())),
        };
        if let Err(error) = self.commit(candidate) {
            return Err(isolation_error(path, &bytes, error.to_string()));
        }
        Ok(())
    }

    /// 同目录临时文件同步并替换目标后返回成功；提交前失败保留原目标。
    pub fn save_file(&self, path: &Path) -> Result<SettingsSaveOutcome, SettingsFileError> {
        let payload = serde_json::to_vec_pretty(&self.current()).map_err(|error| {
            SettingsFileError::Io(format!("settings.storage_format_error: {error}"))
        })?;
        let outcome = atomic_file::write(path, &payload).map_err(|error| match error {
            AtomicFileError::Io(error) => storage_error(error),
            AtomicFileError::MissingParent => {
                SettingsFileError::Io("settings.storage_access_error: missing parent".to_owned())
            }
            AtomicFileError::MissingFileName => {
                SettingsFileError::Io("settings.storage_access_error: missing filename".to_owned())
            }
        })?;
        Ok(SettingsSaveOutcome {
            durability_warning: outcome
                .durability_warning
                .map(|error| format!("settings.storage_durability_warning: {error}")),
        })
    }
}

fn storage_error(error: std::io::Error) -> SettingsFileError {
    SettingsFileError::Io(format!("settings.storage_access_error: {error}"))
}

fn isolation_error(path: &Path, bytes: &[u8], reason: String) -> SettingsFileError {
    match quarantine_settings_file(path, bytes) {
        Ok(quarantined) => SettingsFileError::Isolated {
            quarantined,
            reason,
        },
        Err(error) => error,
    }
}

fn quarantine_settings_file(path: &Path, bytes: &[u8]) -> Result<PathBuf, SettingsFileError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            SettingsFileError::Io("settings.storage_access_error: missing parent".to_owned())
        })?;
    let file_name = path.file_name().ok_or_else(|| {
        SettingsFileError::Io("settings.storage_access_error: missing filename".to_owned())
    })?;
    for index in 0..ISOLATION_NAME_ATTEMPTS {
        let mut suffix = file_name.to_os_string();
        suffix.push(if index == 0 {
            ".isolated".to_owned()
        } else {
            format!(".isolated-{index}")
        });
        let destination = parent.join(suffix);
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(storage_error(error)),
        };
        let copied = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = copied {
            let _ = fs::remove_file(&destination);
            return Err(storage_error(error));
        }
        match fs::remove_file(path) {
            Ok(()) => return Ok(destination),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(destination),
            Err(error) => return Err(storage_error(error)),
        }
    }
    Err(SettingsFileError::Io(
        "settings.storage_access_error: isolation names exhausted".to_owned(),
    ))
}
