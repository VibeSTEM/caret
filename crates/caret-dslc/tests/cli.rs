/// CLI 端到端测试：以 `CARGO_BIN_EXE` 提供的真二进制驱动
/// main() 与退出码契约（成功 0、用法/校验失败 2），子进程继承覆盖率
/// 环境（LLVM_PROFILE_FILE），main() 的行覆盖随本测试并入统计。
use std::process::Command;

fn dslc() -> Command {
    Command::new(env!("CARGO_BIN_EXE_caret-dslc"))
}

#[test]
fn no_arguments_prints_usage_and_exits_two() -> Result<(), Box<dyn std::error::Error>> {
    let output = dslc().output()?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("usage: caret-dslc"), "{stderr}");
    Ok(())
}

#[test]
fn check_succeeds_and_fails_with_exit_code_two() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("caret-dslc-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let ok = dir.join("ok.pa");
    std::fs::write(
        &ok,
        "version: 1\nkind: language\nlanguage: en\nsourcelanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n",
    )?;

    let ok_str = ok.to_str().ok_or("非 UTF-8 路径")?;
    let output = dslc().args(["check", ok_str]).output()?;
    assert_eq!(output.status.code(), Some(0), "{output:?}");

    let bad = dir.join("bad.pa");
    std::fs::write(&bad, "version: 1\nkind: language\nlanguage: en\n")?;
    let bad_str = bad.to_str().ok_or("非 UTF-8 路径")?;
    let output = dslc().args(["check", bad_str]).output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("pa."));

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn formatting_preserves_quotes_and_spaces_for_check_and_emit_ts()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("caret-dslc-round-trip-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let input = dir.join("quoted.pa");
    let before_ts = dir.join("before.ts");
    let after_ts = dir.join("after.ts");
    std::fs::write(
        &input,
        "version: 1\nkind: language\nlanguage: en\n\n[App]\nopen:\n  src: Say \"Hello\"\n  tr: \"Hello \\\"friend\\\" \"\n",
    )?;

    for command in ["check", "emit-ts", "format", "check", "emit-ts"] {
        let mut invocation = dslc();
        invocation.arg(command).arg(&input);
        if command == "emit-ts" {
            invocation.arg(if before_ts.exists() {
                &after_ts
            } else {
                &before_ts
            });
        }
        let output = invocation.output()?;
        assert_eq!(output.status.code(), Some(0), "{command}: {output:?}");
    }
    let output = dslc().arg("format").arg("--check").arg(&input).output()?;
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let ts = std::fs::read_to_string(&before_ts)?;
    assert!(
        ts.contains("<source>Say &quot;Hello&quot;</source>"),
        "{ts}"
    );
    assert!(
        ts.contains("<translation>Hello &quot;friend&quot; </translation>"),
        "{ts}"
    );
    assert_eq!(std::fs::read_to_string(&after_ts)?, ts);

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

#[test]
fn fsm_dispatch_accepts_whitespace_before_the_kind_colon() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = std::env::temp_dir().join(format!("caret-dslc-spaced-kind-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let input = dir.join("spaced-kind.pa");
    std::fs::write(
        &input,
        "version: 1\nkind : fsm\nname: demo-fsm\ninitial: Idle\n\nstates:\n  Idle: active\n  Done: terminal\ntransitions:\n  go:\n    from: Idle\n    on: advance\n    to: Done\n",
    )?;
    for command in ["check", "format", "check"] {
        let output = dslc().arg(command).arg(&input).output()?;
        assert_eq!(output.status.code(), Some(0), "{command}: {output:?}");
    }
    assert!(std::fs::read_to_string(&input)?.contains("kind: fsm\n"));
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
