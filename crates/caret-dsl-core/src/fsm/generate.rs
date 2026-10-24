//! 从已校验 FSM 文档确定性生成 Rust 转移元数据。
//!
//! 只生成结构元数据（状态 / 事件种类 / guard / 带边 ID 的转移表），不生成
//! 执行引擎；手写行为与生成表的一致性由消费者 crate 的双向结构测试保证。
//! 输出顺序由文档声明顺序决定，不嵌入时间戳或本机路径。

use std::fmt::Write;

use super::{FsmDocument, upper_camel_case};

/// 生成可被 `include!` 进私有模块的 Rust 元数据源码。
pub fn generate_rust(document: &FsmDocument) -> String {
    let mut events: Vec<String> = Vec::new();
    let mut guards: Vec<String> = Vec::new();
    let mut rows = String::new();
    for transition in &document.transitions {
        let event = upper_camel_case(&transition.on);
        if !events.iter().any(|existing| existing == &event) {
            events.push(event.clone());
        }
        let guard = match transition.guard.as_deref() {
            Some(value) => {
                let converted = upper_camel_case(value);
                if !guards.iter().any(|existing| existing == &converted) {
                    guards.push(converted.clone());
                }
                format!("Some(Guard::{converted})")
            }
            None => "None".to_owned(),
        };
        let _ = writeln!(
            rows,
            "    Transition {{ id: \"{}\", from: State::{}, on: EventKind::{}, to: State::{}, guard: {} }},",
            transition.id, transition.from, event, transition.to, guard
        );
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "// 有限状态机（FSM）'{}'：由 caret-dsl-core 从 .pa 声明确定性生成；",
        document.name
    );
    out.push_str("// 请勿手工修改。\n");
    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub(super) enum State {\n");
    for fsm_state in &document.states {
        let _ = writeln!(out, "    {},", fsm_state.name);
    }
    out.push_str(
        "}\n\nimpl State {\n    pub(super) fn name(self) -> &'static str {\n        match self {\n",
    );
    for fsm_state in &document.states {
        let _ = writeln!(
            out,
            "            State::{} => \"{}\",",
            fsm_state.name, fsm_state.name
        );
    }
    out.push_str("        }\n    }\n}\n\n");

    let _ = writeln!(
        out,
        "pub(super) const INITIAL_STATE: State = State::{};\n",
        document.initial
    );

    out.push_str("pub(super) const ALL_STATES: &[State] = &[\n");
    for fsm_state in &document.states {
        let _ = writeln!(out, "    State::{},", fsm_state.name);
    }
    out.push_str("];\n\npub(super) const TERMINAL_STATES: &[State] = &[\n");
    for fsm_state in &document.states {
        if fsm_state.terminal {
            let _ = writeln!(out, "    State::{},", fsm_state.name);
        }
    }
    out.push_str("];\n\n");

    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub(super) enum EventKind {\n");
    for event in &events {
        let _ = writeln!(out, "    {event},");
    }
    out.push_str("}\n\nimpl EventKind {\n    pub(super) fn name(self) -> &'static str {\n        match self {\n");
    for event in &events {
        let _ = writeln!(out, "            EventKind::{event} => \"{event}\",");
    }
    out.push_str("        }\n    }\n}\n\npub(super) const ALL_EVENTS: &[EventKind] = &[\n");
    for event in &events {
        let _ = writeln!(out, "    EventKind::{event},");
    }
    out.push_str("];\n\n");

    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub(super) enum Guard {\n");
    for guard in &guards {
        let _ = writeln!(out, "    {guard},");
    }
    out.push_str("}\n");
    if !guards.is_empty() {
        out.push_str("\nimpl Guard {\n    pub(super) fn name(self) -> &'static str {\n        match self {\n");
        for guard in &guards {
            let _ = writeln!(out, "            Guard::{guard} => \"{guard}\",");
        }
        out.push_str("        }\n    }\n}\n");
    }

    out.push_str("\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub(super) struct Transition {\n    pub(super) id: &'static str,\n    pub(super) from: State,\n    pub(super) on: EventKind,\n    pub(super) to: State,\n    pub(super) guard: Option<Guard>,\n}\n\npub(super) const TRANSITIONS: &[Transition] = &[\n");
    out.push_str(&rows);
    out.push_str("];\n");
    out
}
