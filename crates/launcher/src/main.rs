//! 桌面入口。启动 CMake 产物并转发参数与退出码。
//!
//! 产物路径由 build.rs 注入（`CARET_NATIVE_BIN`），不依赖启动时的当前目录。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[cfg(test)]
#[path = "ffi_names.rs"]
mod ffi_names;

/// native 产物缺失或无法启动。取 BSD sysexits.h 的 EX_UNAVAILABLE。
const EXIT_PRODUCT_UNAVAILABLE: u8 = 69;
/// 子进程被信号终止且平台无法给出信号号时的回退退出码。
const EXIT_SIGNAL_FALLBACK: u8 = 130;

fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let product = native_product();
    match run(product.as_deref(), &arguments) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("caret-launcher: {message}");
            ExitCode::from(EXIT_PRODUCT_UNAVAILABLE)
        }
    }
}

/// 构建脚本注入的 native 产物绝对路径。
fn native_product() -> Option<PathBuf> {
    option_env!("CARET_NATIVE_BIN").map(PathBuf::from)
}

/// 启动 native 产物。stdout/stderr 继承，退出码原样转发。
fn run(product: Option<&Path>, arguments: &[OsString]) -> Result<ExitCode, String> {
    let Some(product) = product else {
        return Err("构建配置异常：未注入 CARET_NATIVE_BIN（应总是由 build.rs 提供）".to_owned());
    };
    if !product.exists() {
        return Err(format!(
            "native 产物不存在：{}；请先执行 cargo build（诊断见 ai-docs/standards/dependency-acquisition.md）",
            product.display()
        ));
    }

    let mut command = std::process::Command::new(product);
    command.args(arguments);
    // Windows 开发构建不部署 Qt DLL：ctest 有 native_test_env 兜底，桌面入口
    // 只能自己把托管 Qt 的 bin 前置到子进程 PATH；macOS/Linux 依靠构建 rpath。
    if let Some(qt_bin) = qt_bin_for(option_env!("CARET_QT_BIN"), cfg!(windows)) {
        let path = prepared_qt_path(&qt_bin, std::env::var_os("PATH"))?;
        command.env("PATH", path);
    }
    match command.status() {
        Ok(status) => Ok(exit_code_of(&status)),
        Err(error) => Err(format!("无法启动 {}：{error}", product.display())),
    }
}

/// Windows 上返回注入的托管 Qt bin 目录；其余平台返回 None，不碰 PATH。
fn qt_bin_for(injected: Option<&str>, windows: bool) -> Option<PathBuf> {
    if !windows {
        return None;
    }
    injected
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// 前置目录后的子进程 PATH；继承 PATH 缺失时只含前置目录本身。
fn runtime_path(
    front: PathBuf,
    inherited: Option<OsString>,
) -> Result<OsString, std::env::JoinPathsError> {
    let mut paths = vec![front];
    if let Some(inherited) = inherited {
        paths.extend(std::env::split_paths(&inherited));
    }
    std::env::join_paths(paths)
}

/// staging 被事后清理时构建脚本不会重跑供给；此时给出可操作错误，而不是
/// 让 Windows 弹出找不到 Qt DLL 的系统对话框。
fn prepared_qt_path(qt_bin: &Path, inherited: Option<OsString>) -> Result<OsString, String> {
    if !qt_bin.is_dir() {
        return Err(format!(
            "托管 Qt 运行库目录不存在：{}；重装 Qt 需重跑 launcher 构建脚本（如 cargo clean -p caret-launcher）",
            qt_bin.display()
        ));
    }
    runtime_path(qt_bin.to_path_buf(), inherited)
        .map_err(|error| format!("拼接 Qt 运行库 PATH：{error}"))
}

/// 平台退出码取低 8 位。信号终止映射为 128+信号号。
fn exit_code_of(status: &std::process::ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        return ExitCode::from((code & 0xFF) as u8);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return ExitCode::from((128 + signal).min(u8::MAX.into()) as u8);
        }
    }
    ExitCode::from(EXIT_SIGNAL_FALLBACK)
}

#[cfg(test)]
mod tests;
