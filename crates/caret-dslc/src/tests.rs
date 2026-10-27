//! 验证 DSL 编译器命令入口和错误处理。

// 测试统一以 ? 传播错误：避免永不执行的错误闭包（会被计为未覆盖行），
// 同时不使用 unwrap/expect（工作区 lint 与 011 决策）。
use super::{MAX_SOURCE_BYTES, required_path, run, usage, write_atomically};
use std::fs;
use std::path::PathBuf;

// canonical 形态经 formatter 实测（cn 别名归一为 zh-CN、补唯一末尾换行）；
// dirty 夹具利用解析器容忍的差异（cn 别名、缺末尾换行），4 空格缩进会
// 被解析器直接拒绝，不能作为"待格式化"样本。
const LANGUAGE_CANONICAL: &str = "version: 1\nkind: language\nlanguage: zh-CN\nsourcelanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi\n";
const LANGUAGE_NEEDS_FORMAT: &str = "version: 1\nkind: language\nlanguage: cn\nsourcelanguage: en\n\n[App]\ntitle:\n  src: Hi\n  tr: Hi";
const VARIABLES: &str = "version: 1\nkind: variables\n\nvalues:\n  spacing-small: real = 8\n";
// canonical 形态由 formatter 产出：字段序 from/on/to/guard。
const FSM_CANONICAL: &str = "version: 1\nkind: fsm\nname: demo-fsm\ninitial: Idle\n\nstates:\n  Idle: active\n  Done: terminal\ntransitions:\n  go:\n    from: Idle\n    on: advance\n    to: Done\n";
const FSM_NEEDS_FORMAT: &str = "version: 1\nkind: fsm\nname: demo-fsm\ninitial: Idle\n\nstates:\n  Idle: active\n  Done: terminal\ntransitions:\n  go:\n    to: Done\n    from: Idle\n    on: advance\n";

/// 隔离临时目录：测试结束整体清理，避免触碰仓库工作区。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("caret-dslc-{}-{label}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn write(&self, name: &str, contents: &str) -> std::io::Result<PathBuf> {
        let file = self.path.join(name);
        fs::write(&file, contents)?;
        Ok(file)
    }

    fn read(&self, name: &str) -> std::io::Result<String> {
        fs::read_to_string(self.path.join(name))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
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
    let dir = TempDir::new("check")?;
    let input = dir.write("ok.pa", LANGUAGE_CANONICAL)?;
    for command in ["check", "validate"] {
        run(vec![command.to_owned(), input.display().to_string()])?;
    }
    Ok(())
}

#[test]
fn check_reports_parse_diagnostics() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("check-bad")?;
    let input = dir.write("bad.pa", "version: 1\nkind: language\nlanguage: en\n")?;
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
    let dir = TempDir::new("check-fsm")?;
    let input = dir.write("fsm.pa", FSM_CANONICAL)?;
    run(vec!["check".to_owned(), input.display().to_string()])?;
    run(vec!["validate".to_owned(), input.display().to_string()])?;

    let broken = dir.write(
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
                dir.path.join("out.ts").display().to_string(),
            ]),
            Err(ref error) if error.to_string() == "emit-ts requires kind: language"
        ),
        "fsm 文档不得生成 TS"
    );
    Ok(())
}

#[test]
fn format_normalizes_fsm_field_order() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("format-fsm")?;
    let dirty = dir.write("fsm-dirty.pa", FSM_NEEDS_FORMAT)?;
    run(vec!["format".to_owned(), dirty.display().to_string()])?;
    assert_eq!(dir.read("fsm-dirty.pa")?, FSM_CANONICAL);

    let clean = dir.write("fsm-clean.pa", FSM_CANONICAL)?;
    run(vec!["format".to_owned(), clean.display().to_string()])?;
    assert_eq!(dir.read("fsm-clean.pa")?, FSM_CANONICAL);

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
                dir.write("fsm-dirty2.pa", FSM_NEEDS_FORMAT)?.display().to_string(),
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
    let dir = TempDir::new("emit")?;
    let input = dir.write("cn.pa", LANGUAGE_CANONICAL)?;
    let output = dir.path.join("out.ts");
    let arguments = vec![
        "emit-ts".to_owned(),
        input.display().to_string(),
        output.display().to_string(),
        "zh_CN".to_owned(),
    ];
    run(arguments)?;
    let ts = dir.read("out.ts")?;
    assert!(ts.contains("zh_CN"), "locale 应写入 TS");
    assert!(
        !dir.path.join("out.ts.tmp").exists(),
        "临时文件应被 rename 消费"
    );
    Ok(())
}

#[test]
fn emit_ts_falls_back_to_document_locale() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("emit-doc-locale")?;
    let input = dir.write("en.pa", LANGUAGE_CANONICAL)?;
    let output = dir.path.join("out.ts");
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
    let dir = TempDir::new("emit-bad")?;
    let input = dir.write("vars.pa", VARIABLES)?;
    assert!(
        matches!(
            run(vec![
                "emit-ts".to_owned(),
                input.display().to_string(),
                dir.path.join("out.ts").display().to_string(),
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
    let dir = TempDir::new("format")?;
    let dirty = dir.write("dirty.pa", LANGUAGE_NEEDS_FORMAT)?;
    run(vec!["format".to_owned(), dirty.display().to_string()])?;
    assert_eq!(dir.read("dirty.pa")?, LANGUAGE_CANONICAL);

    let clean = dir.write("clean.pa", LANGUAGE_CANONICAL)?;
    run(vec!["format".to_owned(), clean.display().to_string()])?;
    assert_eq!(dir.read("clean.pa")?, LANGUAGE_CANONICAL);
    Ok(())
}

#[test]
fn format_check_distinguishes_clean_and_dirty() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("format-check")?;
    let clean = dir.write("clean.pa", LANGUAGE_CANONICAL)?;
    let arguments = vec![
        "format".to_owned(),
        "--check".to_owned(),
        clean.display().to_string(),
    ];
    run(arguments)?;

    let dirty = dir.write("dirty.pa", LANGUAGE_NEEDS_FORMAT)?;
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
    let dir = TempDir::new("read")?;
    assert!(
        matches!(
            run(vec![
                "check".to_owned(),
                dir.path.join("absent.pa").display().to_string(),
            ]),
            Err(ref error) if error.to_string().starts_with("read ")
        ),
        "缺失文件必须报 read 诊断"
    );

    let invalid = dir.path.join("invalid.pa");
    fs::write(&invalid, [0xffu8, 0xfe])?;
    assert!(
        matches!(
            run(vec!["check".to_owned(), invalid.display().to_string()]),
            Err(ref error) if error.to_string().starts_with("pa.invalid_utf8 at byte 0")
        ),
        "非法 UTF-8 必须带字节位置"
    );

    let oversized = dir.path.join("oversized.pa");
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
    let dir = TempDir::new("write")?;
    let target = dir.write("a.ts", "old")?;
    write_atomically(&target, b"new")?;
    assert_eq!(dir.read("a.ts")?, "new");
    assert!(!dir.path.join("a.ts.tmp").exists());

    let extensionless = dir.path.join("plain");
    write_atomically(&extensionless, b"x")?;
    assert_eq!(dir.read("plain")?, "x");
    Ok(())
}

#[test]
fn write_atomically_reports_create_and_replace_failures() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new("write-fail")?;
    assert!(
        matches!(
            write_atomically(&dir.path.join("absent-dir").join("a.ts"), b"x"),
            Err(ref error) if error.to_string().starts_with("write ")
        ),
        "不存在目录必须报 write 诊断"
    );

    // 目标是目录时 rename 失败：覆盖 replace 错误面。
    let as_directory = dir.path.join("as-dir.ts");
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
