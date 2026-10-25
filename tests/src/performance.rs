//! 手动 DSL CPU 基准入口；不进入常规 CTest，也不设置耗时门禁。

use std::error::Error;
use std::process::Command;

#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    samples: Option<u32>,
}

impl Options {
    fn parse(arguments: Vec<String>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();
        let mut selected = false;
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "cpu" if !selected => selected = true,
                "all" | "support" | "gpu" => {
                    return Err("当前仅支持 DSL CPU 基准：cargo performance cpu".into());
                }
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
            "cargo performance [cpu] [--samples N]\n运行 caret-dsl-core 已登记的 Criterion 基准；N 至少为 10，省略时使用 Criterion 默认值。"
        );
        return Ok(());
    }
    let options = Options::parse(arguments)?;
    let mut command = Command::new("cargo");
    command
        .args([
            "bench",
            "--locked",
            "--package",
            "caret-dsl-core",
            "--bench",
            "dsl_core_criterion",
            "--target-dir",
        ])
        .arg(crate::paths::target_root())
        .current_dir(crate::paths::repository_root()?);
    if let Some(samples) = options.samples {
        command.args(["--", "--sample-size", &samples.to_string()]);
    }
    crate::command::run("DSL Criterion CPU benchmarks", command)
}

#[cfg(test)]
mod tests {
    use super::Options;

    #[test]
    fn accepts_default_cpu_and_criterion_sample_count() -> Result<(), String> {
        assert_eq!(Options::parse(vec![])?, Options::default());
        assert_eq!(Options::parse(vec!["cpu".into()])?, Options::default());
        assert_eq!(
            Options::parse(["cpu", "--samples", "31"].map(str::to_owned).to_vec())?,
            Options { samples: Some(31) }
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
