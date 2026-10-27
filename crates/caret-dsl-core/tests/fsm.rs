//! FSM schema 行为测试：解析、静态校验、格式化往返与确定性生成。
//! 全部经公共 API（fsm::parse / fsm::format_source / fsm::generate_rust）驱动。

use std::error::Error;

use caret_dsl_core::fsm::{self, generate_rust, parse, snake_case, upper_camel_case};

// 与 formatter 规范形逐字节一致：头部、空行、注释、空行、states 节、
// transitions 节（节间无空行）、唯一末尾换行。
const OPEN_SAVED_NOTE: &str = "version: 1\nkind: fsm\nname: open-saved-note\ninitial: Idle\n\n// 只读激活流程：读取并解析已提交的笔记。\n\nstates:\n  Idle: active\n  LoadingAsset: active\n  Parsing: active\n  Ready: terminal\n  Failed: terminal\n  Cancelled: terminal\n  Expired: terminal\ntransitions:\n  begin:\n    from: Idle\n    on: open-requested\n    to: LoadingAsset\n    guard: record-valid\n  spawn-failed:\n    from: Idle\n    on: fail\n    to: Failed\n  asset-read:\n    from: LoadingAsset\n    on: asset-read\n    to: Parsing\n  read-failed:\n    from: LoadingAsset\n    on: fail\n    to: Failed\n  parse-completed:\n    from: Parsing\n    on: parse-succeeded\n    to: Ready\n    guard: session-current\n  parse-failed:\n    from: Parsing\n    on: fail\n    to: Failed\n  cancel-before-read:\n    from: Idle\n    on: cancel-acknowledged\n    to: Cancelled\n  cancel-while-loading:\n    from: LoadingAsset\n    on: cancel-acknowledged\n    to: Cancelled\n  cancel-while-parsing:\n    from: Parsing\n    on: cancel-acknowledged\n    to: Cancelled\n  expire-while-loading:\n    from: LoadingAsset\n    on: generation-invalidated\n    to: Expired\n  expire-while-parsing:\n    from: Parsing\n    on: generation-invalidated\n    to: Expired\n";

fn assert_rejected(source: &str, code: &str) -> Result<(), Box<dyn Error>> {
    assert!(
        matches!(
            parse(source),
            Err(ref diagnostics) if diagnostics.to_string().contains(code)
        ),
        "'{code}' 应拒绝：{source:?}"
    );
    Ok(())
}

#[test]
fn parses_fsm_document_with_states_and_transitions() -> Result<(), Box<dyn Error>> {
    let document = parse(OPEN_SAVED_NOTE)?;
    assert_eq!(document.version, 1);
    assert_eq!(document.name, "open-saved-note");
    assert_eq!(document.initial, "Idle");
    assert_eq!(document.states.len(), 7);
    assert_eq!(document.transitions.len(), 11);
    assert_eq!(document.states[3].name, "Ready");
    assert!(document.states[3].terminal);
    assert!(
        document
            .comments
            .iter()
            .any(|comment| comment.contains("只读激活"))
    );
    let guard_edge = document
        .transitions
        .iter()
        .find(|transition| transition.id == "parse-completed")
        .ok_or("parse-completed missing")?;
    assert_eq!(guard_edge.guard.as_deref(), Some("session-current"));
    Ok(())
}

#[test]
fn formatter_is_canonical_and_idempotent() -> Result<(), Box<dyn Error>> {
    let formatted = fsm::format_source(OPEN_SAVED_NOTE)?;
    assert_eq!(formatted, OPEN_SAVED_NOTE, "规范 fsm 应幂等");
    assert_eq!(parse(&formatted)?, parse(OPEN_SAVED_NOTE)?);

    // 字段乱序在语义保持下规范为 from/on/to/guard；声明顺序不变。
    let shuffled = "version: 1\nkind: fsm\nname: shuffle\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    to: B\n    from: A\n    on: advance\n";
    let canonical = fsm::format_source(shuffled)?;
    assert!(canonical.contains("  go:\n    from: A\n    on: advance\n    to: B\n"));
    assert_eq!(fsm::format_source(&canonical)?, canonical);
    Ok(())
}

#[test]
fn name_conversions_are_stable() -> Result<(), Box<dyn Error>> {
    assert_eq!(upper_camel_case("open-requested"), "OpenRequested");
    assert_eq!(upper_camel_case("record-valid"), "RecordValid");
    assert_eq!(snake_case("open-saved-note"), "open_saved_note");
    Ok(())
}

#[test]
fn cycles_with_a_terminal_exit_remain_valid_even_when_the_exit_is_guarded()
-> Result<(), Box<dyn Error>> {
    let source = "version: 1\nkind: fsm\nname: retry\ninitial: Idle\n\nstates:\n  Idle: active\n  Retrying: active\n  Done: terminal\ntransitions:\n  begin:\n    from: Idle\n    on: retry\n    to: Retrying\n  repeat:\n    from: Retrying\n    on: retry\n    to: Idle\n  finish:\n    from: Retrying\n    on: ready\n    to: Done\n    guard: ready\n";
    let document = parse(source)?;
    assert_eq!(document.transitions.len(), 3);
    let generated = generate_rust(&document);
    assert!(generated.contains("EventKind::Ready"));
    assert!(generated.contains("Guard::Ready"));
    assert_eq!(parse(&fsm::format_source(source)?)?, document);
    Ok(())
}

#[test]
fn guard_names_have_their_own_collision_and_keyword_checks() -> Result<(), Box<dyn Error>> {
    let source = "version: 1\nkind: fsm\nname: guarded\ninitial: Idle\n\nstates:\n  Idle: active\n  Done: terminal\ntransitions:\n  first:\n    from: Idle\n    on: first\n    to: Done\n    guard: a1\n  second:\n    from: Idle\n    on: second\n    to: Done\n    guard: a-1\n";
    assert_rejected(source, "pa.fsm_name_collision")?;
    assert_rejected(&source.replace("a-1", "self"), "pa.fsm_reserved_name")?;
    assert_rejected(
        &source.replace("a-1", "NotCanonical"),
        "pa.fsm_invalid_name",
    )
}

#[test]
fn rejects_unknown_states_and_initial() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: Missing\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n",
        "pa.fsm_unknown_state",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: Nowhere\n",
        "pa.fsm_unknown_state",
    )
}

#[test]
fn rejects_terminal_initial_duplicate_states_and_terminal_outgoing() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: Done\n\nstates:\n  Done: terminal\n  Idle: active\n  Done2: terminal\n\ntransitions:\n  go:\n    from: Idle\n    on: advance\n    to: Done\n",
        "pa.fsm_invalid_initial",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n  A: active\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n",
        "pa.fsm_duplicate_state",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n  C: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n  leak:\n    from: B\n    on: regress\n    to: C\n",
        "pa.fsm_terminal_outgoing",
    )
}

#[test]
fn rejects_missing_headers_duplicate_sections_and_syntax() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        "kind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n",
        "pa.missing_version",
    )?;
    assert_rejected(
        "version: 1\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n",
        "pa.missing_kind",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\ninitial: B\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n",
        "pa.duplicate_header",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\ntransitions:\n  again:\n    from: A\n    on: other\n    to: B\n",
        "pa.fsm_duplicate_section",
    )?;
    assert_rejected("version: nope\nkind: fsm\n", "pa.syntax")?;
    Ok(())
}

#[test]
fn rejects_ambiguous_edges_and_duplicate_transition_ids() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n  C: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n  again:\n    from: A\n    on: advance\n    to: C\n",
        "pa.fsm_ambiguous_edge",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n  go:\n    from: A\n    on: regress\n    to: B\n",
        "pa.fsm_duplicate_transition",
    )
}

#[test]
fn rejects_unreachable_dead_and_terminalless_states() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n  Orphan: active\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n",
        "pa.fsm_unreachable_state",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n  Orphan: active\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n  reach:\n    from: A\n    on: enter-orphan\n    to: Orphan\n",
        "pa.fsm_dead_state",
    )?;
    // Initial 与 Orphan 互转、无任何终态边。
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: Initial\n\nstates:\n  Initial: active\n  Orphan: active\n  Done: terminal\n\ntransitions:\n  swap:\n    from: Initial\n    on: swap\n    to: Orphan\n  back:\n    from: Orphan\n    on: swap-back\n    to: Initial\n",
        "pa.fsm_no_terminal_path",
    )
}

#[test]
fn rejects_missing_sections_fields_and_headers() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n",
        "pa.fsm_missing_section",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\ninitial: A\n\nstates:\n  A: active\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: A\n",
        "pa.fsm_missing_name",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n\ntransitions:\n  go:\n    on: advance\n    to: A\n",
        "pa.fsm_missing_field",
    )?;
    assert_rejected(
        "version: 2\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: A\n",
        "pa.unsupported_version",
    )?;
    // kind 非 fsm 但其余为 fsm 形状：由 fsm schema 报 kind 错配。
    assert_rejected(
        "version: 1\nkind: language\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n",
        "pa.fsm_kind_mismatch",
    )?;
    // 状态声明落在 transitions 节。
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n  Wrong: active\n",
        "pa.fsm_state_outside_section",
    )
}

#[test]
fn rejects_reserved_names_and_field_duplication() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  Self: active\n  B: terminal\n\ntransitions:\n  go:\n    from: Self\n    on: advance\n    to: B\n",
        "pa.fsm_reserved_name",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: self\n    to: B\n",
        "pa.fsm_reserved_name",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    from: A\n    on: advance\n    to: B\n",
        "pa.fsm_duplicate_field",
    )
}

#[test]
fn rejects_tab_indentation_and_noncanonical_names() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n\tA: active\n",
        "pa.tab_indentation",
    )?;
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad--name\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: advance\n    to: B\n",
        "pa.fsm_invalid_name",
    )?;
    // 事件必须为 kebab-case；大写开头的 state 名不是合法事件名。
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  go:\n    from: A\n    on: Advance\n    to: B\n",
        "pa.fsm_invalid_name",
    )?;
    // transition id 同样受限。
    assert_rejected(
        "version: 1\nkind: fsm\nname: bad\ninitial: A\n\nstates:\n  A: active\n  B: terminal\n\ntransitions:\n  Go:\n    from: A\n    on: advance\n    to: B\n",
        "pa.fsm_invalid_name",
    )
}

#[test]
fn generates_deterministic_metadata_with_guards() -> Result<(), Box<dyn Error>> {
    let document = parse(OPEN_SAVED_NOTE)?;
    let first = generate_rust(&document);
    let second = generate_rust(&parse(OPEN_SAVED_NOTE)?);
    assert_eq!(first, second, "同输入必须产出一致元数据");
    assert!(first.contains("enum State {"), "状态枚举应生成");
    assert!(
        first.contains("EventKind::ParseSucceeded"),
        "kebab 应转换事件"
    );
    assert!(first.contains("Guard::SessionCurrent"), "guard 关联应保留");
    assert!(
        first.contains("Some(Guard::RecordValid)"),
        "begin 边应带 guard"
    );
    assert!(first.contains("id: \"parse-completed\""), "边 ID 应保留");
    assert!(first.contains("const TERMINAL_STATES"), "终态集合应生成");
    assert!(first.contains("const INITIAL_STATE: State = State::Idle;"));
    Ok(())
}

#[test]
fn rejects_duplicate_headers_and_sections_and_missing_initial() -> Result<(), Box<dyn Error>> {
    let duplicated = "version: 1
version: 1
kind: fsm
kind: fsm
name: bad
name: bad
initial: A
initial: A

states:
  A: active
  B: terminal

transitions:
  go:
    from: A
    on: advance
    to: B
";
    let error = match parse(duplicated) {
        Ok(document) => panic!("duplicated headers accepted as {}", document.name),
        Err(diagnostics) => diagnostics,
    };
    assert!(
        error
            .diagnostics
            .iter()
            .filter(|item| item.code == "pa.duplicate_header")
            .count()
            >= 4,
        "version/kind/name/initial 重复都应定位报告"
    );
    assert_rejected(
        "version: 1
kind: fsm
name: bad
initial: A

states:
  A: active
  B: terminal

states:
  A: active

transitions:
  go:
    from: A
    on: advance
    to: B
",
        "pa.fsm_duplicate_section",
    )?;
    assert_rejected(
        "version: 1
kind: fsm
name: bad

states:
  A: active
  B: terminal

transitions:
  go:
    from: A
    on: advance
    to: B
",
        "pa.fsm_missing_initial",
    )
}

#[test]
fn rejects_transition_field_outside_transition_and_missing_to() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        "version: 1
kind: fsm
name: bad
initial: A

states:
  A: active
  B: terminal

transitions:
    from: A
  go:
    from: A
    on: advance
    to: B
",
        "pa.fsm_field_outside_transition",
    )?;
    assert_rejected(
        "version: 1
kind: fsm
name: bad
initial: A

states:
  A: active
  B: terminal

transitions:
  go:
    from: A
    on: advance
    on: regress
    to: B
",
        "pa.fsm_duplicate_field",
    )?;
    Ok(())
}

// 容量边界必须构造 129 状态或 513 条边；Pest 在 Miri 中逐条解释执行超过
// 可行时间。常规 cargo test 验证阈值，Miri 保留其余 FSM 解析行为测试。
#[test]
#[cfg_attr(miri, ignore)]
fn rejects_state_and_transition_limits() -> Result<(), Box<dyn Error>> {
    let mut big_states = String::from(
        "version: 1
kind: fsm
name: big
initial: A000

states:
",
    );
    for index in 0..129 {
        big_states.push_str(&format!(
            "  A{index:03}: active
"
        ));
    }
    big_states.push_str(
        "
transitions:
  go:
    from: A000
    on: advance
    to: A000
",
    );
    assert_rejected(&big_states, "pa.fsm_too_many_states")?;

    let mut big_transitions = String::from(
        "version: 1
kind: fsm
name: big
initial: A

states:
  A: active
  B: terminal

transitions:
",
    );
    for index in 0..513 {
        big_transitions.push_str(&format!(
            "  go{index:03}:
    from: A
    on: event-{index:03}
    to: B
"
        ));
    }
    assert_rejected(&big_transitions, "pa.fsm_too_many_transitions")
}

#[test]
fn rejects_kebab_names_converting_to_the_same_variant() -> Result<(), Box<dyn Error>> {
    // `a1` 与 `a-1` 都转换为 `A1`：生成枚举变体冲突必须拒绝。
    assert_rejected(
        "version: 1
kind: fsm
name: bad
initial: A

states:
  A: active
  B: terminal

transitions:
  go1:
    from: A
    on: a1
    to: B
  go2:
    from: A
    on: a-1
    to: B
",
        "pa.fsm_name_collision",
    )
}

#[test]
#[cfg_attr(miri, ignore)]
fn accepts_documents_exactly_at_resource_limits() -> Result<(), Box<dyn Error>> {
    // 恰好 128 状态（127 active 链 + 1 终态）与 127 条边：合法上限内。
    let mut exact = String::from(
        "version: 1
kind: fsm
name: chain
initial: A000

states:
",
    );
    for index in 0..127 {
        exact.push_str(&format!(
            "  A{index:03}: active
"
        ));
    }
    exact.push_str(
        "  T: terminal

transitions:
",
    );
    for index in 0..127 {
        let target = if index + 1 == 127 {
            "T".to_owned()
        } else {
            format!("A{:03}", index + 1)
        };
        exact.push_str(&format!(
            "  go{index:03}:
    from: A{index:03}
    on: advance-{index:03}
    to: {target}
"
        ));
    }
    let document = parse(&exact)?;
    assert_eq!(document.states.len(), 128);
    assert!(fsm::format_source(&exact)?.starts_with(
        "version: 1
kind: fsm
"
    ));

    // 恰好 512 条边：全部 (from, on) 唯一。
    let mut edges = String::from(
        "version: 1
kind: fsm
name: many
initial: A

states:
  A: active
  B: terminal

transitions:
",
    );
    for index in 0..512 {
        edges.push_str(&format!(
            "  go{index:03}:
    from: A
    on: event-{index:03}
    to: B
"
        ));
    }
    assert_eq!(parse(&edges)?.transitions.len(), 512);
    Ok(())
}

#[test]
fn rejects_missing_on_and_to_fields_individually() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        "version: 1
kind: fsm
name: bad
initial: A

states:
  A: active
  B: terminal

transitions:
  go:
    from: A
    to: B
",
        "pa.fsm_missing_field",
    )?;
    assert_rejected(
        "version: 1
kind: fsm
name: bad
initial: A

states:
  A: active
  B: terminal

transitions:
  go:
    from: A
    on: advance
",
        "pa.fsm_missing_field",
    )
}

#[test]
fn rejects_transition_declaration_inside_states_section() -> Result<(), Box<dyn Error>> {
    assert_rejected(
        "version: 1
kind: fsm
name: bad
initial: A

states:
  A: active
  B: terminal
  go:
    from: A
    on: advance
    to: B

transitions:
  move:
    from: A
    on: advance
    to: B
",
        "pa.fsm_transition_outside_section",
    )
}

#[test]
fn rejects_oversized_sources_before_parsing() -> Result<(), Box<dyn Error>> {
    let mut oversized = String::from(
        "version: 1
kind: fsm
name: big
initial: A

states:
  A: active
",
    );
    oversized.push_str(
        &"// pad
"
        .repeat(150_000),
    );
    assert_rejected(&oversized, "pa.source_too_large")
}

#[test]
fn generates_guardless_metadata_without_guard_enum_members() -> Result<(), Box<dyn Error>> {
    let document = parse(
        "version: 1
kind: fsm
name: plain
initial: A

states:
  A: active
  B: terminal

transitions:
  go:
    from: A
    on: advance
    to: B
",
    )?;
    let generated = generate_rust(&document);
    assert!(
        generated.contains(
            "enum Guard {
}"
        ),
        "无 guard 时枚举为空"
    );
    assert!(
        !generated.contains("impl Guard"),
        "空 guard 不应生成求值接口"
    );
    assert!(!generated.contains("Some(Guard::"));
    Ok(())
}
