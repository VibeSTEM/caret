//! Rust / C++ / CMake / QML 格式入口及源码清单。

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::command::{cargo, cargo_command, run, run_scanner};
use crate::paths::repository_root;

pub(super) fn format_all(
    check: bool,
    changed: bool,
    rest: &[String],
) -> Result<(), Box<dyn Error>> {
    if let Some(unexpected) = rest.first() {
        return Err(format!(
            "format 不接受位置参数 '{unexpected}'；修复为缺省行为，\
                            --check 只验证不改动"
        )
        .into());
    }
    if changed {
        return format_changed(check);
    }
    if check {
        cargo("fmt", ["--all", "--", "--check"])?;
    } else {
        cargo("fmt", ["--all"])?;
    }
    run_pa_format(check)?;
    check_cpp_format(check)?;
    run_cmake_format(check)?;
    run_qml_format(check)?;
    if check {
        println!("format 检查通过");
    } else {
        println!("format 完成");
    }
    Ok(())
}

fn format_changed(check: bool) -> Result<(), Box<dyn Error>> {
    if crate::test_scope::force_full_scope() {
        return format_all(check, false, &[]);
    }
    let root = repository_root()?;
    let paths = crate::test_scope::changed_paths()?;
    if paths.is_empty() {
        println!("变更格式检查：没有待检查的文件");
        return Ok(());
    }

    let rust_packages = crate::test_scope::changed_format_packages()?;
    for package in rust_packages {
        let mut command = Command::new("cargo");
        command.arg("fmt").arg("--package").arg(&package);
        if check {
            command.args(["--", "--check"]);
        }
        command.current_dir(root);
        run(&format!("cargo fmt ({package})"), command)?;
    }

    let cpp_format_all = crate::test_scope::cpp_format_all()?;
    if cpp_format_all {
        check_cpp_format(check)?;
    }
    let cmake_format_all = crate::test_scope::cmake_format_all()?;
    if cmake_format_all {
        run_cmake_format(check)?;
    }

    let changed_files = paths
        .iter()
        .map(|path| root.join(path))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    let pa_files = files_with_extensions(&changed_files, &["pa"])
        .into_iter()
        .filter(|path| is_pa_source_path(root, path))
        .collect::<Vec<_>>();
    if !pa_files.is_empty() {
        run_pa_formatter(check, &pa_files)?;
    }
    let cpp_files = changed_files
        .iter()
        .filter(|path| {
            caret_build::database::is_source(path) || caret_build::database::is_header(path)
        })
        .filter(|path| {
            [
                "native",
                "tests/cpp",
                "tests/qml",
                "tests/performance",
                "tests/support",
                "crates/caret-ffi/src",
                "crates/caret-ffi/include",
            ]
            .iter()
            .any(|directory| path.starts_with(root.join(directory)))
        })
        .cloned()
        .collect::<Vec<_>>();
    if !cpp_format_all && !cpp_files.is_empty() {
        format_cpp_files(check, &cpp_files)?;
    }
    let cmake_files = changed_files
        .iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name == "CMakeLists.txt")
                || path
                    .extension()
                    .is_some_and(|extension| extension == "cmake")
        })
        .filter(|path| {
            ["native", "qml", "tools"]
                .iter()
                .any(|directory| path.starts_with(root.join(directory)))
        })
        .cloned()
        .collect::<Vec<_>>();
    if !cmake_format_all && !cmake_files.is_empty() {
        let mut command = caret_build::python::command(root, "cmake-format")?;
        if check {
            command.args(["--check"]);
        } else {
            command.args(["--in-place"]);
        }
        command.args(&cmake_files);
        run(
            &format!("cmake-format（{} 个变更文件）", cmake_files.len()),
            command,
        )?;
    }
    let qml_files = files_with_extensions(&changed_files, &["qml"])
        .into_iter()
        .filter(|path| {
            ["qml", "tests/qml", "tests/fixtures/qml"]
                .iter()
                .any(|directory| path.starts_with(root.join(directory)))
        })
        .collect::<Vec<_>>();
    if !qml_files.is_empty() {
        format_qml_files(check, &qml_files)?;
    }
    println!("变更文件格式检查通过");
    Ok(())
}

fn files_with_extensions(files: &[PathBuf], extensions: &[&str]) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extensions.contains(&extension))
        })
        .cloned()
        .collect()
}

fn run_pa_formatter(check: bool, files: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let (description, mut build) = cargo_command("build", ["--locked", "--package", "caret-dslc"])?;
    build.arg("--target").arg(env!("CARET_TEST_HOST"));
    run(&description, build)?;
    let profile = if env!("CARET_TEST_BUILD_TYPE") == "Release" {
        "release"
    } else {
        "debug"
    };
    let binary = Path::new(env!("CARET_TEST_CARGO_TARGET_DIR"))
        .join(profile)
        .join(caret_build::exe_name("caret-dslc"));
    if !binary.is_file() {
        return Err(format!("caret-dslc 不存在：{}", binary.display()).into());
    }
    for file in files {
        let mut command = Command::new(&binary);
        command.arg("format");
        if check {
            command.arg("--check");
        }
        command.arg(file);
        run(&format!("caret-dslc format {}", file.display()), command)?;
    }
    Ok(())
}

fn format_cpp_files(check: bool, files: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let llvm = caret_build::resolve_llvm_compilers()?;
    if !llvm.clang_format.is_file() {
        return Err(format!("clang-format 不存在：{}", llvm.clang_format.display()).into());
    }
    let mut command = Command::new(&llvm.clang_format);
    if check {
        command.args(["--dry-run", "-Werror"]);
    } else {
        command.arg("-i");
    }
    command.args(files);
    run(
        &format!("clang-format（{} 个变更文件）", files.len()),
        command,
    )
}

fn format_qml_files(check: bool, files: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let qmlformat = qmlformat_path()?;
    if !qmlformat.is_file() {
        provision_qml_format(root)?;
    }
    for file in files {
        let mut command = Command::new(&qmlformat);
        if check {
            let output = command
                .arg(file)
                .output()
                .map_err(|error| format!("qmlformat 启动失败（{}）：{error}", file.display()))?;
            if !output.status.success() {
                return Err(format!(
                    "qmlformat 失败：{}\n{}",
                    file.display(),
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }
            let formatted = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
            let current = fs::read_to_string(file)?.replace("\r\n", "\n");
            if formatted != current {
                return Err(format!("QML 文件需要格式化：{}", file.display()).into());
            }
        } else {
            command.arg("-i").arg(file);
            run(&format!("qmlformat {}", file.display()), command)?;
        }
    }
    Ok(())
}

/// 所有仓库 `.pa` 源文件共享同一个 parser 和 formatter 规范。
fn run_pa_format(check: bool) -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let mut files = Vec::new();
    for directory in pa_source_roots(root) {
        files.extend(pa_sources(&directory)?);
    }
    if files.is_empty() {
        return Err(".pa 源文件清单为空".into());
    }
    files.sort();

    run_pa_formatter(check, &files)
}

fn pa_source_roots(root: &Path) -> [PathBuf; 3] {
    [
        root.join("crates"),
        root.join("resources"),
        root.join("qml/Themes"),
    ]
}

fn is_pa_source_path(root: &Path, path: &Path) -> bool {
    pa_source_roots(root)
        .iter()
        .any(|directory| path.starts_with(directory))
}

fn pa_sources(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() && entry.file_name() != "fixtures" {
            files.extend(pa_sources(&path)?);
        } else if path.extension().is_some_and(|extension| extension == "pa") {
            files.push(path);
        }
    }
    Ok(files)
}

fn run_qml_format(check: bool) -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let mut files = Vec::new();
    for directory in ["qml", "tests/qml", "tests/fixtures/qml"] {
        extend_if_dir(&mut files, &root.join(directory), qml_sources)?;
    }
    files.sort();
    if files.is_empty() {
        return Err("QML 源文件清单为空".into());
    }
    format_qml_files(check, &files)
}

fn qml_sources(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            files.extend(qml_sources(&path)?);
        } else if path.extension().is_some_and(|extension| extension == "qml") {
            files.push(path);
        }
    }
    Ok(files)
}

fn qmlformat_path() -> Result<PathBuf, Box<dyn Error>> {
    let name = if cfg!(windows) {
        "qmlformat.exe"
    } else {
        "qmlformat"
    };
    Ok(caret_build::toolchain_root()?
        .join("qt/staging/bin")
        .join(name))
}

fn provision_qml_format(root: &Path) -> Result<(), Box<dyn Error>> {
    let cmake = caret_build::resolve_cmake()?;
    let qt = caret_build::toolchain_root()?.join("qt");
    let mut command = Command::new(cmake);
    command
        .arg(format!("-DQT_PROVISION_DIR={}", qt.display()))
        .arg("-P")
        .arg(root.join("native/cmake/qt-provision.cmake"));
    run("准备 qmlformat", command)
}

fn check_cpp_format(check: bool) -> Result<(), Box<dyn Error>> {
    let llvm = caret_build::resolve_llvm_compilers()?;
    let clang_format = &llvm.clang_format;
    if !clang_format.is_file() {
        return Err(format!("clang-format 不存在：{}", clang_format.display()).into());
    }
    let root = repository_root()?;
    let mut files = Vec::new();
    for directory in [
        "native",
        "tests/cpp",
        "tests/qml",
        "tests/performance",
        "tests/support",
        "crates/caret-ffi/src",
        "crates/caret-ffi/include",
    ] {
        extend_if_dir(&mut files, &root.join(directory), cpp_sources)?;
    }
    if files.is_empty() {
        return Err("C++ 源文件清单为空".into());
    }
    files.sort();
    let mut command = Command::new(clang_format);
    if check {
        command.args(["--dry-run", "-Werror"]);
    } else {
        command.arg("-i");
    }
    command.args(files.iter());
    run(&format!("clang-format（{} 个文件）", files.len()), command)
}

fn run_cmake_format(check: bool) -> Result<(), Box<dyn Error>> {
    if check {
        run_cmake_tool("cmake-format", &["--check"])
    } else {
        // cmake-format 缺省把格式化结果打印到 stdout，必须显式 --in-place。
        run_cmake_tool("cmake-format", &["--in-place"])
    }
}

pub(super) fn run_cmake_tool(tool: &str, arguments: &[&str]) -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let mut command = caret_build::python::command(root, tool)?;
    command.args(arguments).args(cmake_files(root)?);
    run_scanner(tool, command)
}

/// 尚未建立的目录跳过。空清单仍由调用方拒绝。
fn extend_if_dir(
    files: &mut Vec<PathBuf>,
    dir: &Path,
    collect: fn(&Path) -> std::io::Result<Vec<PathBuf>>,
) -> std::io::Result<()> {
    if dir.is_dir() {
        files.extend(collect(dir)?);
    }
    Ok(())
}

fn cpp_sources(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            files.extend(cpp_sources(&path)?);
        } else if caret_build::database::is_source(&path) || caret_build::database::is_header(&path)
        {
            files.push(path);
        }
    }
    Ok(files)
}

fn cmake_sources(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            files.extend(cmake_sources(&path)?);
        } else if path
            .file_name()
            .is_some_and(|name| name == "CMakeLists.txt")
            || path
                .extension()
                .is_some_and(|extension| extension == "cmake")
        {
            files.push(path);
        }
    }
    Ok(files)
}

fn cmake_files(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    for directory in ["native", "qml", "tools"] {
        extend_if_dir(&mut files, &root.join(directory), cmake_sources)?;
    }
    if files.is_empty() {
        return Err("CMake 源文件清单为空".into());
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_sources_are_included_in_changed_and_full_pa_format_scopes()
    -> Result<(), Box<dyn Error>> {
        let root = repository_root()?;
        let theme_file = root.join("qml/Themes/gui-default.pa");
        assert!(is_pa_source_path(root, &theme_file));
        assert!(pa_sources(&root.join("qml/Themes"))?.contains(&theme_file));
        assert!(!is_pa_source_path(root, &root.join("qml/Other.qml")));
        Ok(())
    }
}
