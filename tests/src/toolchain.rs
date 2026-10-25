//! 实际编译器、SDK、缓存及编译数据库核验。

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::paths::native_build_dir;

pub(super) fn run(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    match arguments.split_first() {
        Some((command, args)) if command == "hyperfine" => run_hyperfine(args),
        Some((command, _)) => Err(format!("未知 toolchain 命令：{command}").into()),
        None => verify_toolchain(),
    }
}

fn run_hyperfine(arguments: &[String]) -> Result<(), Box<dyn Error>> {
    let binary = caret_build::resolve_hyperfine()?;
    let status = Command::new(&binary).args(arguments).status()?;
    if !status.success() {
        return Err(format!("hyperfine 退出失败：{status}").into());
    }
    Ok(())
}

pub(super) fn verify_toolchain() -> Result<(), Box<dyn Error>> {
    let native_dir = native_build_dir();
    let cmake = caret_build::resolve_cmake()?;
    let llvm = caret_build::resolve_llvm_compilers()?;
    let clang_format = &llvm.clang_format;
    let llvm_root = &llvm.root;
    let llvm_version = caret_build::LLVM_VERSION;
    let toolchain = caret_build::toolchain_root()?;
    let managed_root = toolchain.join("tools");
    let managed_cmake = managed_root.join("cmake");
    let managed_llvm = managed_root.join("llvm");
    let ninja = caret_build::resolve_ninja(&cmake)?;
    let qt_bin = toolchain.join("qt/staging/bin");
    let compile_database = native_dir.join("compile_commands.json");

    require_file("托管 CMake", &cmake)?;
    require_file("托管 Ninja", &ninja)?;
    require_file("托管 clang-format", clang_format)?;
    require_file(
        "托管 clang",
        &llvm_root.join("bin").join(caret_build::exe_name("clang")),
    )?;
    require_file(
        "托管 clang++",
        &llvm_root.join("bin").join(caret_build::exe_name("clang++")),
    )?;
    if cfg!(windows) {
        require_file(
            "托管 clang-cl",
            &llvm_root
                .join("bin")
                .join(caret_build::exe_name("clang-cl")),
        )?;
    }
    let version_binary = if cfg!(windows) {
        llvm_root
            .join("bin")
            .join(caret_build::exe_name("clang-cl"))
    } else {
        llvm_root.join("bin").join(caret_build::exe_name("clang++"))
    };
    require_tool_version(&version_binary, llvm_version)?;
    require_file(
        "Qt qmlformat",
        &qt_bin.join(caret_build::exe_name("qmlformat")),
    )?;
    require_file("Qt qmllint", &qt_bin.join(caret_build::exe_name("qmllint")))?;
    require_file("native compile_commands.json", &compile_database)?;
    if !cmake.starts_with(&managed_cmake) {
        return Err(format!(
            "CMake 未使用 .toolchain 托管资产：{}（期望位于 {}）",
            cmake.display(),
            managed_cmake.display()
        )
        .into());
    }
    if !llvm_root.starts_with(&managed_llvm) || !clang_format.starts_with(&managed_llvm) {
        return Err(format!(
            "LLVM 工具未使用 .toolchain 托管资产：root={} clang-format={}（期望位于 {}）",
            llvm_root.display(),
            clang_format.display(),
            managed_llvm.display()
        )
        .into());
    }
    for tool in ["clang-tidy", "llvm-cov", "llvm-profdata"] {
        let path = llvm.root.join("bin").join(caret_build::exe_name(tool));
        require_file(tool, &path)?;
        require_tool_version(&path, llvm_version)?;
    }
    let cache = fs::read_to_string(native_dir.join("CMakeCache.txt"))?;
    for (key, expected) in [
        ("CMAKE_CXX_COMPILER", version_binary.as_path()),
        (
            "CMAKE_C_COMPILER",
            if cfg!(windows) {
                version_binary.as_path()
            } else {
                llvm.clang.as_path()
            },
        ),
        ("CMAKE_COMMAND", cmake.as_path()),
        ("CMAKE_MAKE_PROGRAM", ninja.as_path()),
    ] {
        let actual = cache
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once('=')?;
                (name.split_once(':')?.0 == key).then_some(value)
            })
            .ok_or_else(|| format!("CMakeCache 缺少 {key}"))?;
        if Path::new(actual).canonicalize()? != expected.canonicalize()? {
            return Err(format!("{key} 实际使用 {actual}，期望 {}", expected.display()).into());
        }
    }
    let commands = caret_build::database::read(&native_dir.join("quality/compile_commands.json"))?;
    caret_build::database::verify(&commands, &version_binary)?;
    println!(
        "Cargo 工具链已就绪：LLVM={} CMake={} Ninja={} Qt={} compile_commands={}",
        llvm_root.display(),
        cmake.display(),
        ninja.display(),
        qt_bin.display(),
        compile_database.display()
    );
    Ok(())
}

fn require_tool_version(path: &Path, expected: &str) -> Result<(), Box<dyn Error>> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|error| format!("运行 {} 失败：{error}", path.display()))?;
    if !output.status.success() {
        return Err(format!("{} --version 失败：{}", path.display(), output.status).into());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.contains(expected) && !stderr.contains(expected) {
        return Err(format!(
            "{} 版本不匹配：期望 LLVM {}，实际 {}{}",
            path.display(),
            expected,
            stdout.trim(),
            stderr.trim()
        )
        .into());
    }
    Ok(())
}

fn require_file(label: &str, path: &Path) -> Result<(), Box<dyn Error>> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("{label} 不存在：{}", path.display()).into())
    }
}
