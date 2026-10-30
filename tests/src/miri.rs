//! 固定 nightly 的纯 Rust UB 检查；与 native sanitizer 分开编排。

use std::error::Error;
use std::process::Command;

use crate::command::run;
use crate::paths::{repository_root, target_root};

/// Miri 只随 nightly 发布；固定日期，升级记录在所属基础设施任务中。
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

/// 只解释执行纯 Rust 的应用服务和 DSL 测试，不构建 Qt、CXX 或启动进程。
/// 路径和设置测试需要真实临时目录，因此关闭宿主文件操作隔离。
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
    // pest/stacker 使用 Miri 不支持的外部汇编栈指针；排除调用 DSL parser 的测试。
    // document_view 与 document_session 测试会创建系统字体目录；Miri 不支持 Fontconfig 的 dlopen。
    miri_test(&[
        "-p",
        "caret-app",
        "--lib",
        "--",
        "--skip",
        "theme::tests::",
        "--skip",
        "document_view::tests::",
        // 字体目录快照测试会通过 fontdb/libloading 调用系统 dlopen；该 FFI 不受 Miri 支持。
        "--skip",
        "document_view::snapshot::tests::",
        "--skip",
        "document_session::tests::",
        "--skip",
        "settings::tests::",
        "--skip",
        "settings::runtime::tests::",
        // 该用例通过 pest 解析翻译资源；pest/stacker 的外部栈指针汇编不受 Miri 支持。
        "--skip",
        "settings::language::tests::supported_locale_metadata_matches_the_built_in_translation_files",
    ])?;
    // 保留诊断、文档格式化、TS 输出和 parser 前置限额等纯 Rust DSL 测试。
    for test in [
        "diagnostic::tests::",
        "format::tests::",
        "ts::tests::",
        "artifact::tests::aggregate_input_bytes_are_bounded_before_parsing",
    ] {
        miri_test(&["-p", "caret-dsl-core", "--lib", "--", test])?;
    }
    Ok(())
}
