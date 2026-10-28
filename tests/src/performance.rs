//! 手动性能基准分发；各 suite 仅由 nightly 运行，不设置耗时门禁。

use std::error::Error;
use std::process::Command;

#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    suite: Suite,
    samples: Option<u32>,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum Suite {
    #[default]
    RustCpu,
    QtCpu,
    QtGpu,
}

impl Options {
    fn parse(arguments: Vec<String>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();
        match arguments.next().as_deref() {
            None | Some("cpu") => options.suite = Suite::RustCpu,
            Some("rust") => match arguments.next().as_deref() {
                Some("cpu") => options.suite = Suite::RustCpu,
                Some(other) => return Err(format!("未知 Rust 性能 suite：{other}")),
                None => return Err("Rust 性能 suite 必须指定 cpu".into()),
            },
            Some("qt") => match arguments.next().as_deref() {
                Some("cpu") => options.suite = Suite::QtCpu,
                Some("gpu") => options.suite = Suite::QtGpu,
                Some(other) => return Err(format!("未知 Qt 性能 suite：{other}")),
                None => return Err("Qt 性能 suite 必须指定 cpu 或 gpu".into()),
            },
            Some(other) => return Err(format!("未知性能 suite：{other}")),
        }
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--samples" if options.samples.is_none() => {
                    let value = arguments.next().ok_or("--samples 缺少次数")?;
                    let samples = value
                        .parse::<u32>()
                        .map_err(|_| "--samples 必须为至少 10 的整数")?;
                    // Criterion 要求 sample-size 至少为 10，提前返回可定位的参数错误。
                    if samples < 10 {
                        return Err("--samples 必须至少为 10".into());
                    }
                    options.samples = Some(samples);
                }
                _ => return Err(format!("未知或重复参数：{argument}")),
            }
        }
        Ok(options)
    }
}

pub(super) fn run(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    if arguments == ["--help"] {
        println!(
            "cargo performance [rust cpu | qt cpu | qt gpu] [--samples N]\n运行独立 CPU/GPU 基准；默认 rust cpu。Rust Criterion 样本数 N 至少为 10。"
        );
        return Ok(());
    }
    let options = Options::parse(arguments)?;
    match options.suite {
        Suite::RustCpu => run_rust_cpu(options.samples),
        Suite::QtCpu => run_qt(options.samples, false),
        Suite::QtGpu => run_qt(options.samples, true),
    }
}

fn run_rust_cpu(samples: Option<u32>) -> Result<(), Box<dyn Error>> {
    let mut command = Command::new("cargo");
    command
        .args([
            "bench",
            "--locked",
            "--package",
            "caret-layout",
            "--bench",
            "layout_criterion",
            "--package",
            "caret-dsl-core",
            "--bench",
            "dsl_core_criterion",
            "--package",
            "caret-engine",
            "--bench",
            "novel_document_criterion",
            "--package",
            "caret-app",
            "--bench",
            "document_view_input_criterion",
            "--target-dir",
        ])
        .arg(crate::paths::target_root())
        .current_dir(crate::paths::repository_root()?);
    if let Some(samples) = samples {
        command.args(["--", "--sample-size", &samples.to_string()]);
    }
    crate::command::run("Rust CPU Criterion benchmarks", command)
}

fn run_qt(samples: Option<u32>, gpu: bool) -> Result<(), Box<dyn Error>> {
    let repository_root = crate::paths::repository_root()?;
    let mut launcher = Command::new("cargo");
    launcher
        .args(["build", "--locked", "--release", "-p", "caret-launcher"])
        .env("CARET_ENABLE_PERFORMANCE", "1")
        .current_dir(repository_root);
    crate::command::run("Release Qt performance prerequisites", launcher)?;
    if gpu && cfg!(target_os = "linux") {
        verify_qt_xcb_runtime_dependencies()?;
    }

    let native_dir = crate::paths::target_root().join("native").join("release");
    let target = if gpu {
        "caret_qt_gpu_benchmark"
    } else {
        "caret_qt_cpu_benchmark"
    };
    let mut build = Command::new(caret_build::resolve_cmake()?);
    build.args(["--build"]).arg(&native_dir).args([
        "--config",
        "Release",
        "--target",
        target,
        "--parallel",
    ]);
    build
        .envs(caret_build::native_test_env(env!("CARET_TEST_HOST"))?)
        .current_dir(&native_dir);
    crate::command::run("Qt headless performance benchmark build", build)?;

    let binary = native_dir
        .join("canvas")
        .join(caret_build::exe_name(target));
    if !binary.is_file() {
        return Err(format!("Qt 性能基准不存在：{}", binary.display()).into());
    }
    let mut benchmark = Command::new(binary);
    benchmark
        .arg(if gpu { "gpu" } else { "cpu" })
        .envs(caret_build::native_test_env(env!("CARET_TEST_HOST"))?)
        .current_dir(repository_root);
    if !gpu {
        benchmark.env("QT_QPA_PLATFORM", "offscreen");
    }
    if let Some(samples) = samples {
        benchmark.args(["--samples", &samples.to_string()]);
    }
    crate::command::run(
        if gpu {
            "Qt headless GPU benchmark"
        } else {
            "Qt CPU benchmark"
        },
        benchmark,
    )
}

/// 在 Linux GPU 基准启动前报告 Qt xcb 插件缺失的系统动态库。
fn verify_qt_xcb_runtime_dependencies() -> Result<(), Box<dyn Error>> {
    let plugin = caret_build::toolchain_root()?.join("qt/staging/plugins/platforms/libqxcb.so");
    if !plugin.is_file() {
        return Err(format!("Qt xcb 平台插件不存在：{}", plugin.display()).into());
    }
    let output = Command::new("ldd").arg(&plugin).output()?;
    if !output.status.success() {
        return Err(format!(
            "无法检查 Qt xcb 插件依赖：{}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let report = String::from_utf8_lossy(&output.stdout);
    if report.lines().any(|line| line.contains("not found")) {
        return Err(format!("Qt xcb 插件缺少系统动态库：\n{report}").into());
    }
    println!("Qt xcb runtime dependencies: all resolved");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Options, Suite};

    #[test]
    fn accepts_default_cpu_and_criterion_sample_count() -> Result<(), String> {
        assert_eq!(Options::parse(vec![])?, Options::default());
        assert_eq!(Options::parse(vec!["cpu".into()])?, Options::default());
        assert_eq!(
            Options::parse(["rust", "cpu"].map(str::to_owned).to_vec())?,
            Options::default()
        );
        assert_eq!(
            Options::parse(["qt", "gpu"].map(str::to_owned).to_vec())?.suite,
            Suite::QtGpu
        );
        assert_eq!(
            Options::parse(["cpu", "--samples", "31"].map(str::to_owned).to_vec())?,
            Options {
                samples: Some(31),
                ..Options::default()
            }
        );
        Ok(())
    }

    #[test]
    fn rejects_unavailable_suites_and_invalid_sample_counts() {
        for arguments in [
            vec!["all"],
            vec!["support"],
            vec!["gpu"],
            vec!["cpu", "cpu"],
            vec!["qt", "cpu", "gpu"],
            vec!["rust", "gpu"],
            vec!["qt"],
            vec!["--samples", "0"],
            vec!["--samples", "9"],
            vec!["--samples", "bad"],
            vec!["--samples"],
            vec!["--samples", "10", "--samples", "20"],
            vec!["--project", "example.caret"],
            vec!["--presentation"],
        ] {
            assert!(Options::parse(arguments.into_iter().map(str::to_owned).collect()).is_err());
        }
    }
}
