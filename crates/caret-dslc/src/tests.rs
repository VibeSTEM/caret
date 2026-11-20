//! 验证 DSL 编译器命令入口和错误处理。

// 测试统一以 ? 传播错误：避免永不执行的错误闭包（会被计为未覆盖行），
// 同时不使用 unwrap/expect（工作区 lint 与 011 决策）。
use super::{MAX_SOURCE_BYTES, required_path, run, usage, write_atomically};
use core::mem::DropGuard;
use std::fs;
use std::path::{Path, PathBuf};

// canonical 形态经 formatter 实测（cn 别名归一为 zh-CN、补唯一末尾换行）；
// dirty 夹具利用解析器容忍的差异（cn 别名、缺末尾换行），4 空格缩进会
// 被解析器直接拒绝，不能作为"待格式化"样本。
const LANGUAGE_CANONICAL: &str = "version: 1\nkind: language\nlanguage: zh-CN\nsourcelanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n";
const LANGUAGE_NEEDS_FORMAT: &str = "version: 1\nkind: language\nlanguage: cn\nsourcelanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi";
const VARIABLES: &str = "version: 1\nkind: variables\n\nvalues:\n  spacing-small: real = 8\n";
// canonical 形态由 formatter 产出：字段序 from/on/to/guard。
const FSM_CANONICAL: &str = "version: 1\nkind: fsm\nname: demo-fsm\ninitial: Idle\n\nstates:\n  Idle: active\n  Done: terminal\ntransitions:\n  go:\n    from: Idle\n    on: advance\n    to: Done\n";
const FSM_NEEDS_FORMAT: &str = "version: 1\nkind: fsm\nname: demo-fsm\ninitial: Idle\n\nstates:\n  Idle: active\n  Done: terminal\ntransitions:\n  go:\n    to: Done\n    from: Idle\n    on: advance\n";

/// 临时目录的清理守卫。
type TempDirGuard = DropGuard<PathBuf, fn(PathBuf)>;

/// 隔离临时目录夹具：测试结束整体清理，避免触碰仓库工作区。
fn temp_dir(label: &str) -> std::io::Result<(PathBuf, TempDirGuard)> {
    let path = std::env::temp_dir().join(format!("caret-dslc-{}-{label}", std::process::id()));
    fs::create_dir_all(&path)?;
    let guard: TempDirGuard = DropGuard::new(path.clone(), |path| {
        let _ = fs::remove_dir_all(path);
    });
    Ok((path, guard))
}

fn write(dir: &Path, name: &str, contents: &str) -> std::io::Result<PathBuf> {
    let file = dir.join(name);
    fs::write(&file, contents)?;
    Ok(file)
}

fn read(dir: &Path, name: &str) -> std::io::Result<String> {
    fs::read_to_string(dir.join(name))
}

#[test]
fn missing_or_unknown_command_reports_usage() {
    for arguments in [vec![], vec!["nonsense".to_owned()]] {
        assert!(
            matches!(
                run(arguments),
                Err(ref error) if error.to_string() == usage()
            ),
            "无效应命令必须报 usage"
        );
    }
}

#[test]
fn check_and_validate_accept_canonical_dictionary() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _cleanup) = temp_dir("check")?;
    let input = write(&dir, "ok.pa", LANGUAGE_CANONICAL)?;
    for command in ["check", "validate"] {
        run(vec![command.to_owned(), input.display().to_string()])?;
    }
    Ok(())
}

#[test]
fn check_reports_parse_diagnostics() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _cleanup) = temp_dir("check-bad")?;
    let input = write(&dir, "bad.pa", "version: 1\nkind: language\nlanguage: en\n")?;
    assert!(
        matches!(
            run(vec!["check".to_owned(), input.display().to_string()]),
            Err(ref error) if error.to_string().contains("pa.")
        ),
        "坏字典必须带诊断失败"
    );
    Ok(())
}

#[test]
fn check_accepts_fsm_documents_and_reports_fsm_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let (dir, _cleanup) = temp_dir("check-fsm")?;
    let input = write(&dir, "fsm.pa", FSM_CANONICAL)?;
    run(vec!["check".to_owned(), input.display().to_string()])?;
    run(vec!["validate".to_owned(), input.display().to_string()])?;

    let broken = write(
        &dir,
        "fsm-broken.pa",
        "version: 1\nkind: fsm\nname: broken\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: Nowhere\n",
    )?;
    assert!(
        matches!(
            run(vec!["check".to_owned(), broken.display().to_string()]),
            Err(ref error) if error.to_string().contains("pa.fsm_unknown_state")
        ),
        "坏 fsm 必须带 fsm 诊断"
    );

    // emit-ts 明确拒绝 fsm。
    assert!(
        matches!(
            run(vec![
                "emit-ts".to_owned(),
                input.display().to_string(),
                dir.join("out.ts").display().to_string(),
            ]),
            Err(ref error) if error.to_string() == "emit-ts requires kind: language"
        ),
        "fsm 文档不得生成 TS"
    );
    Ok(())
}

#[test]
fn format_normalizes_fsm_field_order() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _cleanup) = temp_dir("format-fsm")?;
    let dirty = write(&dir, "fsm-dirty.pa", FSM_NEEDS_FORMAT)?;
    run(vec!["format".to_owned(), dirty.display().to_string()])?;
    assert_eq!(read(&dir, "fsm-dirty.pa")?, FSM_CANONICAL);

    let clean = write(&dir, "fsm-clean.pa", FSM_CANONICAL)?;
    run(vec!["format".to_owned(), clean.display().to_string()])?;
    assert_eq!(read(&dir, "fsm-clean.pa")?, FSM_CANONICAL);

    let check_clean = vec![
        "format".to_owned(),
        "--check".to_owned(),
        clean.display().to_string(),
    ];
    run(check_clean)?;
    assert!(
        matches!(
            run(vec![
                "format".to_owned(),
                "--check".to_owned(),
                write(&dir, "fsm-dirty2.pa", FSM_NEEDS_FORMAT)?.display().to_string(),
            ]),
            Err(ref error) if error.to_string().contains("requires formatting")
        ),
        "--check 对脏 fsm 必须非零"
    );
    Ok(())
}

#[test]
fn emit_ts_writes_requested_locale_and_cleans_temporary() -> Result<(), Box<dyn std::error::Error>>
{
    let (dir, _cleanup) = temp_dir("emit")?;
    let input = write(&dir, "cn.pa", LANGUAGE_CANONICAL)?;
    let output = dir.join("out.ts");
    let arguments = vec![
        "emit-ts".to_owned(),
        input.display().to_string(),
        output.display().to_string(),
        "zh_CN".to_owned(),
    ];
    run(arguments)?;
    let ts = read(&dir, "out.ts")?;
    assert!(ts.contains("zh_CN"), "locale 应写入 TS");
    assert!(!dir.join("out.ts.tmp").exists(), "临时文件应被 rename 消费");
    Ok(())
}

#[test]
fn emit_ts_falls_back_to_document_locale() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _cleanup) = temp_dir("emit-doc-locale")?;
    let input = write(&dir, "en.pa", LANGUAGE_CANONICAL)?;
    let output = dir.join("out.ts");
    let arguments = vec![
        "emit-ts".to_owned(),
        input.display().to_string(),
        output.display().to_string(),
    ];
    run(arguments)?;
    assert!(output.exists());
    Ok(())
}

#[test]
fn emit_ts_rejects_non_language_kind_and_missing_output() -> Result<(), Box<dyn std::error::Error>>
{
    let (dir, _cleanup) = temp_dir("emit-bad")?;
    let input = write(&dir, "vars.pa", VARIABLES)?;
    assert!(
        matches!(
            run(vec![
                "emit-ts".to_owned(),
                input.display().to_string(),
                dir.join("out.ts").display().to_string(),
            ]),
            Err(ref error) if error.to_string() == "emit-ts requires kind: language"
        ),
        "variables 字典不得生成 TS"
    );
    assert!(
        matches!(
            run(vec!["emit-ts".to_owned(), input.display().to_string()]),
            Err(ref error) if error.to_string() == usage()
        ),
        "缺输出参数必须报 usage"
    );
    Ok(())
}

#[test]
fn format_writes_only_when_needed() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _cleanup) = temp_dir("format")?;
    let dirty = write(&dir, "dirty.pa", LANGUAGE_NEEDS_FORMAT)?;
    run(vec!["format".to_owned(), dirty.display().to_string()])?;
    assert_eq!(read(&dir, "dirty.pa")?, LANGUAGE_CANONICAL);

    let clean = write(&dir, "clean.pa", LANGUAGE_CANONICAL)?;
    run(vec!["format".to_owned(), clean.display().to_string()])?;
    assert_eq!(read(&dir, "clean.pa")?, LANGUAGE_CANONICAL);
    Ok(())
}

#[test]
fn format_check_distinguishes_clean_and_dirty() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, _cleanup) = temp_dir("format-check")?;
    let clean = write(&dir, "clean.pa", LANGUAGE_CANONICAL)?;
    let arguments = vec![
        "format".to_owned(),
        "--check".to_owned(),
        clean.display().to_string(),
    ];
    run(arguments)?;

    let dirty = write(&dir, "dirty.pa", LANGUAGE_NEEDS_FORMAT)?;
    assert!(
        matches!(
            run(vec![
                "format".to_owned(),
                "--check".to_owned(),
                dirty.display().to_string(),
            ]),
            Err(ref error) if error.to_string().contains("requires formatting")
        ),
        "--check 对脏文件必须非零"
    );
    Ok(())
}

#[test]
fn read_source_reports_missing_invalid_utf8_and_oversized_files()
-> Result<(), Box<dyn std::error::Error>> {
    let (dir, _cleanup) = temp_dir("read")?;
    assert!(
        matches!(
            run(vec![
                "check".to_owned(),
                dir.join("absent.pa").display().to_string(),
            ]),
            Err(ref error) if error.to_string().starts_with("read ")
        ),
        "缺失文件必须报 read 诊断"
    );

    let invalid = dir.join("invalid.pa");
    fs::write(&invalid, [0xffu8, 0xfe])?;
    assert!(
        matches!(
            run(vec!["check".to_owned(), invalid.display().to_string()]),
            Err(ref error) if error.to_string().starts_with("pa.invalid_utf8 at byte 0")
        ),
        "非法 UTF-8 必须带字节位置"
    );

    let oversized = dir.join("oversized.pa");
    fs::write(&oversized, vec![b'x'; MAX_SOURCE_BYTES + 1])?;
    assert!(matches!(
        run(vec!["check".to_owned(), oversized.display().to_string()]),
        Err(ref error) if error.to_string().starts_with("pa.source_too_large")
    ));
    Ok(())
}

#[test]
fn required_path_reports_usage_without_argument() {
    assert!(
        matches!(
            required_path(&["check".to_owned()], 1),
            Err(ref error) if *error == usage()
        ),
        "缺参必须报 usage"
    );
}

#[test]
fn write_atomically_replaces_content_and_covers_extensionless_target()
-> Result<(), Box<dyn std::error::Error>> {
    let (dir, _cleanup) = temp_dir("write")?;
    let target = write(&dir, "a.ts", "old")?;
    write_atomically(&target, b"new")?;
    assert_eq!(read(&dir, "a.ts")?, "new");
    assert!(!dir.join("a.ts.tmp").exists());

    let extensionless = dir.join("plain");
    write_atomically(&extensionless, b"x")?;
    assert_eq!(read(&dir, "plain")?, "x");
    Ok(())
}

#[test]
fn write_atomically_reports_create_and_replace_failures() -> Result<(), Box<dyn std::error::Error>>
{
    let (dir, _cleanup) = temp_dir("write-fail")?;
    assert!(
        matches!(
            write_atomically(&dir.join("absent-dir").join("a.ts"), b"x"),
            Err(ref error) if error.to_string().starts_with("write ")
        ),
        "不存在目录必须报 write 诊断"
    );

    // 目标是目录时 rename 失败：覆盖 replace 错误面。
    let as_directory = dir.join("as-dir.ts");
    fs::create_dir_all(&as_directory)?;
    assert!(
        matches!(
            write_atomically(&as_directory, b"x"),
            Err(ref error) if error.to_string().starts_with("replace ")
        ),
        "目录目标必须报 replace 诊断"
    );
    Ok(())
}
