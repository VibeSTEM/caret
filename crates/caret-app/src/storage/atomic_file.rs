//! 同目录临时写入与原子替换；替换后的目录同步失败作为持久性警告返回。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
const TEMP_NAME_ATTEMPTS: usize = 128;

#[derive(Debug)]
pub(crate) enum AtomicFileError {
    Io(std::io::Error),
    MissingParent,
    MissingFileName,
}

/// 能返回此结果就表示目标已替换；警告仅表示目录同步无法确认崩溃持久性。
#[derive(Debug)]
pub(crate) struct AtomicWriteOutcome {
    pub(crate) durability_warning: Option<std::io::Error>,
}

pub(crate) fn write(path: &Path, payload: &[u8]) -> Result<AtomicWriteOutcome, AtomicFileError> {
    write_with(
        path,
        payload,
        |file, bytes| file.write_all(bytes),
        sync_parent,
    )
}

fn write_with(
    path: &Path,
    payload: &[u8],
    write_payload: impl FnOnce(&mut fs::File, &[u8]) -> std::io::Result<()>,
    sync_directory: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<AtomicWriteOutcome, AtomicFileError> {
    let parent = path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .ok_or(AtomicFileError::MissingParent)?;
    let file_name = path.file_name().ok_or(AtomicFileError::MissingFileName)?;
    fs::create_dir_all(parent).map_err(AtomicFileError::Io)?;

    let (temporary, mut file) = create_temp_file(parent, file_name)?;
    let written = write_payload(&mut file, payload).and_then(|()| file.sync_all());
    drop(file);
    let replaced = written.and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = replaced {
        let _ = fs::remove_file(&temporary);
        return Err(AtomicFileError::Io(error));
    }
    // rename 是提交点；此后不能返回“未写入”，否则内存与目标文件会发生分歧。
    Ok(AtomicWriteOutcome {
        durability_warning: sync_directory(parent).err(),
    })
}

fn create_temp_file(
    parent: &Path,
    file_name: &OsStr,
) -> Result<(PathBuf, fs::File), AtomicFileError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    for _ in 0..TEMP_NAME_ATTEMPTS {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let mut name = OsString::from(".");
        name.push(file_name);
        name.push(format!(
            ".{}.{timestamp:x}.{sequence:x}.tmp",
            std::process::id()
        ));
        let path = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(AtomicFileError::Io(error)),
        }
    }
    Err(AtomicFileError::Io(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "temporary file names exhausted",
    )))
}

fn sync_parent(parent: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
