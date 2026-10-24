//! 归档下载、解包与 SHA256；限制安装子进程墙钟时间并回收超时进程。

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::assets::{Tool, ToolAsset};

pub(super) fn download(
    asset: &ToolAsset,
    destination: &Path,
    name: &str,
    version: &str,
) -> Result<(), String> {
    let mut command = Command::new("curl");
    command
        // 速度护栏针对 CI 实测的传输停滞：60 秒均值低于 1 KiB/s 即中止并
        // 随 --retry 重试。curl 的 --max-time 会在每次重试时重置，
        // 因而额外用父进程限制整个下载（包括重试）最多 30 分钟。
        .args([
            "-fSL",
            "--retry",
            "3",
            "--retry-delay",
            "5",
            "--connect-timeout",
            "30",
            "--speed-limit",
            "1024",
            "--speed-time",
            "60",
            "--max-time",
            "1800",
            "--create-dirs",
            "-o",
        ])
        .arg(destination)
        .arg(asset.url);
    run_tool_command(
        &mut command,
        &format!("下载 {name} {version}：{}", asset.url),
        Duration::from_secs(30 * 60),
    )
}

pub(super) fn extract(
    tool: Tool,
    archive: &Path,
    destination: &Path,
    cmake: Option<&Path>,
) -> Result<(), String> {
    if cfg!(windows) && matches!(tool, Tool::Llvm) {
        // Windows 的 LLVM tar.xz 归档由系统 tar 解包极慢（CI 实测超过
        // 20 分钟）；官方 NSIS 安装包包含同一套 clang-cl/lld/format，
        // 支持静默安装和自定义目录，避免依赖 runner 上额外的 7-Zip。
        // 安装器清单要求 requireAdministrator，非提权 shell 直接启动报
        // os error 740；RunAsInvoker 兼容层覆盖清单按当前用户运行。安装
        // 目标在 `.toolchain/` 内，唯一需要系统权限的卸载注册表项与快捷
        // 方式在静默模式下写入失败不阻断安装；已提权进程（CI runner）
        // 行为不变。
        let mut command = Command::new(archive);
        command
            .env("__COMPAT_LAYER", "RunAsInvoker")
            .arg("/S")
            .arg(format!("/D={}", destination.display()));
        return run_tool_command(
            &mut command,
            &format!("安装 LLVM {}：{}", tool.version(), archive.display()),
            Duration::from_secs(20 * 60),
        );
    }
    let mut command = match tool {
        // CMake 压缩包由平台自带 tar 解开（macOS/Windows 为 bsdtar，可直接
        // 读 zip；Linux 为 GNU tar，自动识别 gzip）。
        Tool::Cmake | Tool::Llvm | Tool::Uv | Tool::Hyperfine => {
            let mut command = Command::new("tar");
            command.arg("-xf").arg(archive).arg("-C").arg(destination);
            command
        }
        Tool::Ninja => {
            let Some(cmake) = cmake else {
                return Err("内部错误：解包 Ninja 需要 cmake 路径".to_owned());
            };
            let mut command = Command::new(cmake);
            command
                .args(["-E", "tar", "xf"])
                .arg(archive)
                .current_dir(destination);
            command
        }
    };
    run_tool_command(
        &mut command,
        &format!("解包 {}：{}", tool.name(), archive.display()),
        Duration::from_secs(20 * 60),
    )
}

/// 保留工具实时输出，并限制整个子进程的墙钟时间；超时后终止并回收子进程。
fn run_tool_command(command: &mut Command, stage: &str, timeout: Duration) -> Result<(), String> {
    eprintln!("[caret-tools] {stage}：开始（上限 {timeout:?}）");
    let started = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .spawn()
        .map_err(|error| format!("{stage}：无法启动 {:?}：{error}", command.get_program()))?;
    let mut next_progress = Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    eprintln!("[caret-tools] {stage}：完成（{:?}）", started.elapsed());
                    return Ok(());
                }
                return Err(format!("{stage}：失败（{status}）"));
            }
            Ok(None) => {}
            Err(error) => {
                if child.kill().is_ok() {
                    let _ = child.wait();
                }
                return Err(format!("{stage}：无法查询子进程状态：{error}"));
            }
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            child.kill().map_err(|error| {
                format!(
                    "{stage}：超时（上限 {timeout:?}），无法终止 PID {}：{error}",
                    child.id()
                )
            })?;
            let status = child
                .wait()
                .map_err(|error| format!("{stage}：超时后无法回收子进程：{error}"))?;
            return Err(format!(
                "{stage}：超时（上限 {timeout:?}），子进程已终止并回收（{status}）"
            ));
        }
        if elapsed >= next_progress {
            eprintln!(
                "[caret-tools] {stage}：仍在运行（{elapsed:?}，PID {}）",
                child.id()
            );
            next_progress = elapsed + Duration::from_secs(30);
        }
        std::thread::sleep(Duration::from_millis(100).min(timeout - elapsed));
    }
}

pub(super) fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        fs::File::open(path).map_err(|error| format!("打开 {} 失败：{error}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_process_fixture() {
        match std::env::var("CARET_TEST_TOOL_PROCESS").as_deref() {
            Ok("fail") => std::process::exit(23),
            Ok("stall") => std::thread::sleep(std::time::Duration::from_secs(60)),
            _ => {}
        }
    }

    #[test]
    fn tool_process_propagates_failure_and_terminates_stalls() -> Result<(), String> {
        use std::process::Command;
        use std::time::{Duration, Instant};
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let command = |mode| {
            let mut command = Command::new(&executable);
            command
                .args(["--exact", "archive::tests::tool_process_fixture"])
                .env("CARET_TEST_TOOL_PROCESS", mode);
            command
        };
        super::run_tool_command(&mut command("success"), "fixture", Duration::from_secs(10))?;
        let error = super::run_tool_command(
            &mut command("fail"),
            "fixture failure",
            Duration::from_secs(10),
        )
        .err()
        .ok_or("非零退出应失败")?;
        assert!(
            error.contains("fixture failure") && error.contains("23"),
            "{error}"
        );
        let started = Instant::now();
        let error = super::run_tool_command(
            &mut command("stall"),
            "fixture timeout",
            Duration::from_millis(300),
        )
        .err()
        .ok_or("停滞进程应超时")?;
        assert!(
            error.contains("fixture timeout") && error.contains("已终止并回收"),
            "{error}"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        let error = super::run_tool_command(
            &mut Command::new(executable.join("missing")),
            "fixture missing",
            Duration::from_secs(10),
        )
        .err()
        .ok_or("缺少可执行文件应失败")?;
        assert!(
            error.contains("fixture missing") && error.contains("无法启动"),
            "{error}"
        );
        Ok(())
    }

    #[test]
    fn hex_encodes_lowercase_fixed_width() {
        assert_eq!(hex(&[]), "");
        assert_eq!(hex(&[0x0f, 0xa0]), "0fa0");
        assert_eq!(hex(&[255; 32]).len(), 64);
    }

    #[test]
    fn sha256_file_matches_known_vector() {
        let dir = std::env::temp_dir().join(format!("caret-provision-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("mkdir failed: {error}"));
        let file = dir.join("sample.txt");
        fs::write(&file, b"caret").unwrap_or_else(|error| panic!("write failed: {error}"));
        // printf 'caret' | shasum -a 256
        assert_eq!(
            sha256_file(&file).unwrap_or_else(|error| panic!("hash failed: {error}")),
            "2d717ccf122fdd913024e7e38ef4bfa1634fded8c1d3891de121f2d2481ed5a4"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
