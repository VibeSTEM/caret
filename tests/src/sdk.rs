//! GoogleTest 静态 SDK 独立生产与归档消费自检，不进入普通构建。

use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::command::run as run_step;
use crate::paths::repository_root;

pub(super) fn run(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    if arguments.len() != 3 {
        return Err("用法：sdk-googletest <源码目录> <构建目录> <平台 triple>".into());
    }
    let root = repository_root()?;
    let source = root.join(&arguments[0]).canonicalize()?;
    let work = root.join(&arguments[1]);
    let triple = &arguments[2];
    let expected = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "macos-arm64",
        ("linux", "x86_64") => "linux-x86_64",
        ("windows", "x86_64") => "windows-x86_64",
        _ => return Err("未支持的 SDK 宿主平台".into()),
    };
    if triple != expected {
        return Err(format!("SDK triple 必须匹配宿主：{expected}").into());
    }
    let pin = Command::new("git")
        .args([
            OsStr::new("-C"),
            source.as_os_str(),
            OsStr::new("rev-parse"),
            OsStr::new("HEAD"),
        ])
        .output()?;
    if !pin.status.success()
        || String::from_utf8_lossy(&pin.stdout).trim() != "063de7e9578f82b369302001269680b4b1553359"
    {
        return Err("GoogleTest 源码不是锁定 commit".into());
    }
    let cmake = caret_build::resolve_cmake()?;
    let llvm = caret_build::resolve_llvm_compilers()?;
    let ninja = caret_build::resolve_ninja(&cmake)?;
    let compiler = if cfg!(windows) {
        llvm.clang_cl.as_ref().ok_or("缺少 clang-cl")?
    } else {
        &llvm.clangxx
    };
    // 上游 GoogleTest 的 project 还会启用 C，必须同时固定两个驱动器。
    // Windows 不能让 CMake 为 C 自动找到 GNU 命令行的 clang.exe，
    // 再把 MSVC 风格的 /D、/Zi 等参数交给它。
    let c_compiler = if cfg!(windows) { compiler } else { &llvm.clang };
    let sdk_env = caret_build::windows_sdk_env(env!("CARET_TEST_HOST"))?;
    let install = work.join("install");
    for (name, source_dir) in [
        ("build", root.join("tools/sdk")),
        ("selfcheck", root.join("tools/sdk/selfcheck/googletest")),
    ] {
        let consumed_root = if name == "selfcheck" {
            archive_sdk(&cmake, root, &work, &install, triple)?
        } else {
            install.clone()
        };
        let build = work.join(name);
        if name == "selfcheck" && build.exists() {
            // CMake 的 GTest_DIR 缓存会绕过新的 PSDK_ROOT，必须用独立干净树验收归档。
            fs::remove_dir_all(&build)?;
        }
        let mut configure = Command::new(&cmake);
        configure
            .envs(sdk_env.clone())
            .args(["-S"])
            .arg(source_dir)
            .arg("-B")
            .arg(&build)
            .args(["-G", "Ninja", "-DCMAKE_BUILD_TYPE=Release"])
            .arg(format!("-DCMAKE_MAKE_PROGRAM={}", ninja.display()))
            .arg(format!("-DCMAKE_CXX_COMPILER={}", compiler.display()))
            .arg(format!("-DCMAKE_C_COMPILER={}", c_compiler.display()))
            .arg(format!("-DCMAKE_INSTALL_PREFIX={}", install.display()))
            .arg(format!("-DGOOGLETEST_SOURCE={}", source.display()))
            .arg(format!("-DSDK_TRIPLE={triple}"))
            .arg(format!("-DPSDK_ROOT={}", consumed_root.display()));
        if let Some(sysroot) = caret_build::macos_sdk()? {
            configure
                .arg(format!("-DCMAKE_OSX_SYSROOT={}", sysroot.display()))
                .arg(format!(
                    "-DCMAKE_OSX_DEPLOYMENT_TARGET={}",
                    caret_build::MACOS_DEPLOYMENT_TARGET
                ));
        }
        run_step("配置 GoogleTest SDK", configure)?;
        let mut compile = Command::new(&cmake);
        compile
            .envs(sdk_env.clone())
            .arg("--build")
            .arg(&build)
            .args(["--config", "Release", "--parallel"]);
        run_step("编译 GoogleTest SDK", compile)?;
        let mut finish = if name == "build" {
            let mut command = Command::new(&cmake);
            command
                .arg("--install")
                .arg(&build)
                .args(["--config", "Release"]);
            command
        } else {
            let mut command = Command::new(cmake.with_file_name(caret_build::exe_name("ctest")));
            command.arg("--test-dir").arg(&build).args([
                "-C",
                "Release",
                "--output-on-failure",
                "--no-tests=error",
            ]);
            command
        };
        finish.envs(sdk_env.clone());
        run_step("验收 GoogleTest SDK", finish)?;
    }
    Ok(())
}

/// 自检消费归档的搬迁副本，避免只验证仍引用生产目录的安装树。
fn archive_sdk(
    cmake: &Path,
    root: &Path,
    work: &Path,
    install: &Path,
    triple: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let mut archive = Command::new(cmake);
    archive
        .arg(format!("-DPKG_ROOT={}", install.display()))
        .arg("-DPKG_NAME=googletest")
        .arg("-DPKG_VERSION=1.18.0")
        .arg(format!("-DPKG_TRIPLE={triple}"))
        .arg(format!("-DPKG_DIST={}", work.join("dist").display()))
        .arg("-P")
        .arg(root.join("tools/sdk/package.cmake"));
    run_step("归档 GoogleTest SDK", archive)?;
    let unpacked = work.join("unpacked");
    if unpacked.exists() {
        fs::remove_dir_all(&unpacked)?;
    }
    fs::create_dir_all(&unpacked)?;
    let mut extract = Command::new(cmake);
    extract
        .args(["-E", "tar", "xzf"])
        .arg(
            work.join("dist")
                .join(format!("googletest-1.18.0-{triple}.tar.gz")),
        )
        .current_dir(&unpacked);
    run_step("解包 GoogleTest SDK 自检副本", extract)?;
    Ok(unpacked)
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::paths::repository_root;

    #[test]
    fn rejects_invalid_usage_without_building_or_downloading() {
        let error = run(Vec::new()).err().map(|error| error.to_string());
        assert!(error.is_some_and(|message| message.contains("用法")));
    }

    #[test]
    fn rejects_target_mismatch_before_producing_sdk() -> Result<(), Box<dyn std::error::Error>> {
        let source = repository_root()?.to_string_lossy().into_owned();
        let result = run(vec![
            source,
            "target/unused-sdk-output".into(),
            "other-platform".into(),
        ]);
        let message = result
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            message.contains("triple") || message.contains("宿主平台"),
            "{message}"
        );
        Ok(())
    }
}
