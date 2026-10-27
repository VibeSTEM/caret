//! PR 与本地共用的受影响测试规划和分层执行。
//! `qml_lint` 单独追踪静态 QML 源/bridge 影响，不随全量 Rust/native 测试自动扩大。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde_json::Value;

// 只登记当前 native 模块和真实 CTest 标签。未知模块回退全量。
const NATIVE_LABELS: &[(&str, &[&str])] = &[
    ("i18n", &["cpp", "i18n", "qml"]),
    ("app", &["qml"]),
    ("adapters", &["qml"]),
    ("viewmodels", &["qml"]),
    ("canvas", &["qml"]),
];

// Rust package 的直接消费端可能跨过 CXX/构建脚本进入 native 测试。
// 新增此类边时同步扩展此表和下面的回归用例。
const RUST_NATIVE_LABELS: &[(&str, &[&str])] = &[
    ("caret-dsl-core", &["cpp", "i18n", "qml"]),
    ("caret-app", &["qml"]),
    ("caret-ffi", &["qml"]),
    ("caret-launcher", &["cpp", "i18n", "qml"]),
];

#[derive(Debug, Default)]
struct Plan {
    code: bool,
    cargo_deps: bool,
    rust: bool,
    runner_tests: bool,
    cpp: bool,
    cpp_lint: bool,
    cpp_lint_all: bool,
    cpp_format_all: bool,
    cmake: bool,
    cmake_lint_all: bool,
    cmake_format_all: bool,
    layering: bool,
    native: bool,
    qml: bool,
    qml_lint: bool,
    rust_lint_packages: BTreeSet<String>,
    rust_format_packages: BTreeSet<String>,
    packages_l1: BTreeSet<String>,
    packages_l2: BTreeSet<String>,
    native_labels: BTreeSet<String>,
    full: bool,
}

impl Plan {
    fn packages(&self) -> BTreeSet<String> {
        self.packages_l1.union(&self.packages_l2).cloned().collect()
    }

    fn print_github_outputs(&self) -> Result<(), serde_json::Error> {
        let packages = json_array(&self.packages())?;
        let packages_l1 = json_array(&self.packages_l1)?;
        let packages_l2 = json_array(&self.packages_l2)?;
        let native_labels = json_array(&self.native_labels)?;
        println!("code={}", self.code);
        println!("cargo_deps={}", self.cargo_deps);
        println!("rust={}", self.rust);
        println!(
            "rust_lint_packages={}",
            json_array(&self.rust_lint_packages)?
        );
        println!("runner_tests={}", self.runner_tests);
        println!("cpp={}", self.cpp);
        println!("cpp_lint={}", self.cpp_lint);
        println!("cpp_lint_all={}", self.cpp_lint_all);
        println!("cpp_format_all={}", self.cpp_format_all);
        println!("cmake={}", self.cmake);
        println!("cmake_lint_all={}", self.cmake_lint_all);
        println!("cmake_format_all={}", self.cmake_format_all);
        println!("layering={}", self.layering);
        println!("native={}", self.native);
        println!("qml={}", self.qml);
        println!("qml_lint={}", self.qml_lint);
        println!(
            "rust_format_packages={}",
            json_array(&self.rust_format_packages)?
        );
        println!("test_packages={packages}");
        println!("test_packages_l1={packages_l1}");
        println!("test_packages_l2={packages_l2}");
        println!("native_labels={native_labels}");
        println!("full_tests={}", self.full);
        Ok(())
    }
}

#[derive(Debug)]
struct Package {
    manifest: PathBuf,
    targets: Vec<PathBuf>,
    dependencies: Vec<String>,
}

type Packages = BTreeMap<String, Package>;

/// 共享头、构建或生成输入改变时扫描全部消费者；纯源文件只扫描自身。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum NativeLintScope {
    All,
    Sources(BTreeSet<String>),
}

impl NativeLintScope {
    pub(super) fn is_empty(&self) -> bool {
        matches!(self, Self::Sources(sources) if sources.is_empty())
    }
}

pub(super) fn native_lint_scope() -> Result<NativeLintScope, Box<dyn Error>> {
    if force_full_scope() {
        return Ok(NativeLintScope::All);
    }
    let root = repository_root()?;
    let paths = changed_paths()?;
    if make_plan(&paths, &root)?.cpp_lint_all {
        return Ok(NativeLintScope::All);
    }
    Ok(native_scope_for_paths(&paths))
}

fn native_scope_for_paths(paths: &[String]) -> NativeLintScope {
    let shared_input = paths.iter().any(|path| {
        let source = Path::new(path);
        caret_build::database::is_header(source)
            || Path::new(path)
                .file_name()
                .is_some_and(|name| name == ".clang-tidy")
            || path.starts_with("crates/caret-build/")
            || path.starts_with("crates/launcher/")
            || (path.starts_with("crates/caret-ffi/")
                && (path.ends_with(".rs") || path.ends_with("/Cargo.toml")))
    });
    if shared_input {
        NativeLintScope::All
    } else {
        NativeLintScope::Sources(
            paths
                .iter()
                .filter(|path| caret_build::database::is_source(Path::new(path)))
                .cloned()
                .collect(),
        )
    }
}

pub(super) fn is_cmake_path(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .is_some_and(|name| name == "CMakeLists.txt")
        || path.ends_with(".cmake")
}

pub(super) fn print_plan(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let ci_input = arguments.as_slice() == ["--ci"];
    if !ci_input && !arguments.is_empty() {
        return Err("用法：cargo test-plan [--ci]；默认比较 origin/main 与当前工作树".into());
    }
    let paths = if ci_input {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        input.lines().map(str::to_owned).collect::<Vec<_>>()
    } else {
        local_changed_paths(&root, "origin/main")?
    };
    let plan = if paths.is_empty() && !ci_input {
        Plan::default()
    } else {
        make_plan(&paths, &root)?
    };
    plan.print_github_outputs()?;
    println!("changed_paths={}", serde_json::to_string(&paths)?);
    eprintln!(
        "测试选择：L1={}；L2={}；CTest labels={}；完整回退={}",
        display_set(&plan.packages_l1),
        display_set(&plan.packages_l2),
        display_set(&plan.native_labels),
        plan.full
    );
    Ok(())
}

/// Returns changed paths from CI's shared selector output or the local Git diff.
pub(super) fn changed_paths() -> Result<Vec<String>, Box<dyn Error>> {
    if let Some(paths) = std::env::var_os("CARET_CHANGED_PATHS_JSON") {
        let paths: Vec<String> = serde_json::from_str(&paths.to_string_lossy())?;
        if paths.iter().any(|path| path.is_empty()) {
            return Err("变更路径列表包含空路径".into());
        }
        return Ok(paths);
    }
    local_changed_paths(&repository_root()?, "origin/main")
}

pub(super) fn qml_lint_needed() -> Result<bool, Box<dyn Error>> {
    if force_full_scope() {
        return Ok(true);
    }
    let root = repository_root()?;
    Ok(make_plan(&changed_paths()?, &root)?.qml_lint)
}

pub(super) fn cmake_lint_all() -> Result<bool, Box<dyn Error>> {
    let root = repository_root()?;
    Ok(make_plan(&changed_paths()?, &root)?.cmake_lint_all)
}

pub(super) fn cpp_format_all() -> Result<bool, Box<dyn Error>> {
    let root = repository_root()?;
    Ok(make_plan(&changed_paths()?, &root)?.cpp_format_all)
}

pub(super) fn cmake_format_all() -> Result<bool, Box<dyn Error>> {
    let root = repository_root()?;
    Ok(make_plan(&changed_paths()?, &root)?.cmake_format_all)
}

pub(super) fn force_full_scope() -> bool {
    std::env::var("CARET_FORCE_FULL").is_ok_and(|value| value.eq_ignore_ascii_case("true"))
}

/// Returns the Cargo package closure affected by changed Rust code.
pub(super) fn changed_lint_packages() -> Result<BTreeSet<String>, Box<dyn Error>> {
    let root = repository_root()?;
    let paths = changed_paths()?;
    if force_full_scope() {
        return Ok(cargo_packages(&root)?.keys().cloned().collect());
    }
    Ok(make_plan(&paths, &root)?.rust_lint_packages)
}

/// Returns directly changed Cargo packages for source-only formatting.
pub(super) fn changed_format_packages() -> Result<BTreeSet<String>, Box<dyn Error>> {
    let root = repository_root()?;
    let paths = changed_paths()?;
    if force_full_scope() {
        return Ok(cargo_packages(&root)?.keys().cloned().collect());
    }
    if paths.is_empty() {
        return Ok(BTreeSet::new());
    }
    Ok(make_plan(&paths, &root)?.rust_format_packages)
}

#[cfg(test)]
fn is_rust_package_path(path: &str) -> bool {
    path.starts_with("crates/")
        || path.starts_with("tests/rust/")
        || path.starts_with("tests/src/")
        || path.starts_with("tests/integration/")
        || (path.starts_with("tests/performance/rust/cpu/") && path.ends_with(".rs"))
        || matches!(
            path,
            "Cargo.toml" | "Cargo.lock" | "rust-toolchain.toml" | "rustfmt.toml" | ".rustfmt.toml"
        )
        || path.starts_with(".cargo/")
        || path.ends_with("/Cargo.toml")
}

pub(super) fn run_scope(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    let (scope, base) = parse_scope_args(arguments)?;
    let root = repository_root()?;
    let paths = local_changed_paths(&root, &base)?;
    if paths.is_empty() {
        println!("本地测试范围：{scope}\n工作树相对 {base} 没有变更，无需运行受影响测试。");
        return Ok(());
    }
    let plan = make_plan(&paths, &root)?;
    println!("本地测试范围：{scope}\n变更路径：{}", paths.len());
    plan.print_github_outputs()?;

    let selected = selected_packages(&scope, &plan)?;
    let mut tested_runner = false;
    for package in selected {
        if package == "caret-tests" {
            run_cargo([
                "test",
                "--locked",
                "--package",
                "caret-tests",
                "--bin",
                "caret-tests",
            ])?;
            tested_runner = true;
        } else {
            run_cargo(["test", "--locked", "--package", package.as_str()])?;
        }
    }
    if !tested_runner && scope == "all" && plan.rust {
        run_cargo([
            "test",
            "--locked",
            "--package",
            "caret-tests",
            "--bin",
            "caret-tests",
        ])?;
    }

    let run_native = scope_runs_native(&scope, &plan);
    if run_native {
        run_cargo(["build", "--locked", "--package", "caret-launcher"])?;
        let mut command = cargo_command([
            "test",
            "--locked",
            "--package",
            "caret-tests",
            "--test",
            "native",
        ])?;
        if !plan.full && !plan.native_labels.is_empty() {
            command.env("CARET_TEST_CTEST_LABELS", label_regex(&plan.native_labels));
        }
        command.status_checked("受影响的 native/QML 测试")?;
    }
    Ok(())
}

fn parse_scope_args(arguments: Vec<String>) -> Result<(String, String), Box<dyn Error>> {
    let mut args = arguments.into_iter();
    let scope = args
        .next()
        .ok_or("用法：cargo test-scope <l1|l2|all> [--base <ref>]")?;
    if !matches!(scope.as_str(), "l1" | "l2" | "all") {
        return Err("范围须为 l1、l2 或 all".into());
    }
    let mut base = "origin/main".to_owned();
    while let Some(argument) = args.next() {
        if argument == "--base" {
            base = args.next().ok_or("--base 后缺少 Git ref")?;
        } else {
            return Err(format!("未知 test-scope 参数：{argument}").into());
        }
    }
    Ok((scope, base))
}

fn selected_packages(scope: &str, plan: &Plan) -> Result<BTreeSet<String>, Box<dyn Error>> {
    if plan.full {
        return Ok(plan.packages());
    }
    match scope {
        "l1" => Ok(plan.packages_l1.clone()),
        "l2" => Ok(plan.packages_l2.clone()),
        "all" => Ok(plan.packages()),
        _ => Err("范围须为 l1、l2 或 all".into()),
    }
}

fn scope_runs_native(scope: &str, plan: &Plan) -> bool {
    plan.full || (scope != "l1" && (plan.native || plan.qml))
}

fn local_changed_paths(root: &Path, base: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let merge_base = git_output(root, ["merge-base", base, "HEAD"])?;
    let mut paths = BTreeSet::new();
    for args in [
        vec![
            "diff",
            "--no-renames",
            "--name-only",
            merge_base.trim(),
            "HEAD",
        ],
        vec!["diff", "--no-renames", "--name-only", "HEAD"],
        vec!["diff", "--cached", "--no-renames", "--name-only"],
        vec!["ls-files", "--others", "--exclude-standard"],
    ] {
        let output = git_output_vec(root, args.into_iter().map(str::to_owned).collect())?;
        paths.extend(
            output
                .lines()
                .filter(|path| !path.is_empty())
                .map(str::to_owned),
        );
    }
    Ok(paths.into_iter().collect())
}

fn make_plan(paths: &[String], root: &Path) -> Result<Plan, Box<dyn Error>> {
    let mut plan = Plan::default();
    if !paths.is_empty() && paths.iter().all(|path| is_document_path(path)) {
        return Ok(plan);
    }

    let packages = cargo_packages(root)?;
    let all_packages = packages.keys().cloned().collect::<BTreeSet<_>>();
    let reverse = reverse_dependencies(&packages);
    let mut changed_packages = BTreeSet::new();
    let mut rust_source_packages = BTreeSet::new();
    let mut native_dependency_changed = false;
    let all_native_labels = all_native_labels();
    plan.full = paths.is_empty();
    if paths.is_empty() {
        plan.cargo_deps = true;
        plan.rust = true;
        plan.cpp = true;
        plan.cpp_lint = true;
        plan.cpp_lint_all = true;
        plan.cpp_format_all = true;
        plan.cmake = true;
        plan.cmake_lint_all = true;
        plan.cmake_format_all = true;
        plan.native = true;
        plan.qml = true;
        plan.qml_lint = true;
    }

    for path in paths {
        if is_document_path(path) {
            continue;
        }
        if matches!(
            Path::new(path).file_name().and_then(|name| name.to_str()),
            Some(".clang-format" | ".clang-tidy" | ".cmake-format.py")
        ) {
            // 工具配置只扩展对应工具的作用域，不广播到其它质量检查。
            plan.cpp |= matches!(
                Path::new(path).file_name().and_then(|name| name.to_str()),
                Some(".clang-format" | ".clang-tidy")
            );
            plan.cpp_format_all |= Path::new(path)
                .file_name()
                .is_some_and(|name| name == ".clang-format");
            plan.cpp_lint |= matches!(
                Path::new(path).file_name().and_then(|name| name.to_str()),
                Some(".clang-tidy")
            );
            plan.cpp_lint_all |= Path::new(path)
                .file_name()
                .is_some_and(|name| name == ".clang-tidy");
            plan.cmake |= path.ends_with(".cmake-format.py");
            plan.cmake_lint_all |= path.ends_with(".cmake-format.py");
            plan.cmake_format_all |= path.ends_with(".cmake-format.py");
            plan.layering |= path.ends_with(".cmake-format.py");
            continue;
        }
        if matches!(
            path.as_str(),
            ".github/workflows/ci.yml" | ".github/actions/changed-paths/action.yml"
        ) {
            // CI 路由变化由 runner 自测覆盖，不启动无关源码检查。
            plan.runner_tests = true;
            continue;
        }
        if path == "Cargo.toml"
            || path == "Cargo.lock"
            || path == "rust-toolchain.toml"
            || path.starts_with(".cargo/")
        {
            // 工作区依赖锁、目标配置和 toolchain 均可能改变 CXX/native 产物。
            plan.full = true;
            plan.rust = true;
            plan.cpp = true;
            plan.cmake = true;
            plan.layering = true;
            plan.cargo_deps = true;
            changed_packages.extend(all_packages.iter().cloned());
            rust_source_packages.extend(all_packages.iter().cloned());
            continue;
        }
        if path == "deny.toml" {
            plan.cargo_deps = true;
            continue;
        }
        if path == "tests/Cargo.toml" {
            plan.cargo_deps = true;
            plan.rust = true;
            plan.layering = true;
            if let Some(package) = package_for_path(path, &packages, root) {
                rust_source_packages.insert(package.clone());
                changed_packages.insert(package);
            } else {
                plan.full = true;
                plan.qml = true;
                plan.qml_lint = true;
                plan.cpp = true;
                plan.cpp_lint = true;
                plan.cmake = true;
                rust_source_packages.extend(all_packages.iter().cloned());
            }
            continue;
        }
        if path.ends_with("/Cargo.toml") {
            plan.cargo_deps = true;
            plan.rust = true;
            plan.layering = true;
            if let Some(package) = package_for_path(path, &packages, root) {
                plan.qml_lint |= package_path_affects_qml_bridge(&package, path);
                plan.cpp |= package == "caret-ffi"
                    || package == "caret-build"
                    || (package == "caret-launcher" && path.ends_with("/build.rs"));
                plan.cpp_lint |= package == "caret-ffi"
                    || package == "caret-build"
                    || (package == "caret-launcher" && path.ends_with("/build.rs"));
                plan.cmake |= package == "caret-build"
                    || (package == "caret-launcher" && path.ends_with("/build.rs"));
                native_dependency_changed |= package_path_requires_native_tests(&package, path);
                rust_source_packages.insert(package.clone());
                changed_packages.insert(package);
            } else {
                plan.full = true;
                plan.native = true;
                plan.qml = true;
                plan.qml_lint = true;
                plan.cpp = true;
                plan.cmake = true;
                rust_source_packages.extend(all_packages.iter().cloned());
                changed_packages.extend(all_packages.iter().cloned());
            }
            continue;
        }
        if path.starts_with("crates/") || path.starts_with("tests/rust/") {
            plan.rust = true;
            if let Some(package) = package_for_path(path, &packages, root) {
                plan.qml_lint |= package_path_affects_qml_bridge(&package, path);
                plan.cpp |= package == "caret-ffi"
                    || package == "caret-build"
                    || (package == "caret-launcher" && path.ends_with("/build.rs"));
                plan.cpp_lint |= package == "caret-ffi"
                    || package == "caret-build"
                    || (package == "caret-launcher" && path.ends_with("/build.rs"));
                plan.cmake |= package == "caret-build"
                    || (package == "caret-launcher" && path.ends_with("/build.rs"));
                native_dependency_changed |= package_path_requires_native_tests(&package, path);
                rust_source_packages.insert(package.clone());
                changed_packages.insert(package);
            } else {
                plan.full = true;
                plan.native = true;
                plan.qml = true;
                plan.qml_lint = true;
                plan.cpp = true;
                plan.cmake = true;
                rust_source_packages.extend(all_packages.iter().cloned());
                changed_packages.extend(all_packages.iter().cloned());
            }
            continue;
        }
        if path.starts_with("tests/performance/rust/cpu/") && path.ends_with(".rs") {
            plan.rust = true;
            if let Some(package) = package_for_path(path, &packages, root) {
                plan.qml_lint |= package_path_affects_qml_bridge(&package, path);
                plan.cpp |= package == "caret-ffi"
                    || package == "caret-build"
                    || (package == "caret-launcher" && path.ends_with("/build.rs"));
                plan.cpp_lint |= package == "caret-ffi"
                    || package == "caret-build"
                    || (package == "caret-launcher" && path.ends_with("/build.rs"));
                plan.cmake |= package == "caret-build"
                    || (package == "caret-launcher" && path.ends_with("/build.rs"));
                native_dependency_changed |= package_path_requires_native_tests(&package, path);
                rust_source_packages.insert(package.clone());
                changed_packages.insert(package);
            } else {
                plan.full = true;
                plan.native = true;
                plan.qml = true;
                plan.qml_lint = true;
                plan.cpp = true;
                plan.cpp_lint = true;
                plan.cmake = true;
                rust_source_packages.extend(all_packages.iter().cloned());
                changed_packages.extend(all_packages.iter().cloned());
            }
            continue;
        }
        if path.starts_with("tests/performance/qt/") {
            // Qt 基准目标在 nightly 手动构建；日常 PR 仅格式化源码，不运行基准或 native 测试。
            continue;
        }
        if path.starts_with("tests/src/") {
            plan.rust = true;
            plan.runner_tests = true;
            changed_packages.insert("caret-tests".into());
            rust_source_packages.insert("caret-tests".into());
            if path == "tests/src/layering.rs" {
                plan.layering = true;
            }
            if path == "tests/src/build.rs" {
                plan.native = true;
                plan.native_labels.extend(all_native_labels.iter().cloned());
            }
            continue;
        }
        if path.starts_with("tests/integration/") {
            plan.rust = true;
            plan.native = true;
            if let Some(package) = package_for_path(path, &packages, root) {
                rust_source_packages.insert(package.clone());
                changed_packages.insert(package);
            } else {
                plan.full = true;
                plan.qml = true;
            }
            plan.native_labels.extend(all_native_labels.iter().cloned());
            continue;
        }
        if path.starts_with("tests/cpp/") || path.starts_with("native/") {
            plan.native = true;
            plan.cpp = true;
            plan.cpp_lint |= caret_build::database::is_header(Path::new(path))
                || caret_build::database::is_source(Path::new(path));
            plan.cmake |= is_cmake_path(path);
            plan.layering |= is_cmake_path(path);
            let parts = Path::new(path)
                .components()
                .filter_map(|part| part.as_os_str().to_str())
                .collect::<Vec<_>>();
            if parts.len() >= 3 && parts[0] == "native" {
                if parts[1] == "performance" {
                    plan.full = true;
                    plan.native_labels.extend(all_native_labels.iter().cloned());
                } else if let Some((_, labels)) =
                    NATIVE_LABELS.iter().find(|(module, _)| *module == parts[1])
                {
                    plan.native_labels
                        .extend(labels.iter().map(|label| (*label).to_owned()));
                    plan.qml_lint |= native_header_affects_qml_typeinfo(path);
                } else {
                    plan.full = true;
                    plan.qml_lint = true;
                    plan.native_labels.extend(all_native_labels.iter().cloned());
                }
            } else if path.starts_with("native/") {
                plan.full = true;
                plan.cmake = true;
                plan.native_labels.extend(all_native_labels.iter().cloned());
            } else if path.starts_with("tests/cpp/i18n/") {
                plan.native_labels.insert("cpp".to_owned());
            } else if path.starts_with("tests/cpp/shell/") {
                plan.qml = true;
                plan.native_labels.insert("qml".to_owned());
            } else if path.starts_with("tests/cpp/") {
                plan.full = true;
                plan.cpp_lint = true;
                plan.qml_lint = true;
                plan.native_labels.extend(all_native_labels.iter().cloned());
            }
            continue;
        }
        if path.starts_with("tests/fixtures/") || path.starts_with("tests/support/") {
            plan.full = true;
            plan.rust = true;
            plan.native = true;
            plan.qml = true;
            plan.qml_lint |= path.ends_with(".qml");
            plan.cpp |= caret_build::database::is_header(Path::new(path))
                || caret_build::database::is_source(Path::new(path));
            plan.cpp_lint |= caret_build::database::is_header(Path::new(path))
                || caret_build::database::is_source(Path::new(path));
            plan.cmake |= is_cmake_path(path);
            changed_packages.extend(all_packages.iter().cloned());
            plan.native_labels.extend(all_native_labels.iter().cloned());
            continue;
        }
        if path.starts_with("qml/Themes/") && path.ends_with(".pa") {
            plan.rust = true;
            changed_packages.insert("caret-app".into());
            plan.qml = true;
            plan.native = true;
            plan.native_labels.insert("qml".to_owned());
            continue;
        }
        if path.starts_with("qml/")
            || path.starts_with("tests/qml/")
            || path.starts_with("prototypes/shell/")
        {
            plan.qml = true;
            plan.qml_lint = true;
            plan.cmake |= is_cmake_path(path);
            plan.layering |= is_cmake_path(path);
            plan.cpp |= is_cmake_path(path);
            plan.native = true;
            plan.native_labels.insert("qml".to_owned());
            continue;
        }
        if path.starts_with("resources/") {
            plan.full = true;
            plan.rust = true;
            plan.native = true;
            plan.qml = true;
            changed_packages.extend(all_packages.iter().cloned());
            plan.native_labels.extend(all_native_labels.iter().cloned());
            continue;
        }
        // 新增/未登记路径保守回退，避免选择器落后时 CI 假绿。
        plan.full = true;
        plan.qml_lint = true;
        plan.rust = true;
        plan.cpp = true;
        plan.cpp_lint = true;
        plan.cpp_lint_all = true;
        plan.cmake = true;
        plan.cmake_lint_all = true;
        plan.cpp_format_all = true;
        plan.cmake_format_all = true;
        plan.layering = true;
        plan.native = true;
        plan.qml = true;
        plan.cargo_deps = true;
        changed_packages.extend(all_packages.iter().cloned());
        rust_source_packages.extend(all_packages.iter().cloned());
        plan.native_labels.extend(all_native_labels.iter().cloned());
    }

    if plan.full {
        plan.packages_l2 = all_packages.clone();
        plan.native_labels = all_native_labels;
    } else {
        plan.packages_l1 = changed_packages.clone();
        let mut affected = dependency_closure(changed_packages.clone(), &reverse);
        if !native_dependency_changed && !changed_packages.contains("caret-launcher") {
            // 纯 Rust 改动无需构建 launcher 的 Qt/CMake 树；仍编译其他直接消费者。
            affected.remove("caret-launcher");
        }
        if !affected.is_empty() {
            plan.rust = true;
        }
        if native_dependency_changed {
            for (package, labels) in RUST_NATIVE_LABELS {
                if affected.contains(*package) {
                    plan.native = true;
                    plan.qml |= labels.contains(&"qml");
                    plan.native_labels
                        .extend(labels.iter().map(|label| (*label).to_owned()));
                }
            }
        }
        if changed_packages.iter().any(|name| name == "caret-build")
            && paths
                .iter()
                .any(|path| path.starts_with("crates/caret-build/"))
        {
            plan.full = true;
            plan.cpp = true;
            plan.cmake = true;
            plan.layering = true;
            plan.native = true;
            plan.qml = true;
            plan.qml_lint |= changed_packages.contains("caret-build");
            plan.packages_l1.clear();
            plan.packages_l2 = all_packages.clone();
            plan.native_labels = all_native_labels;
        } else {
            plan.packages_l2 = affected.difference(&plan.packages_l1).cloned().collect();
        }
    }
    if plan.full {
        // 完整回退必须同步唤起每个域对应的 CI job，不能只扩大测试列表。
        plan.cargo_deps = true;
        plan.rust = true;
        plan.layering = true;
        plan.native = true;
        plan.qml = true;
    }
    plan.cpp |= plan.native_labels.contains("cpp");
    plan.qml |= plan.native_labels.contains("qml");
    if paths.is_empty() {
        rust_source_packages.extend(all_packages);
    }
    plan.rust_lint_packages = rust_source_packages.clone();
    plan.rust_format_packages = rust_source_packages;
    plan.code = plan.rust || plan.cpp || plan.cmake || plan.native || plan.qml;
    Ok(plan)
}

fn package_path_affects_qml_bridge(package: &str, path: &str) -> bool {
    match package {
        "caret-ffi" | "caret-build" => true,
        "caret-launcher" => path.ends_with("/build.rs"),
        "caret-app" => path.starts_with("crates/caret-app/src/settings/theme"),
        _ => false,
    }
}

fn native_header_affects_qml_typeinfo(path: &str) -> bool {
    matches!(
        path,
        "native/adapters/document_path_adapter.hpp"
            | "native/adapters/platform_fonts.hpp"
            | "native/adapters/window_chrome_metrics.hpp"
            | "native/viewmodels/theme/theme_view_model.hpp"
            | "native/viewmodels/settings/preferences_view_model.hpp"
            | "native/viewmodels/settings/settings_dialog_controller.hpp"
            | "native/viewmodels/document/layout_view_model.hpp"
            | "native/canvas/document_canvas.hpp"
            | "native/canvas/document_units.hpp"
            | "native/canvas/canvas_policy.hpp"
    )
}

fn package_path_requires_native_tests(package: &str, path: &str) -> bool {
    match package {
        "caret-ffi" | "caret-dsl-core" | "caret-build" | "caret-launcher" => true,
        "caret-app" => path.starts_with("crates/caret-app/src/settings/"),
        _ => false,
    }
}

fn cargo_packages(root: &Path) -> Result<Packages, Box<dyn Error>> {
    let output = Command::new("cargo")
        .args(["metadata", "--locked", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata 失败：{}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let mut packages = Packages::new();
    for package in metadata["packages"]
        .as_array()
        .ok_or("Cargo metadata 缺少 packages")?
    {
        let name = package["name"]
            .as_str()
            .ok_or("Cargo package 缺少 name")?
            .to_owned();
        let manifest = PathBuf::from(
            package["manifest_path"]
                .as_str()
                .ok_or("Cargo package 缺少 manifest_path")?,
        );
        let targets = package["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|target| target["src_path"].as_str().map(PathBuf::from))
            .collect();
        let dependencies = package["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|dependency| dependency["name"].as_str().map(str::to_owned))
            .collect();
        packages.insert(
            name,
            Package {
                manifest,
                targets,
                dependencies,
            },
        );
    }
    Ok(packages)
}

fn reverse_dependencies(packages: &Packages) -> BTreeMap<String, BTreeSet<String>> {
    let mut reverse = BTreeMap::<String, BTreeSet<String>>::new();
    for (consumer, package) in packages {
        for dependency in &package.dependencies {
            if packages.contains_key(dependency) {
                reverse
                    .entry(dependency.clone())
                    .or_default()
                    .insert(consumer.clone());
            }
        }
    }
    reverse
}

fn dependency_closure(
    changed: BTreeSet<String>,
    reverse: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut result = changed.clone();
    let mut queue = VecDeque::from_iter(changed);
    while let Some(package) = queue.pop_front() {
        if let Some(consumers) = reverse.get(&package) {
            for consumer in consumers {
                if result.insert(consumer.clone()) {
                    queue.push_back(consumer.clone());
                }
            }
        }
    }
    result
}

fn package_for_path(path: &str, packages: &Packages, root: &Path) -> Option<String> {
    let resolved = normalize_path(&root.join(path));
    packages
        .iter()
        .filter_map(|(name, package)| {
            let manifest = normalize_path(&package.manifest);
            let package_root = normalize_path(package.manifest.parent()?);
            let manifest_match = resolved == manifest || resolved.starts_with(&package_root);
            let target_depth = package
                .targets
                .iter()
                .filter(|source| resolved == normalize_path(source))
                .map(|source| normalize_path(source).components().count())
                .max()
                .unwrap_or(0);
            let root_depth = if manifest_match {
                package_root.components().count()
            } else {
                0
            };
            let depth = root_depth.max(target_depth);
            (depth > 0).then_some((depth, name.clone()))
        })
        .max_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)))
        .map(|(_, name)| name)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !path.is_absolute() {
                    normalized.push(component.as_os_str());
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn is_document_path(path: &str) -> bool {
    if path.starts_with("tests/performance/") && path.ends_with(".md") {
        return true;
    }
    if ["crates/", "tests/", "native/", "qml/", "resources/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return false;
    }
    path.ends_with(".md") || matches!(path, "LICENSE" | ".gitignore")
}

fn all_native_labels() -> BTreeSet<String> {
    NATIVE_LABELS
        .iter()
        .flat_map(|(_, labels)| labels.iter().map(|label| (*label).to_owned()))
        .chain(["build".into(), "qml".into()])
        .collect()
}

fn repository_root() -> Result<PathBuf, Box<dyn Error>> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "tests package 必须位于仓库根目录".into())
}

fn git_output<const N: usize>(root: &Path, arguments: [&str; N]) -> Result<String, Box<dyn Error>> {
    git_output_vec(root, arguments.into_iter().map(str::to_owned).collect())
}

fn git_output_vec(root: &Path, arguments: Vec<String>) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(format!("git 命令失败：{}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn run_cargo<const N: usize>(arguments: [&str; N]) -> Result<(), Box<dyn Error>> {
    cargo_command(arguments)?.status_checked("Cargo 测试")
}

fn cargo_command<const N: usize>(arguments: [&str; N]) -> Result<Command, Box<dyn Error>> {
    let mut command = Command::new("cargo");
    command.args(arguments).current_dir(repository_root()?);
    Ok(command)
}

trait CheckedStatus {
    fn status_checked(&mut self, description: &str) -> Result<(), Box<dyn Error>>;
}

impl CheckedStatus for Command {
    fn status_checked(&mut self, description: &str) -> Result<(), Box<dyn Error>> {
        let status = self.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{description}失败（退出码 {:?}）", status.code()).into())
        }
    }
}

fn json_array(values: &BTreeSet<String>) -> Result<String, serde_json::Error> {
    serde_json::to_string(&values.iter().collect::<Vec<_>>())
}

fn display_set(values: &BTreeSet<String>) -> String {
    if values.is_empty() {
        "无".into()
    } else {
        values.iter().cloned().collect::<Vec<_>>().join(",")
    }
}

fn label_regex(labels: &BTreeSet<String>) -> String {
    format!(
        "^({})$",
        labels.iter().cloned().collect::<Vec<_>>().join("|")
    )
}

#[cfg(test)]
mod tests;
