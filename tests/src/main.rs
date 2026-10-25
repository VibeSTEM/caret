//! Cargo 质量命令分发；具体构建、检查和报告由各职责模块承接。

mod build;
mod command;
mod coverage;
mod format;
mod layering;
mod lint;
mod miri;
mod package;
mod paths;
mod performance;
mod sanitize;
mod sdk;
mod test_scope;
mod toolchain;

use std::error::Error;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let result = match arguments.next().as_deref() {
        Some("quality") => quality(),
        Some("verify-changed") => verify_changed(),
        Some("performance") => performance::run(arguments.collect()),
        Some("test") => build::test_all(),
        Some("native-test") => build::test_native(),
        Some("package") => package::desktop(arguments.collect()),
        Some("sdk-googletest") => sdk::run(arguments.collect()),
        Some("test-plan") => test_scope::print_plan(arguments.collect()),
        Some("test-scope") => test_scope::run_scope(arguments.collect()),
        Some("format") => {
            let mut rest: Vec<String> = arguments.collect();
            let check = take_check_flag(&mut rest);
            let changed = take_flag(&mut rest, "--changed");
            format::format_all(check, changed, &rest)
        }
        Some("audit") => audit(),
        Some("coverage") => match arguments.next().as_deref() {
            None | Some("rust") => coverage::run(),
            Some("native") => coverage::native_coverage(),
            Some(other) => Err(format!("未知 coverage 类型：{other}").into()),
        },
        Some("sanitize") => sanitize::sanitize(),
        // 对外叫 ub-check：别名 `miri` 会遮蔽 cargo-miri 外部子命令。
        Some("ub-check") => miri::miri(),
        Some("toolchain") => toolchain::run(arguments.collect()),
        Some("lint") => {
            let mut rest: Vec<String> = arguments.collect();
            let check = take_check_flag(&mut rest);
            let changed = take_flag(&mut rest, "--changed");
            let tool = rest.first().map(String::as_str);
            let tool_args = rest.get(1..).unwrap_or_default();
            lint::lint(tool, check, changed, tool_args)
        }
        Some(command) => Err(format!(
            "未知命令 '{command}'；可用：quality、verify-changed、test、test-plan、test-scope、package、sdk-googletest、audit、lint、format、coverage、sanitize、\
             ub-check、toolchain [hyperfine <参数>…]、performance"
        )
        .into()),
        None => Err(
            "缺少命令；可用：quality、verify-changed、test、test-plan、test-scope、package、sdk-googletest、audit、lint、format、coverage、sanitize、ub-check、\
                 toolchain [hyperfine <参数>…]、performance"
                .into(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("caret-tests: {error}");
            ExitCode::FAILURE
        }
    }
}

/// 从剩余参数摘除 `--check`（位置不限）。缺省行为是就地修复（写文件），
/// `--check` 只验证不改动；CI 与 pre-commit 一律带 `--check`。
fn take_check_flag(arguments: &mut Vec<String>) -> bool {
    let check = arguments.iter().any(|argument| argument == "--check");
    arguments.retain(|argument| argument != "--check");
    check
}

fn take_flag(arguments: &mut Vec<String>, flag: &str) -> bool {
    let present = arguments.iter().any(|argument| argument == flag);
    arguments.retain(|argument| argument != flag);
    present
}

fn quality() -> Result<(), Box<dyn Error>> {
    format::format_all(true, false, &[])?;
    lint::lint(None, true, false, &[])?;
    audit()?;
    build::test_all()
}

fn verify_changed() -> Result<(), Box<dyn Error>> {
    format::format_all(true, true, &[])?;
    lint::lint(None, true, true, &[])?;
    // L1 包自身与 L2 消费者闭包都要运行，避免没有消费者的直接变更漏测。
    test_scope::run_scope(vec!["all".to_owned()])
}

fn audit() -> Result<(), Box<dyn Error>> {
    command::cargo("deny", ["check"])
}

#[cfg(test)]
mod tests {
    use super::take_check_flag;

    #[test]
    fn check_flag_is_removed_without_reordering_arguments() {
        let mut arguments = vec![
            "--check".to_owned(),
            "clippy".to_owned(),
            "--check".to_owned(),
        ];
        assert!(take_check_flag(&mut arguments));
        assert_eq!(arguments, ["clippy"]);
        assert!(!take_check_flag(&mut arguments));
        assert_eq!(arguments, ["clippy"]);
    }
}
