//! 质量子进程、Cargo 参数与固定版本扩展工具安装；保留原退出诊断。

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

use crate::paths::{repository_root, target_root};

const CARGO_DENY_VERSION: &str = "0.20.2";
const CARGO_MACHETE_VERSION: &str = "0.9.2";

pub(super) fn cargo(
    subcommand: &str,
    args: impl IntoIterator<Item = &'static str>,
) -> Result<(), Box<dyn Error>> {
    let (description, command) = cargo_command(subcommand, args)?;
    run(&description, command)
}

/// 扫描类工具的静默变体：通过时不转发输出，失败时原样附上全部诊断。
pub(super) fn cargo_scanner(
    subcommand: &str,
    args: impl IntoIterator<Item = &'static str>,
) -> Result<(), Box<dyn Error>> {
    let (description, command) = cargo_command(subcommand, args)?;
    run_scanner(&description, command)
}

pub(super) fn cargo_command(
    subcommand: &str,
    args: impl IntoIterator<Item = &'static str>,
) -> Result<(String, Command), Box<dyn Error>> {
    let target_dir = target_root();
    let args: Vec<_> = args.into_iter().collect();
    // deny/machete 走托管的独立二进制；其余是 cargo 子命令。
    let managed_scanner = matches!(subcommand, "deny" | "machete");
    let mut command = if managed_scanner {
        let (name, version) = if subcommand == "deny" {
            ("cargo-deny", CARGO_DENY_VERSION)
        } else {
            ("cargo-machete", CARGO_MACHETE_VERSION)
        };
        Command::new(ensure_cargo_tool(name, version)?)
    } else {
        let mut command = Command::new("cargo");
        // -q 去掉 Compiling/Finished 状态行；构建脚本与诊断仍透传。
        command.arg("-q").arg(subcommand);
        command
    };
    if env!("CARET_TEST_BUILD_TYPE") == "Release"
        && matches!(subcommand, "build" | "test" | "clippy")
        && !args.contains(&"--release")
    {
        command.arg("--release");
    }
    let mut inserted_target_dir = false;
    let mut passthrough = false;
    let needs_target = matches!(subcommand, "build" | "test" | "clippy");
    for arg in args {
        if arg == "--" && !passthrough {
            // Cargo 参数必须位于测试 / 子命令透传参数之前。
            if needs_target && !inserted_target_dir {
                command.arg("--target-dir").arg(target_dir);
                inserted_target_dir = true;
            }
            passthrough = true;
        }
        if arg == "--target-dir" && !passthrough {
            command.arg(arg).arg(target_dir);
            inserted_target_dir = true;
        } else {
            command.arg(arg);
        }
    }
    if needs_target && !inserted_target_dir {
        command.arg("--target-dir").arg(target_dir);
    }
    command.current_dir(repository_root()?);
    Ok((format!("cargo {subcommand}"), command))
}

/// 扫描类工具的静默运行：通过时零输出，失败时原样转发全部捕获输出。
pub(super) fn run_scanner(description: &str, mut command: Command) -> Result<(), Box<dyn Error>> {
    let output = command
        .output()
        .map_err(|error| format!("{description} 启动失败：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "{description} 失败（退出码 {:?}）\n{stdout}{stderr}",
            output.status.code()
        )
        .into())
    }
}

pub(super) fn ensure_cargo_tool(name: &str, version: &str) -> Result<PathBuf, Box<dyn Error>> {
    let toolchain = caret_build::toolchain_root()?;
    let cargo_target = target_root();
    let root = caret_build::install_directory(&toolchain, name, version, |staging| {
        let mut command = Command::new("cargo");
        command
            .args(["install", "--root"])
            .arg(staging)
            .args([name, "--locked", "--version", &format!("={version}")])
            .env(
                "CARGO_TARGET_DIR",
                cargo_target.join("cargo-install").join(name),
            );
        run(&format!("安装 {name} {version}"), command).map_err(|e| e.to_string())?;
        if !staging
            .join("bin")
            .join(caret_build::exe_name(name))
            .is_file()
        {
            return Err(format!("安装未生成 {name}"));
        }
        Ok(())
    })?;
    Ok(root.join("bin").join(caret_build::exe_name(name)))
}

pub(super) fn run(description: &str, mut command: Command) -> Result<(), Box<dyn Error>> {
    let status = command
        .status()
        .map_err(|error| format!("{description} 启动失败：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{description} 失败（退出码 {:?}）", status.code()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_arguments_keep_target_before_passthrough() -> Result<(), Box<dyn Error>> {
        let (_, command) =
            cargo_command("test", ["--locked", "--", "--nocapture", "--target-dir"])?;
        let args = command
            .get_args()
            .map(std::ffi::OsStr::to_os_string)
            .collect::<Vec<_>>();
        let boundary = args
            .iter()
            .position(|arg| arg == "--")
            .ok_or("missing passthrough")?;
        assert!(boundary >= 2);
        assert_eq!(args[boundary - 2], "--target-dir");
        assert_eq!(args[boundary - 1], target_root());
        assert_eq!(
            &args[boundary + 1..],
            &[
                std::ffi::OsString::from("--nocapture"),
                "--target-dir".into()
            ]
        );
        assert_eq!(command.get_current_dir(), Some(repository_root()?));
        let (_, explicit) = cargo_command(
            "clippy",
            [
                "--locked",
                "--release",
                "--target-dir",
                "--",
                "-D",
                "warnings",
            ],
        )?;
        let args = explicit.get_args().collect::<Vec<_>>();
        assert_eq!(args.iter().filter(|arg| **arg == "--release").count(), 1);
        assert_eq!(args.iter().filter(|arg| **arg == "--target-dir").count(), 1);
        Ok(())
    }

    #[test]
    fn process_fixture() {
        if std::env::var("CARET_TEST_RUNNER_FIXTURE").as_deref() == Ok("fail") {
            println!("fixture stdout");
            eprintln!("fixture stderr");
            std::process::exit(23);
        }
    }

    #[test]
    fn subprocess_errors_keep_exit_code_and_scanner_diagnostics() -> Result<(), Box<dyn Error>> {
        let executable = std::env::current_exe()?;
        let fixture = |mode| {
            let mut command = Command::new(&executable);
            command
                .args(["--exact", "command::tests::process_fixture", "--nocapture"])
                .env("CARET_TEST_RUNNER_FIXTURE", mode);
            command
        };
        run("success", fixture("success"))?;
        run_scanner("success scanner", fixture("success"))?;
        let failure = run("failure", fixture("fail"))
            .err()
            .ok_or("failure accepted")?
            .to_string();
        assert!(failure.contains("退出码 Some(23)"), "{failure}");
        let failure = run_scanner("failure scanner", fixture("fail"))
            .err()
            .ok_or("scanner failure accepted")?
            .to_string();
        assert!(
            failure.contains("退出码 Some(23)")
                && failure.contains("fixture stdout")
                && failure.contains("fixture stderr"),
            "{failure}"
        );
        for scanner in [false, true] {
            let command = Command::new(executable.join("missing"));
            let result = if scanner {
                run_scanner("missing", command)
            } else {
                run("missing", command)
            };
            let error = result.err().ok_or("missing process accepted")?.to_string();
            assert!(error.contains("missing 启动失败"), "{error}");
        }
        Ok(())
    }
}
