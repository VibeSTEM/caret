//! 工具安装布局、可执行文件查找及可执行权限。

use std::fs;
use std::path::{Component, Path, PathBuf};

/// 平台可执行名；测试运行器与本 crate 的工具定位共用同一后缀规则。
pub fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// 工具、Qt、SDK、归档和锁的根。`CARET_TOOLCHAIN_ROOT` 优先；未设置时是仓库根
/// `.toolchain/`。不读取 Cargo 的 `target/`，未设置时也不回退到那里。
pub fn toolchain_root() -> Result<PathBuf, String> {
    resolve_toolchain_root(
        std::env::var_os("CARET_TOOLCHAIN_ROOT").as_deref(),
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
}

pub(super) fn resolve_toolchain_root(
    override_value: Option<&std::ffi::OsStr>,
    manifest_dir: &Path,
) -> Result<PathBuf, String> {
    if let Some(value) = override_value {
        if value.is_empty() {
            return Err("CARET_TOOLCHAIN_ROOT 为空".into());
        }
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Ok(path);
        }
        if path.has_root() || matches!(path.components().next(), Some(Component::Prefix(_))) {
            return Err(
                "CARET_TOOLCHAIN_ROOT 必须为绝对路径或不带盘符、根前缀的仓库相对路径".into(),
            );
        }
        let repo = manifest_dir
            .parent()
            .and_then(Path::parent)
            .ok_or("无法从 caret-build 清单定位仓库根")?;
        // build script、runner 和 CMake 子进程的 cwd 不同；相对覆盖始终
        // 以仓库根解析，不能返回会随子进程 cwd 改变含义的工具路径。
        return Ok(repo.join(path));
    }
    let repo = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or("无法从 caret-build 清单定位仓库根")?;
    Ok(repo.join(".toolchain"))
}

/// `<toolchain>/tools/<name>/<id>/`。归档和锁在 toolchain 根下的 `archives/` 与 `locks/`，不放在这里。
pub(super) fn tools_root(toolchain_root: &Path) -> PathBuf {
    toolchain_root.join("tools")
}

/// 深度受限地查找 `bin/cmake`；返回第一个命中的路径。
pub(super) fn find_cmake_binary(dir: &Path, exe: &str, depth: usize) -> Option<PathBuf> {
    if depth > 4 {
        return None;
    }
    let bin_dir = dir.join("bin");
    let candidate = bin_dir.join(exe);
    if candidate.is_file() {
        return Some(candidate);
    }
    let entries = fs::read_dir(dir).ok()?;
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            paths.push(path);
        }
    }
    paths.sort();
    paths
        .iter()
        .find_map(|path| find_cmake_binary(path, exe, depth + 1))
}

/// 在 LLVM 解包目录中查找工具；官方压缩包在顶层带有版本目录，Windows
/// 归档和 Unix 归档的布局因此统一按相对路径处理。
pub(super) fn find_binary(dir: &Path, name: &str) -> Option<PathBuf> {
    let exe = exe_name(name);
    let mut directories = vec![(dir.to_owned(), 0usize)];
    while let Some((directory, depth)) = directories.pop() {
        if depth > 6 {
            continue;
        }
        for candidate in [directory.join(&exe), directory.join("bin").join(&exe)] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        let mut children = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        children.sort();
        for path in children {
            directories.push((path, depth + 1));
        }
    }
    None
}

pub(super) fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exe = exe_name(name);
    std::env::split_paths(&path)
        .map(|directory| directory.join(&exe))
        .find(|candidate| candidate.is_file())
}

pub(super) fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)
            .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
            .map_err(|error| format!("设置 {} 可执行位失败：{error}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        // Windows zip 内的 .exe 自带可执行语义，无需额外处理。
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_cmake_binary_in_nested_layout() {
        let dir = std::env::temp_dir().join(format!("caret-provision-tree-{}", std::process::id()));
        let nested = dir
            .join("cmake-4.4.3-macos-universal")
            .join("CMake.app")
            .join("Contents")
            .join("bin");
        fs::create_dir_all(&nested).unwrap_or_else(|error| panic!("mkdir failed: {error}"));
        let binary = nested.join(exe_name("cmake"));
        fs::write(&binary, b"MZ").unwrap_or_else(|error| panic!("write failed: {error}"));

        let found = find_cmake_binary(&dir, &exe_name("cmake"), 0);
        assert_eq!(found, Some(binary));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_binary_returns_none() {
        let dir =
            std::env::temp_dir().join(format!("caret-provision-empty-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("mkdir failed: {error}"));
        assert_eq!(find_cmake_binary(&dir, exe_name("cmake").as_str(), 0), None);
        assert_eq!(
            find_cmake_binary(
                &PathBuf::from("/nonexistent-caret"),
                exe_name("cmake").as_str(),
                0
            ),
            None
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn toolchain_root_uses_repo_dot_toolchain_without_target_fallback() {
        let repository = std::env::temp_dir().join("caret-toolchain-repo");
        let manifest = repository.join("crates/caret-build");
        let root = resolve_toolchain_root(None, &manifest)
            .unwrap_or_else(|error| panic!("resolve failed: {error}"));
        assert_eq!(root, repository.join(".toolchain"));
        assert!(resolve_toolchain_root(Some(std::ffi::OsStr::new("")), &manifest).is_err());
        let custom_root = std::env::temp_dir().join("caret-custom-tools");
        let custom = resolve_toolchain_root(Some(custom_root.as_os_str()), &manifest)
            .unwrap_or_else(|error| panic!("resolve failed: {error}"));
        assert_eq!(custom, custom_root);
    }

    #[test]
    fn relative_toolchain_override_is_anchored_to_the_repository() -> Result<(), String> {
        let repository = std::env::temp_dir().join("caret-relative-toolchain");
        let root = resolve_toolchain_root(
            Some(std::ffi::OsStr::new("shared/tools")),
            &repository.join("crates/caret-build"),
        )?;
        assert_eq!(root, repository.join("shared/tools"));
        assert!(root.is_absolute());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn ambiguous_windows_toolchain_roots_are_rejected() {
        let manifest = std::env::temp_dir().join("caret-repo/crates/caret-build");
        for value in [r"D:tools", r"\tools"] {
            assert!(resolve_toolchain_root(Some(std::ffi::OsStr::new(value)), &manifest).is_err());
        }
    }
}
