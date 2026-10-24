//! 在同一文件系统中准备完整目录后替换构建产物集合。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

/// 将命名文件集合发布为一个目录；准备或切换失败时恢复旧目录。
pub fn replace_directory_with_files(
    target: &Path,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    if files.is_empty() {
        return Err("拒绝发布空目录".to_owned());
    }
    for name in files.keys() {
        if !is_single_file_name(name) {
            return Err(format!("非法的目录内文件名：{name:?}"));
        }
    }

    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let directory_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("目标目录名不是有效 UTF-8：{}", target.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("创建父目录 {} 失败：{error}", parent.display()))?;

    let staging = unique_sibling(parent, directory_name, "staging");
    fs::create_dir(&staging)
        .map_err(|error| format!("创建暂存目录 {} 失败：{error}", staging.display()))?;
    for (name, contents) in files {
        let destination = staging.join(name);
        if let Err(error) = fs::write(&destination, contents) {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!(
                "写入暂存文件 {} 失败：{error}",
                destination.display()
            ));
        }
    }

    let had_previous = match fs::metadata(target) {
        Ok(metadata) if metadata.is_dir() => true,
        Ok(_) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("目标存在但不是目录：{}", target.display()));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("读取目标目录 {} 失败：{error}", target.display()));
        }
    };

    let backup = unique_sibling(parent, directory_name, "backup");
    if had_previous && let Err(error) = fs::rename(target, &backup) {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("暂存旧目录 {} 失败：{error}", target.display()));
    }
    if let Err(error) = fs::rename(&staging, target) {
        let restore = if had_previous {
            fs::rename(&backup, target).err()
        } else {
            None
        };
        let _ = fs::remove_dir_all(&staging);
        return Err(match restore {
            Some(restore_error) => format!(
                "发布目录 {} 失败：{error}；恢复旧目录也失败：{restore_error}",
                target.display()
            ),
            None => format!("发布目录 {} 失败：{error}", target.display()),
        });
    }
    if had_previous && let Err(error) = fs::remove_dir_all(&backup) {
        eprintln!("清理已替换的构建目录 {} 失败：{error}", backup.display());
    }
    Ok(())
}

fn is_single_file_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && Path::new(name)
            .file_name()
            .is_some_and(|file_name| file_name == name)
}

fn unique_sibling(parent: &Path, directory_name: &str, role: &str) -> PathBuf {
    loop {
        let sequence = NEXT_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{directory_name}.{role}.{}.{sequence}",
            std::process::id()
        ));
        if !candidate.exists() {
            return candidate;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::collections::BTreeMap;
    use std::error::Error;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::replace_directory_with_files;

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn create() -> Result<Self, std::io::Error> {
            loop {
                let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "caret-build-publish-{}-{sequence}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Ok(Self(path)),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn replaces_a_complete_file_set_and_removes_stale_files() -> Result<(), Box<dyn Error>> {
        let temporary = TestDirectory::create()?;
        let target = temporary.0.join("i18n");
        fs::create_dir(&target)?;
        fs::write(target.join("stale.ts"), b"old")?;
        let files = BTreeMap::from([
            ("caret_en.ts".to_owned(), b"english".to_vec()),
            ("caret_zh_CN.ts".to_owned(), b"chinese".to_vec()),
        ]);

        replace_directory_with_files(&target, &files).map_err(std::io::Error::other)?;

        assert!(!target.join("stale.ts").exists());
        assert_eq!(fs::read(target.join("caret_en.ts"))?, b"english");
        assert_eq!(fs::read(target.join("caret_zh_CN.ts"))?, b"chinese");
        Ok(())
    }

    #[test]
    fn rejects_invalid_file_names_without_touching_the_previous_directory()
    -> Result<(), Box<dyn Error>> {
        let temporary = TestDirectory::create()?;
        let target = temporary.0.join("i18n");
        fs::create_dir(&target)?;
        fs::write(target.join("existing.ts"), b"previous")?;
        let files = BTreeMap::from([("../outside.ts".to_owned(), b"invalid".to_vec())]);

        assert!(replace_directory_with_files(&target, &files).is_err());
        assert_eq!(fs::read(target.join("existing.ts"))?, b"previous");
        assert!(!temporary.0.join("outside.ts").exists());
        Ok(())
    }
}
