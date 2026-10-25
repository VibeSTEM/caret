//! 常规构建与 native 测试编排；CMake 仍拥有 native 构建图。

use std::error::Error;
use std::path::Path;
use std::process::Command;

use crate::command::{cargo, run, run_scanner};
use crate::paths::native_build_dir;

pub(super) fn test_all() -> Result<(), Box<dyn Error>> {
    // native integration 会构建并验证真实窗口，不排除 runner 自身的回归测试。
    cargo("test", ["--locked", "--workspace", "--target-dir"])
}

pub(super) fn build_launcher() -> Result<(), Box<dyn Error>> {
    cargo("build", ["--locked", "-p", "caret-launcher"])
}

/// 公共集成入口：先构建，再运行 qmllint 和真实 CTest，不能用空套件代替成功。
pub(super) fn test_native() -> Result<(), Box<dyn Error>> {
    build_launcher()?;
    // CI 有独立 qmllint job；工作区测试和本地默认仍检查 QML。
    if std::env::var("CARET_TEST_QML_LINT").as_deref() != Ok("false") {
        run_qmllint()?;
    }
    let labels = std::env::var("CARET_TEST_CTEST_LABELS").ok();
    run_ctest(labels.as_deref())
}

/// 在指定 CMake 树执行完整 CTest；环境由调用方准备（常规树、覆盖率或
/// sanitizer 各有差异），参数与 `cargo coverage native` 保持一致。
pub(super) fn run_ctest_in(
    native_dir: &Path,
    envs: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    description: &str,
    configuration: Option<&str>,
    regex: Option<&str>,
    exclude: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let cmake = caret_build::resolve_cmake()?;
    let ctest = cmake.with_file_name(caret_build::exe_name("ctest"));
    if !ctest.is_file() {
        return Err(format!("ctest 不存在：{}", ctest.display()).into());
    }
    let mut test = Command::new(ctest);
    test.envs(envs);
    test.current_dir(native_dir)
        .args(["--output-on-failure", "--no-tests=error"]);
    if let Some(configuration) = configuration {
        test.args(["-C", configuration]);
    }
    if let Some(regex) = regex {
        test.args(["-R", regex]);
    }
    if let Some(exclude) = exclude {
        test.args(["-E", exclude]);
    }
    run(description, test)
}

/// 手动基准目标默认不构建，但仍在 clang-tidy 编译数据库中且包含 moc 输出；
/// 登记清单由 native 构建图的 caret_benchmark_moc 聚合目标维护。
pub(super) fn build_benchmark_moc() -> Result<(), Box<dyn Error>> {
    // 尚无 native 基准时不要求一个无消费者的空目标；查询当前构建图，
    // 将来注册真实基准后仍会生成其 moc，避免静态分析漏掉生成头。
    let mut help = cmake_build_command()?;
    help.args(["--target", "help"]);
    let output = help.output()?;
    if !output.status.success() {
        return Err(format!(
            "查询 native 目标失败（{}）：{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let registered = String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        line.split_once(':')
            .is_some_and(|(name, _)| name.trim() == "caret_benchmark_moc")
    });
    if registered {
        let mut command = cmake_build_command()?;
        command.args(["--target", "caret_benchmark_moc", "--parallel"]);
        run("生成手动基准 moc", command)?;
    }
    Ok(())
}

pub(super) fn cmake_build_command() -> Result<Command, Box<dyn Error>> {
    let native_dir = native_build_dir();
    let mut command = Command::new(caret_build::resolve_cmake()?);
    command
        .envs(caret_build::native_test_env(env!("CARET_TEST_HOST"))?)
        .current_dir(native_dir)
        .args(["--build"])
        .arg(native_dir)
        .args(["--config", env!("CARET_TEST_BUILD_TYPE")]);
    Ok(command)
}

pub(super) fn run_qmllint() -> Result<(), Box<dyn Error>> {
    let mut command = cmake_build_command()?;
    command.args(["--target", "all_qmllint"]);
    // 通过时静默：Qt 生成的 no-op qmllint 目标会回显 "Nothing to do"，
    // 失败时原样转发全部输出（含 qmllint 诊断）。
    run_scanner("qmllint", command)
}

fn run_ctest(labels: Option<&str>) -> Result<(), Box<dyn Error>> {
    let native_dir = native_build_dir();
    let cmake = caret_build::resolve_cmake()?;
    let ctest = cmake.with_file_name(caret_build::exe_name("ctest"));
    if !ctest.is_file() {
        return Err(format!("ctest 不存在：{}", ctest.display()).into());
    }
    let mut command = Command::new(ctest);
    command
        .envs(caret_build::native_test_env(env!("CARET_TEST_HOST"))?)
        .current_dir(native_dir)
        .args([
            "--output-on-failure",
            "--no-tests=error",
            "-C",
            env!("CARET_TEST_BUILD_TYPE"),
        ]);
    if let Some(labels) = labels {
        command.args(["-L", labels]);
    }
    run("ctest", command)
}
