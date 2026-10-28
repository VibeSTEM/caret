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
const NATIVE_COVERAGE_MINIMUM: u64 = 85;
const IGNORED_SOURCE_REGEX: &str = r"[/\\](caret-launcher|caret-tests|caret-build)[/\\]";
const NATIVE_PRODUCT_IGNORED_SOURCE_REGEX: &str = r"(/target/|googletest|/usr/|/tests/)";
// 构建和测试调度不计入领域覆盖率；caret-tests::document 作为额外测试目标单独参与采集，
// 避免执行会重新构建并启动 CTest 的 native 聚合入口。报告范围从 workspace 动态推导。
const WORKSPACE_TEST_EXCLUSIONS: &[&str] = &["caret-tests", "caret-launcher", "caret-build"];

pub(super) fn run() -> Result<(), Box<dyn Error>> {
    let tool = ensure_cargo_tool("cargo-llvm-cov", CARGO_LLVM_COV_VERSION)?;
    let root = repository_root()?;
    let coverage_packages = coverage_packages(root)?;
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
        "workspace_test_exclusions": WORKSPACE_TEST_EXCLUSIONS,
        "coverage_packages": coverage_packages,
        "additional_test_targets": ["caret-tests::document"],
        "minimum_percent": {"functions": FUNCTIONS_MINIMUM, "lines": LINES_MINIMUM},
        "branch_coverage": "not measured",
        "scope": "cargo-llvm-cov default filters; inline tests remain included",
        "detailed_reports": ["uncovered-lines.txt", "html/index.html"]
    });
    fs::write(
        directory.join("metadata.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    let mut clean = Command::new(&tool);
    clean
        .arg("llvm-cov")
        .args(["clean", "--workspace"])
        .env("CARGO_TARGET_DIR", target.as_os_str())
        .current_dir(root);
    run_command("清理旧 Rust coverage profiles", clean)?;

    // 分开运行领域单测与公开文档集成测试，避免 coverage 执行会再次启动 CTest 的 native target。
    let mut command = Command::new(&tool);
    command
        .arg("llvm-cov")
        .args(["--locked", "--workspace", "--no-report"]);
    for package in WORKSPACE_TEST_EXCLUSIONS {
        command.args(["--exclude", package]);
    }
    command.env("CARGO_TARGET_DIR", target).current_dir(root);
    run_command("cargo llvm-cov workspace 单元测试", command)?;

    let mut document_tests = Command::new(&tool);
    document_tests
        .arg("llvm-cov")
        .args([
            "--locked",
            "--package",
            "caret-tests",
            "--test",
            "document",
            "--no-report",
        ])
        .env("CARGO_TARGET_DIR", target)
        .current_dir(root);
    run_command("cargo llvm-cov document 集成测试", document_tests)?;

    let report_path = directory.join("coverage.json");
    let mut report_command = Command::new(&tool);
    report_command.arg("llvm-cov").args(["report", "--locked"]);
    configure_report_scope(&mut report_command, &coverage_packages);
    report_command
        .args(["--json", "--output-path"])
        .arg(&report_path)
        .env("CARGO_TARGET_DIR", target.as_os_str())
        .current_dir(root);
    run_command("cargo llvm-cov 合并覆盖率", report_command)?;
    let report = Report::parse(&fs::read(report_path)?, root)?;
    let summary = report.summary();
    fs::write(directory.join("summary.txt"), &summary)?;
    print!("{summary}");
    write_rust_details(&tool, root, target, &directory, &coverage_packages)?;
    report.check_gate()
}

/// Generate actionable line and source views from the profiles collected by the gate run.
fn write_rust_details(
    tool: &Path,
    root: &Path,
    target: &Path,
    directory: &Path,
    coverage_packages: &[String],
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
    configure_report_scope(&mut missing, coverage_packages);
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
    configure_report_scope(&mut html, coverage_packages);
    configure_report_command(&mut html, root, target);
    run_command("Rust HTML coverage report", html)?;
    Ok(())
}

fn configure_report_scope(command: &mut Command, packages: &[String]) {
    for package in packages {
        command.arg("--package").arg(package);
    }
}

fn coverage_packages(root: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let metadata: Value = serde_json::from_str(&command_text(
        "cargo",
        &["metadata", "--locked", "--format-version", "1", "--no-deps"],
        root,
    )?)?;
    coverage_packages_from_metadata(&metadata)
}

fn coverage_packages_from_metadata(metadata: &Value) -> Result<Vec<String>, Box<dyn Error>> {
    let workspace_members = metadata["workspace_members"]
        .as_array()
        .ok_or("Cargo metadata workspace_members 缺失")?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("Cargo metadata packages 缺失")?;
    let members: std::collections::BTreeSet<_> = workspace_members
        .iter()
        .map(|member| member.as_str().ok_or("Cargo metadata member ID 无效"))
        .collect::<Result<_, _>>()?;
    let mut names = packages
        .iter()
        .filter(|package| {
            package["id"]
                .as_str()
                .is_some_and(|id| members.contains(id))
        })
        .filter_map(|package| package["name"].as_str())
        .filter(|name| !WORKSPACE_TEST_EXCLUSIONS.contains(name))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    names.sort();
    if names.is_empty() {
        return Err("Cargo workspace 中没有可统计覆盖率的 package".into());
    }
    Ok(names)
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
    let app = find_native_app(&native)?.ok_or("没有找到 native 覆盖率应用程序产物")?;
    binaries.push(app);
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
        &format!("--ignore-filename-regex={NATIVE_PRODUCT_IGNORED_SOURCE_REGEX}"),
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
        &format!("-ignore-filename-regex={NATIVE_PRODUCT_IGNORED_SOURCE_REGEX}"),
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

    let mut product_export =
        native_llvm_cov_command(&llvm_cov, "export", first, &binaries, &merged);
    product_export.args([
        "-format=text",
        "-summary-only",
        &format!("-ignore-filename-regex={NATIVE_PRODUCT_IGNORED_SOURCE_REGEX}"),
    ]);
    let output = product_export.output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let product_coverage = NativeCoverage::parse(&output.stdout)?;
    let gate = product_coverage.summary();
    fs::write(report_dir.join("gate.txt"), &gate)?;
    print!("{gate}");
    product_coverage.check_gate(NATIVE_COVERAGE_MINIMUM)?;
    Ok(())
}

struct NativeCoverage {
    totals: Totals,
}

impl NativeCoverage {
    fn parse(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let value: Value = serde_json::from_slice(bytes)?;
        if value["type"] != "llvm.coverage.json.export" {
            return Err("不支持的 native LLVM coverage JSON 格式".into());
        }
        let data = value["data"]
            .as_array()
            .ok_or("native coverage data 缺失")?;
        if data.len() != 1 {
            return Err("native coverage 必须包含一个合并结果".into());
        }
        let totals = data[0]
            .get("totals")
            .and_then(Value::as_object)
            .ok_or("native coverage totals 缺失")?;
        let coverage = Totals {
            functions: Metric::parse(
                totals
                    .get("functions")
                    .ok_or("native coverage functions 缺失")?,
            )?,
            lines: Metric::parse(totals.get("lines").ok_or("native coverage lines 缺失")?)?,
        };
        if coverage.functions.count == 0 || coverage.lines.count == 0 {
            return Err("native 产品源码没有可统计的函数或可执行行".into());
        }
        Ok(Self { totals: coverage })
    }

    fn summary(&self) -> String {
        format!(
            "Native product coverage gate: functions {}/{} ({}), lines {}/{} ({})\nExcluded: tests, generated/build output, third-party and system sources.\n",
            self.totals.functions.covered,
            self.totals.functions.count,
            self.totals.functions.percent(),
            self.totals.lines.covered,
            self.totals.lines.count,
            self.totals.lines.percent(),
        )
    }

    fn check_gate(&self, minimum: u64) -> Result<(), Box<dyn Error>> {
        if !self.totals.functions.reaches(minimum) || !self.totals.lines.reaches(minimum) {
            return Err(format!(
                "native 产品源码覆盖率低于 {minimum}% 门槛：函数 {}，行 {}",
                self.totals.functions.percent(),
                self.totals.lines.percent()
            )
            .into());
        }
        Ok(())
    }
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

fn find_native_app(directory: &Path) -> std::io::Result<Option<PathBuf>> {
    let expected_name = caret_build::exe_name("caret");
    let mut pending = vec![directory.join("app")];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                pending.push(path);
            } else if entry.file_type()?.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name == std::ffi::OsStr::new(&expected_name))
            {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
