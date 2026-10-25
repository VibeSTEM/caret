//! 平台 SDK、native 子进程环境与 Cargo 输出目录；不维护第二份资产清单。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Qt 与 macOS 原生 SDK 制品共同支持的最低部署版本。
pub const MACOS_DEPLOYMENT_TARGET: &str = "13.0";

pub fn use_system_tools() -> bool {
    matches!(
        std::env::var("CARET_USE_SYSTEM_TOOLS").as_deref(),
        Ok("1" | "true" | "yes")
    )
}

/// Ninja 使用 Visual Studio 的 SDK/CRT 搜索环境，但编译器始终显式指定托管 clang-cl。
pub fn windows_sdk_env(
    target: &str,
) -> Result<Vec<(std::ffi::OsString, std::ffi::OsString)>, String> {
    if !target.contains("windows-msvc") {
        return Ok(Vec::new());
    }
    cc::windows_registry::find_tool(target, "cl.exe")
        .map(|tool| tool.env().to_vec())
        .ok_or_else(|| "未找到 MSVC Build Tools / Windows SDK；请安装平台 SDK 后重试".to_owned())
}

/// 将目录前置到环境变量列表的 PATH（大小写不敏感识别既有 PATH；列表缺失
/// 时回退进程 PATH），整体替换后下发。测试子进程与构建脚本共用。
pub fn prepend_path(
    mut environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    directories: Vec<PathBuf>,
) -> Result<Vec<(std::ffi::OsString, std::ffi::OsString)>, String> {
    let inherited = environment
        .iter()
        .find(|(key, _)| env_key_eq(key, "PATH"))
        .map(|(_, value)| value.clone())
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    let mut paths = directories;
    paths.extend(std::env::split_paths(&inherited));
    let path = std::env::join_paths(paths).map_err(|error| format!("拼接 PATH：{error}"))?;
    environment.retain(|(key, _)| !env_key_eq(key, "PATH"));
    environment.push((std::ffi::OsString::from("PATH"), path));
    Ok(environment)
}

/// 为 native 测试子进程补齐托管运行库的搜索路径；消费方包括 ctest/测试
/// 运行器，以及构建脚本自身——Windows 下 POST_BUILD gtest discovery 在
/// 链接后立即启动测试可执行文件，构建子进程必须能解析这些 DLL。
///
/// Windows 将托管 Qt runtime 与已安装 SDK 的运行库 bin 前置到继承的
/// PATH，兼容 SDK 内的嵌套 bin 布局；macOS/Linux 依靠构建 rpath 解析。
pub fn native_test_env(
    target: &str,
) -> Result<Vec<(std::ffi::OsString, std::ffi::OsString)>, String> {
    let environment = windows_sdk_env(target)?;
    if !cfg!(windows) {
        return Ok(environment);
    }

    let toolchain = crate::toolchain_root()?;
    let qt_bin = toolchain.join("qt").join("staging").join("bin");
    if !qt_bin.is_dir() {
        return Err(format!("托管 Qt 运行库目录不存在：{}", qt_bin.display()));
    }
    let mut paths = vec![qt_bin];
    paths.extend(windows_sdk_dll_dirs(&toolchain));
    prepend_path(environment, paths)
}

/// SDK 缓存（`<toolchain>/sdk/<name>/<version>/<triple>/`）里 Windows triple 的
/// 运行库目录：顶层与嵌套的 `bin/` 都收集；
/// triple 目录不存在或无 bin 时跳过，保持幂等。
fn windows_sdk_dll_dirs(toolchain: &Path) -> Vec<PathBuf> {
    let sdk_root = toolchain.join("sdk");
    let mut dirs = Vec::new();
    let Ok(names) = fs::read_dir(&sdk_root) else {
        return dirs;
    };
    for name in names.flatten() {
        let Ok(versions) = fs::read_dir(name.path()) else {
            continue;
        };
        for version in versions.flatten() {
            let Ok(triples) = fs::read_dir(version.path()) else {
                continue;
            };
            for triple in triples.flatten() {
                if !triple.file_name().to_string_lossy().starts_with("windows") {
                    continue;
                }
                collect_dll_dirs(&triple.path(), 0, &mut dirs);
            }
        }
    }
    dirs.sort();
    dirs
}

/// 递归收集 `bin` 命名的目录，其余子目录最多下探 4 层，避免异常目录树
/// 导致无界遍历。
fn collect_dll_dirs(dir: &Path, depth: u8, dirs: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().is_some_and(|name| name == "bin") {
            dirs.push(path);
        } else {
            collect_dll_dirs(&path, depth + 1, dirs);
        }
    }
}

fn env_key_eq(key: &std::ffi::OsStr, expected: &str) -> bool {
    key.to_string_lossy().eq_ignore_ascii_case(expected)
}

/// 官方 LLVM 不替代 Apple SDK；显式 sysroot 保证 CXX 和 CMake 使用同一套平台头文件。
pub fn macos_sdk() -> Result<Option<PathBuf>, String> {
    if !cfg!(target_os = "macos") {
        return Ok(None);
    }
    let output = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .map_err(|e| format!("无法定位 Apple SDK：{e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    if !path.is_dir() {
        return Err(format!("Apple SDK 不存在：{}", path.display()));
    }
    Ok(Some(path))
}

/// OUT_DIR 可能包含显式 --target 的 triple 层；拒绝未验证的跨目标构建。
pub fn target_root(out: &Path) -> Result<PathBuf, String> {
    let host = std::env::var("HOST").map_err(|e| e.to_string())?;
    let target = std::env::var("TARGET").map_err(|e| e.to_string())?;
    output_root(out, &host, &target)
}

fn output_root(out: &Path, host: &str, target: &str) -> Result<PathBuf, String> {
    if host != target {
        return Err(format!("尚未支持交叉编译：{host} -> {target}"));
    }
    let mut root = out.ancestors().nth(4).ok_or("无法解析 OUT_DIR")?;
    if root.file_name().is_some_and(|name| name == target) {
        root = root.parent().ok_or("target 根缺失")?;
    }
    Ok(root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_sdk_dll_dirs_only_keeps_windows_triples_with_bin() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("caret-sdk-dll-{}", std::process::id()));
        let sdk = root.join("sdk");
        // 正常布局：sdk/<name>/<version>/<triple>/[bin|lib]。
        let vtk = sdk.join("vtk/9.7.0");
        for triple in ["windows-x86_64", "linux-x86_64", "macos-arm64"] {
            fs::create_dir_all(vtk.join(triple).join("lib")).map_err(|e| e.to_string())?;
        }
        fs::create_dir_all(vtk.join("windows-x86_64/bin")).map_err(|e| e.to_string())?;
        // 另一 SDK 同布局；windows triple 缺 bin 时不收录。
        fs::create_dir_all(sdk.join("dawn/1.0.0/windows-x86_64/bin")).map_err(|e| e.to_string())?;
        fs::create_dir_all(sdk.join("dawn/1.0.0/macos-arm64/bin")).map_err(|e| e.to_string())?;
        // 嵌套运行库布局：非 bin 中间目录照常下探。
        let occt = sdk.join("occt/8.0.1/windows-x86_64");
        fs::create_dir_all(occt.join("win64/vc14/bin")).map_err(|e| e.to_string())?;
        let dirs = windows_sdk_dll_dirs(&root);
        let expected = vec![
            sdk.join("dawn/1.0.0/windows-x86_64/bin"),
            occt.join("win64/vc14/bin"),
            vtk.join("windows-x86_64/bin"),
        ];
        assert_eq!(dirs, expected);
        fs::remove_dir_all(&root).map_err(|e| e.to_string())?;
        Ok(())
    }

    #[test]
    fn environment_keys_are_case_insensitive() {
        assert!(env_key_eq(std::ffi::OsStr::new("PATH"), "Path"));
        assert!(env_key_eq(std::ffi::OsStr::new("Path"), "PATH"));
        assert!(!env_key_eq(std::ffi::OsStr::new("PATHEXT"), "PATH"));
    }

    #[test]
    fn path_prepend_replaces_case_variants_and_keeps_other_environment() -> Result<(), String> {
        let base = std::env::temp_dir();
        let first = base.join("first dir");
        let inherited = base.join("inherited");
        let discarded = base.join("discarded");
        let environment = vec![
            (
                "Path".into(),
                std::env::join_paths([&inherited]).map_err(|e| e.to_string())?,
            ),
            (
                "PATH".into(),
                std::env::join_paths([&discarded]).map_err(|e| e.to_string())?,
            ),
            ("KEEP".into(), "value".into()),
        ];
        let result = prepend_path(environment, vec![first.clone()])?;
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ("KEEP".into(), "value".into()));
        assert_eq!(result[1].0, std::ffi::OsString::from("PATH"));
        assert_eq!(
            std::env::split_paths(&result[1].1).collect::<Vec<_>>(),
            vec![first, inherited]
        );
        Ok(())
    }

    #[test]
    fn cargo_output_root_handles_explicit_target_and_rejects_cross_builds() -> Result<(), String> {
        let root = PathBuf::from("workspace").join("target");
        let host = "fixture-host";
        for out in [
            root.join("debug/build/hash/out"),
            root.join(host).join("release/build/hash/out"),
        ] {
            assert_eq!(output_root(&out, host, host)?, root);
        }
        assert_eq!(
            output_root(Path::new("out"), host, host),
            Err("无法解析 OUT_DIR".to_owned())
        );
        assert_eq!(
            output_root(Path::new("out"), host, "other"),
            Err("尚未支持交叉编译：fixture-host -> other".to_owned())
        );
        Ok(())
    }
}
