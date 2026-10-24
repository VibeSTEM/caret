//! 固定 nightly 的纯 Rust UB 检查；与 native sanitizer 分开编排。

use std::error::Error;
use std::process::Command;

use crate::command::run;
use crate::paths::{repository_root, target_root};

/// Miri 只随 nightly 发布：固定日期保证可复现，升级时同步回填 032 验证表。
const MIRI_NIGHTLY: &str = "nightly-2026-09-15";
/// 在 Miri 语义内运行一组 Cargo 测试目标。
fn miri_test(args: &[&str]) -> Result<(), Box<dyn Error>> {
    let mut test = Command::new("cargo");
    test.arg(format!("+{MIRI_NIGHTLY}"))
        .args(["miri", "test", "--locked"])
        .args(args)
        .env("MIRIFLAGS", "-Zmiri-disable-isolation")
        .env("CARGO_TARGET_DIR", target_root().join("miri"))
        .current_dir(repository_root()?);
    run("cargo miri test", test)
}

/// Miri 解释执行纯 Rust 测试：工具链固定为日期版 nightly，产物走独立
/// target/miri，不污染常规构建缓存。路径/日志类测试使用真实文件系统夹具，
/// 因此关闭隔离放行宿主文件操作。压缩 sidecar 依赖 Miri 不支持的 zstd FFI，
/// 相关用例不纳入 Miri；项目和激活集成测试仅挑选不访问 Mesh sidecar 的用例。
/// Cargo 构建夹具单独运行，其子进程用例在 Miri 下显式忽略。
pub(super) fn miri() -> Result<(), Box<dyn Error>> {
    let mut install = Command::new("rustup");
    install.args(["toolchain", "install", MIRI_NIGHTLY]).args([
        "--profile",
        "minimal",
        "--component",
        "miri",
        "--no-self-update",
    ]);
    run("安装 Miri 工具链", install)?;
    miri_test(&["-p", "caret-core", "--lib"])?;
    miri_test(&["-p", "caret-core", "--test", "fsm_build"])?;
    // 这些工程元数据用例不构造或读取 Mesh sidecar，可在 Miri 下完整执行。
    for test in [
        "accepts_only_caret_project_files",
        "asynchronous_preview_rejects_replaced_cancelled_and_old_project_requests",
        "create_command_save_open_and_model_command_round_trip",
        "converted_coordinate_overflow_leaves_project_unchanged",
        "error_display_uses_stable_codes_and_details",
        "handles_manifest_revision_limits_and_save_io_failures",
        "inspects_binary_stl",
        "opens_invalid_manifests_and_accepts_case_insensitive_extension",
        "rejects_invalid_names_and_existing_targets",
        "rejects_fill_settings_for_missing_import_without_replacing_current_project",
        "rejects_invalid_stl_import_requests_without_project_changes",
        "rejects_unsupported_manifest_schema",
        "reports_empty_or_relative_locations_and_missing_project_state",
        "validates_platform_names_and_command_failures",
    ] {
        miri_test(&[
            "-p",
            "caret-core",
            "--test",
            "project",
            test,
            "--",
            "--exact",
        ])?;
    }
    // 此用例只检查工程和记录缺失时的同步错误，不启动资产解码。
    miri_test(&[
        "-p",
        "caret-core",
        "--test",
        "activation",
        "unknown_record_and_missing_project_fail_synchronously",
        "--",
        "--exact",
    ])?;
    miri_test(&["-p", "caret-dsl-core", "-p", "caret-foundation"])
}
