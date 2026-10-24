//! 桌面入口。启动 CMake 产物并转发参数与退出码。
//!
//! 产物路径由 build.rs 注入（`CARET_NATIVE_BIN`），不依赖启动时的当前目录。

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
    match command.status() {
        Ok(status) => Ok(exit_code_of(&status)),
        Err(error) => Err(format!("无法启动 {}：{error}", product.display())),
    }
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
mod tests {
    use super::{ExitCode, run};
    use std::path::Path;

    #[test]
    fn missing_product_reports_unavailable() {
        let missing = Path::new("/caret/definitely/missing/caret");
        let error = match run(Some(missing), &[]) {
            Ok(code) => panic!("缺失产物应返回错误，实际退出码 {code:?}"),
            Err(error) => error,
        };
        assert!(error.contains("native 产物不存在"), "{error}");
    }

    #[test]
    fn absent_injection_reports_configuration_error() {
        let error = match run(None, &[]) {
            Ok(code) => panic!("未注入产物路径应返回错误，实际退出码 {code:?}"),
            Err(error) => error,
        };
        assert!(error.contains("CARET_NATIVE_BIN"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn forwards_child_exit_code() {
        let code = match run(Some(Path::new("/usr/bin/false")), &[]) {
            Ok(code) => code,
            Err(error) => panic!("无法转发 /usr/bin/false：{error}"),
        };
        assert_eq!(code, ExitCode::from(1));
    }

    #[cfg(unix)]
    #[test]
    fn forwards_child_success() {
        let code = match run(Some(Path::new("/usr/bin/true")), &[]) {
            Ok(code) => code,
            Err(error) => panic!("无法转发 /usr/bin/true：{error}"),
        };
        assert_eq!(code, ExitCode::SUCCESS);
    }
}
