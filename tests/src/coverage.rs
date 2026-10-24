//! Rust 覆盖率报告与阶段门禁、native 覆盖率报告；逐 crate 汇总不设分模块阈值。

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::build::run_ctest_in;
use crate::command::{ensure_cargo_tool, run as run_command};
use crate::paths::{repository_root, target_root};

const CARGO_LLVM_COV_VERSION: &str = "0.9.1";

const FUNCTIONS_MINIMUM: u64 = 85;
const LINES_MINIMUM: u64 = 85;
const IGNORED_SOURCE_REGEX: &str = r"[/\\](caret-launcher|caret-tests|caret-build)[/\\]";
// 保持既有业务统计边界；这些构建/调度代码的覆盖缺口登记于任务 032。
const EXCLUDED_PACKAGES: &[&str] = &["caret-launcher", "caret-tests", "caret-build"];

pub(super) fn run() -> Result<(), Box<dyn Error>> {
    let tool = ensure_cargo_tool("cargo-llvm-cov", CARGO_LLVM_COV_VERSION)?;
    let root = repository_root()?;
    let target = target_root();
    let directory = target.join("rust-coverage");
    // 失败时也不能把上次的报告上传成当前提交的结果。
    if directory.exists() {
        fs::remove_dir_all(&directory)?;
    }
    fs::create_dir_all(&directory)?;
    let commit = command_text("git", &["rev-parse", "HEAD"], root)?;
    let dirty = !command_text("git", &["status", "--porcelain"], root)?.is_empty();
    let rustc = command_text("rustc", &["--version"], root)?;
    let tool_version = command_text(&tool, &["llvm-cov", "--version"], root)?;
    let metadata = json!({
        "commit": commit, "working_tree_dirty": dirty,
        "rustc": rustc, "cargo_llvm_cov": tool_version,
        "excluded_packages": EXCLUDED_PACKAGES,
        "minimum_percent": {"functions": FUNCTIONS_MINIMUM, "lines": LINES_MINIMUM},
        "branch_coverage": "not measured",
        "scope": "cargo-llvm-cov default filters; inline tests remain included",
        "detailed_reports": ["uncovered-lines.txt", "html/index.html"]
    });
    fs::write(
        directory.join("metadata.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    let report_path = directory.join("coverage.json");
    let mut command = Command::new(&tool);
    command.arg("llvm-cov").args(["--locked", "--workspace"]);
    for package in EXCLUDED_PACKAGES {
        command.args(["--exclude", package]);
    }
    command
        .args(["--json", "--output-path"])
        .arg(&report_path)
        .env("CARGO_TARGET_DIR", target)
        .current_dir(root);
    run_command("cargo llvm-cov", command)?;
    let report = Report::parse(&fs::read(report_path)?, root)?;
    let summary = report.summary();
    fs::write(directory.join("summary.txt"), &summary)?;
    print!("{summary}");
    write_rust_details(&tool, root, target, &directory)?;
    report.check_gate()
}

/// Generate actionable line and source views from the profiles collected by the gate run.
fn write_rust_details(
    tool: &Path,
    root: &Path,
    target: &Path,
    directory: &Path,
) -> Result<(), Box<dyn Error>> {
    let mut missing = Command::new(tool);
    missing
        .args([
            "llvm-cov",
            "report",
            "--text",
            "--show-missing-lines",
            "--show-instantiations",
            "--ignore-filename-regex",
            IGNORED_SOURCE_REGEX,
            "--output-path",
        ])
        .arg(directory.join("uncovered-lines.txt"));
    configure_report_command(&mut missing, root, target);
    run_command("Rust uncovered-line report", missing)?;

    let mut html = Command::new(tool);
    html.args([
        "llvm-cov",
        "report",
        "--html",
        "--show-instantiations",
        "--ignore-filename-regex",
        IGNORED_SOURCE_REGEX,
        "--output-dir",
    ])
    .arg(directory);
    configure_report_command(&mut html, root, target);
    run_command("Rust HTML coverage report", html)?;
    Ok(())
}

fn configure_report_command(command: &mut Command, root: &Path, target: &Path) {
    command.env("CARGO_TARGET_DIR", target).current_dir(root);
}

fn command_text(
    program: impl AsRef<std::ffi::OsStr>,
    arguments: &[&str],
    root: &Path,
) -> Result<String, Box<dyn Error>> {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "coverage 元数据命令失败：{}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

#[derive(Clone, Copy, Default)]
struct Metric {
    covered: u64,
    count: u64,
}

impl Metric {
    fn parse(value: &Value) -> Result<Self, Box<dyn Error>> {
        let count = value["count"]
            .as_u64()
            .ok_or("coverage count 缺失或不是无符号整数")?;
        let covered = value["covered"]
            .as_u64()
            .ok_or("coverage covered 缺失或不是无符号整数")?;
        if covered > count {
            return Err("coverage covered 超过 count".into());
        }
        Ok(Self { covered, count })
    }
    fn add(&mut self, other: Self) -> Result<(), Box<dyn Error>> {
        self.covered = self
            .covered
            .checked_add(other.covered)
            .ok_or("coverage covered 溢出")?;
        self.count = self
            .count
            .checked_add(other.count)
            .ok_or("coverage count 溢出")?;
        Ok(())
    }
    fn percent(self) -> String {
        if self.count == 0 {
            "n/a".to_owned()
        } else {
            format!("{:.2}%", self.covered as f64 * 100.0 / self.count as f64)
        }
    }
    fn reaches(self, minimum: u64) -> bool {
        self.count > 0
            && u128::from(self.covered) * 100 >= u128::from(self.count) * u128::from(minimum)
    }
}

#[derive(Default)]
struct Totals {
    functions: Metric,
    lines: Metric,
}

impl Totals {
    fn parse(value: &Value) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            functions: Metric::parse(&value["functions"])?,
            lines: Metric::parse(&value["lines"])?,
        })
    }
    fn add(&mut self, other: Self) -> Result<(), Box<dyn Error>> {
        self.functions.add(other.functions)?;
        self.lines.add(other.lines)
    }
}

struct Report {
    global: Totals,
    crates: BTreeMap<String, Totals>,
}

impl Report {
    fn parse(bytes: &[u8], root: &Path) -> Result<Self, Box<dyn Error>> {
        let value: Value = serde_json::from_slice(bytes)?;
        if value["type"] != "llvm.coverage.json.export"
            || !value["version"]
                .as_str()
                .is_some_and(|v| v.starts_with("3."))
        {
            return Err(format!(
                "不支持的 LLVM coverage JSON 格式：type={}，version={}（要求 llvm.coverage.json.export 3.x）",
                value["type"], value["version"]
            ).into());
        }
        let data = value["data"].as_array().ok_or("coverage data 缺失")?;
        if data.len() != 1 {
            return Err("coverage 必须包含一个合并结果".into());
        }
        let files = data[0]["files"].as_array().ok_or("coverage files 缺失")?;
        if files.is_empty() {
            return Err("coverage 文件集为空".into());
        }
        let mut crates: BTreeMap<String, Totals> = BTreeMap::new();
        for file in files {
            let filename = file["filename"].as_str().ok_or("coverage filename 缺失")?;
            let relative = Path::new(filename).strip_prefix(root).ok();
            let name = relative
                .and_then(|p| p.strip_prefix("crates").ok())
                .and_then(|p| p.components().next())
                .map_or_else(
                    || "other sources".to_owned(),
                    |c| c.as_os_str().to_string_lossy().into_owned(),
                );
            crates
                .entry(name)
                .or_default()
                .add(Totals::parse(&file["summary"])?)?;
        }
        Ok(Self {
            global: Totals::parse(&data[0]["totals"])?,
            crates,
        })
    }
    fn summary(&self) -> String {
        let mut text = format!(
            "Rust coverage: global gate functions >= {FUNCTIONS_MINIMUM}%, lines >= {LINES_MINIMUM}%\nCrate totals are informational; branch coverage is not measured.\n"
        );
        for (name, totals) in std::iter::once(("TOTAL", &self.global)).chain(
            self.crates
                .iter()
                .map(|(name, totals)| (name.as_str(), totals)),
        ) {
            text.push_str(&format!(
                "{name}: functions {}/{} ({}), lines {}/{} ({})\n",
                totals.functions.covered,
                totals.functions.count,
                totals.functions.percent(),
                totals.lines.covered,
                totals.lines.count,
                totals.lines.percent()
            ));
        }
        text
    }
    fn check_gate(&self) -> Result<(), Box<dyn Error>> {
        if !self.global.functions.reaches(FUNCTIONS_MINIMUM)
            || !self.global.lines.reaches(LINES_MINIMUM)
        {
            return Err(format!(
                "Rust coverage 低于阶段下限或统计为空：functions {}，lines {}",
                self.global.functions.percent(),
                self.global.lines.percent()
            )
            .into());
        }
        Ok(())
    }
}

/// 覆盖率构建使用独立 CMake 树，与普通构建共享已校验的依赖缓存。
pub(super) fn native_coverage() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let target = target_root();
    let report_dir = target.join("native-coverage");
    if report_dir.exists() {
        fs::remove_dir_all(&report_dir)?;
    }
    let profiles = report_dir.join("raw");
    fs::create_dir_all(&profiles)?;
    let profile_file = profiles.join("%m-%p.profraw");
    let mut build = Command::new("cargo");
    build
        .current_dir(root)
        .args(["build", "--locked", "-p", "caret-launcher", "--target-dir"])
        .arg(target)
        .env("CARET_NATIVE_COVERAGE", "1");
    run_command("native coverage build", build)?;
    let native = target.join("native/debug-coverage");
    let mut envs = caret_build::native_test_env(env!("CARET_TEST_HOST"))?;
    envs.push((
        std::ffi::OsString::from("LLVM_PROFILE_FILE"),
        profile_file.into_os_string(),
    ));
    run_ctest_in(&native, envs, "native coverage tests", None, None, None)?;
    let llvm = caret_build::resolve_llvm_compilers()?;
    let raw = fs::read_dir(&profiles)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "profraw"))
        .collect::<Vec<_>>();
    if raw.is_empty() {
        return Err("native 测试未生成覆盖率数据".into());
    }
    let merged = report_dir.join("native.profdata");
    let mut merge = Command::new(
        llvm.root
            .join("bin")
            .join(caret_build::exe_name("llvm-profdata")),
    );
    merge
        .args(["merge", "-sparse"])
        .args(raw)
        .arg("-o")
        .arg(&merged);
    run_command("native coverage merge", merge)?;
    let mut binaries = Vec::new();
    collect_test_binaries(&native, &mut binaries)?;
    binaries.sort();
    let first = binaries.first().ok_or("没有找到 native 覆盖率测试产物")?;
    let llvm_cov = llvm
        .root
        .join("bin")
        .join(caret_build::exe_name("llvm-cov"));
    let mut report = native_llvm_cov_command(&llvm_cov, "report", first, &binaries, &merged);
    report.args([
        "--ignore-filename-regex=(/target/|googletest|/usr/)",
        "--show-region-summary=false",
    ]);
    let output = report.output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    fs::write(report_dir.join("summary.txt"), &output.stdout)?;
    print!("{}", String::from_utf8_lossy(&output.stdout));

    let mut product_report =
        native_llvm_cov_command(&llvm_cov, "report", first, &binaries, &merged);
    product_report.args([
        "--ignore-filename-regex=(/target/|googletest|/usr/|/tests/)",
        "--show-region-summary=false",
    ]);
    let output = product_report.output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    fs::write(report_dir.join("product-summary.txt"), &output.stdout)?;

    let mut lines = native_llvm_cov_command(&llvm_cov, "show", first, &binaries, &merged);
    lines.args([
        "-format=text",
        "-show-line-counts-or-regions",
        "-ignore-filename-regex=(/target/|googletest|/usr/)",
    ]);
    let output = lines.output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    fs::write(report_dir.join("line-coverage.txt"), &output.stdout)?;

    let mut product_lines = native_llvm_cov_command(&llvm_cov, "show", first, &binaries, &merged);
    product_lines.args([
        "-format=text",
        "-show-line-counts-or-regions",
        "-ignore-filename-regex=(/target/|googletest|/usr/|/tests/)",
    ]);
    let output = product_lines.output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    fs::write(report_dir.join("product-line-coverage.txt"), &output.stdout)?;

    let mut html = native_llvm_cov_command(&llvm_cov, "show", first, &binaries, &merged);
    html.args([
        "-format=html",
        "-show-instantiations",
        "-ignore-filename-regex=(/target/|googletest|/usr/)",
    ])
    .arg(format!("-output-dir={}", report_dir.join("html").display()));
    run_command("native HTML coverage report", html)?;
    Ok(())
}

fn native_llvm_cov_command(
    llvm_cov: &Path,
    subcommand: &str,
    first: &Path,
    binaries: &[PathBuf],
    profile: &Path,
) -> Command {
    let mut command = Command::new(llvm_cov);
    command
        .arg(subcommand)
        .arg(first)
        .arg(format!("-instr-profile={}", profile.display()));
    for binary in binaries.iter().skip(1) {
        command.arg("-object").arg(binary);
    }
    command
}

fn collect_test_binaries(directory: &Path, result: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_test_binaries(&path, result)?;
        } else if path
            .file_stem()
            .is_some_and(|name| name.to_string_lossy().ends_with("_test"))
            && (path.extension().is_none() || path.extension().is_some_and(|ext| ext == "exe"))
        {
            result.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(functions: u64, lines: u64) -> Value {
        let totals = json!({
            "functions": {"count": 100, "covered": functions},
            "lines": {"count": 100, "covered": lines}
        });
        json!({
            "type": "llvm.coverage.json.export",
            "version": "3.1.0",
            "data": [{
                "totals": totals,
                "files": [{
                    "filename": "/repo/crates/caret-core/src/lib.rs",
                    "summary": totals
                }]
            }]
        })
    }
    fn parse(value: &Value) -> Result<Report, Box<dyn Error>> {
        Report::parse(&serde_json::to_vec(value)?, Path::new("/repo"))
    }
    #[test]
    fn gate_accepts_boundary_and_rejects_either_deficit() -> Result<(), Box<dyn Error>> {
        parse(&fixture(85, 85))?.check_gate()?;
        assert!(parse(&fixture(84, 100))?.check_gate().is_err());
        assert!(parse(&fixture(100, 84))?.check_gate().is_err());
        let rounded = Metric {
            covered: 84_999,
            count: 100_000,
        };
        assert_eq!(rounded.percent(), "85.00%");
        assert!(!rounded.reaches(FUNCTIONS_MINIMUM));
        Ok(())
    }
    #[test]
    fn rejects_missing_empty_and_invalid_statistics() -> Result<(), Box<dyn Error>> {
        assert!(Report::parse(b"not json", Path::new("/repo")).is_err());
        for version in [json!(null), json!("2.0.1"), json!("4.0.0")] {
            let mut value = fixture(100, 100);
            value["version"] = version;
            assert!(parse(&value).is_err());
        }
        let mut value = fixture(100, 100);
        value["data"][0]["totals"]["lines"] = json!(null);
        assert!(parse(&value).is_err());
        for replacement in [json!([]), json!([{}]), json!([{}, {}])] {
            let mut value = fixture(100, 100);
            value["data"] = replacement;
            assert!(parse(&value).is_err());
        }
        let mut value = fixture(100, 100);
        value["data"][0]["files"] = json!([]);
        assert!(parse(&value).is_err());
        assert!(parse(&fixture(101, 100)).is_err());
        let mut value = fixture(0, 0);
        value["data"][0]["totals"]["functions"]["count"] = json!(0);
        value["data"][0]["totals"]["lines"]["count"] = json!(0);
        assert!(parse(&value)?.check_gate().is_err());
        Ok(())
    }
    #[test]
    fn crate_summary_combines_files_and_preserves_zero_denominator() -> Result<(), Box<dyn Error>> {
        let mut value = fixture(100, 100);
        let file = value["data"][0]["files"][0].clone();
        value["data"][0]["files"]
            .as_array_mut()
            .ok_or("files")?
            .push(file);
        let report = parse(&value)?;
        assert!(
            report
                .summary()
                .contains("caret-core: functions 200/200 (100.00%)")
        );
        assert_eq!(Metric::default().percent(), "n/a");
        let mut metric = Metric {
            covered: u64::MAX,
            count: u64::MAX,
        };
        assert!(
            metric
                .add(Metric {
                    covered: 1,
                    count: 1
                })
                .is_err()
        );
        Ok(())
    }
}
