//! `.pa` 有限状态机（FSM）声明：schema、校验与格式化（073）。
//!
//! FSM 是独立于 language/theme/variables 的文档种类：一个文件一个流程，
//! 顶层仅允许 `version / kind / name / initial` 与 `states / transitions`
//! 两节。格式化、CLI 与消费者 build script 共用本模块的解析与校验；
//! 确定性 Rust 元数据生成见 [`generate_rust`]。

mod generate;

pub use generate::generate_rust;

use std::collections::{BTreeMap, BTreeSet};

use pest::Parser as PestParser;

use crate::diagnostic::push_diagnostic;
use crate::syntax::{MAX_SOURCE_BYTES, PaParser, Rule, pest_diagnostic};
use crate::{Diagnostic, Diagnostics};

/// 状态与转移的资源上限：与 language 的声明上限同型，保证生成体积可测。
pub const MAX_FSM_STATES: usize = 128;
pub const MAX_FSM_TRANSITIONS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsmState {
    /// UpperCamelCase 状态名；生成 Rust 枚举变体时按原样使用。
    pub name: String,
    /// 终态标记：终态不得有出边，`active` 状态必须有出边且可达终态。
    pub terminal: bool,
    /// 声明行在源码中的字节偏移，供定位诊断使用。
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsmTransition {
    /// kebab-case 转移 ID；只用于诊断与测试关联，不参与状态转移决策。
    pub id: String,
    /// 源状态 / 目标状态名；必须是已声明的状态。
    pub from: String,
    /// 触发事件名（kebab-case），生成 `EventKind` 变体的来源。
    pub on: String,
    pub to: String,
    /// 可选 guard 名（kebab-case），生成 `Guard` 变体的来源。
    pub guard: Option<String>,
    /// 声明行在源码中的字节偏移，供定位诊断使用。
    pub offset: usize,
}

/// 已校验的 FSM 文档；states / transitions 的顺序即声明顺序，
/// 也是生成器输出枚举变体与转移表行序的唯一依据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsmDocument {
    pub version: u32,
    pub name: String,
    pub initial: String,
    pub states: Vec<FsmState>,
    pub transitions: Vec<FsmTransition>,
    /// 独立注释；格式化时整体保留在头部之后。
    pub comments: Vec<String>,
}

/// 解析 `.pa` 有限状态机（FSM）文档并做静态校验。
pub fn parse(source: &str) -> Result<FsmDocument, Diagnostics> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Diagnostics::one(
            "pa.source_too_large",
            format!("source exceeds {} bytes", MAX_SOURCE_BYTES),
            0,
            source,
        ));
    }
    if let Some(offset) = source.find('\t') {
        return Err(Diagnostics::one(
            "pa.tab_indentation",
            "tabs are not allowed; use two spaces for indentation",
            offset,
            source,
        ));
    }

    let Some(document_pair) = PaParser::parse(Rule::fsm_document, source)
        .map_err(|error| pest_diagnostic(error, source))?
        .next()
    else {
        return Err(Diagnostics::one(
            "pa.syntax",
            "document is empty",
            0,
            source,
        ));
    };

    let mut version: Option<u32> = None;
    let mut kind: Option<String> = None;
    let mut name: Option<(String, usize)> = None;
    let mut initial: Option<(String, usize)> = None;
    let mut section = FsmSection::Header;
    let mut seen_states_header = false;
    let mut seen_transitions_header = false;
    let mut states: Vec<FsmState> = Vec::new();
    let mut transitions: Vec<FsmTransition> = Vec::new();
    // 当前 transition：id、(offset, from/on/to/guard 值与字段偏移)。
    let mut current: Option<PartialTransition> = None;
    let mut comments = Vec::new();
    let mut diagnostics = Vec::new();

    for line in document_pair.into_inner() {
        let Some(body) = line.into_inner().next() else {
            continue;
        };
        let offset = body.as_span().start();
        match body.as_rule() {
            Rule::blank => {}
            Rule::comment_line => {
                let comment = body.as_str().trim();
                if !comment.is_empty() {
                    comments.push(comment.to_owned());
                }
            }
            Rule::version => {
                if version.is_some() {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.duplicate_header",
                        "version is repeated",
                        offset,
                        source,
                    );
                } else {
                    version = Some(
                        find_text(body, Rule::integer)
                            .and_then(|value| value.parse::<u32>().ok())
                            .unwrap_or_default(),
                    );
                }
            }
            Rule::kind => {
                if kind.is_some() {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.duplicate_header",
                        "kind is repeated",
                        offset,
                        source,
                    );
                } else {
                    kind = Some(find_text(body, Rule::kind_name).unwrap_or_default());
                }
            }
            Rule::fsm_name => {
                if name.is_some() {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.duplicate_header",
                        "name is repeated",
                        offset,
                        source,
                    );
                } else {
                    let value = find_text(body, Rule::fsm_kebab).unwrap_or_default();
                    if is_canonical_kebab(&value) {
                        name = Some((value, offset));
                    } else {
                        push_diagnostic(
                            &mut diagnostics,
                            "pa.fsm_invalid_name",
                            format!("fsm name '{value}' must use canonical kebab-case"),
                            offset,
                            source,
                        );
                    }
                }
            }
            Rule::fsm_initial => {
                if initial.is_some() {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.duplicate_header",
                        "initial is repeated",
                        offset,
                        source,
                    );
                } else {
                    initial = Some((
                        find_text(body, Rule::fsm_state_name).unwrap_or_default(),
                        offset,
                    ));
                }
            }
            Rule::fsm_states_header => {
                flush_transition(&mut current, &mut transitions);
                if seen_states_header {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.fsm_duplicate_section",
                        "states is repeated",
                        offset,
                        source,
                    );
                }
                seen_states_header = true;
                section = FsmSection::States;
            }
            Rule::fsm_transitions_header => {
                flush_transition(&mut current, &mut transitions);
                if seen_transitions_header {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.fsm_duplicate_section",
                        "transitions is repeated",
                        offset,
                        source,
                    );
                }
                seen_transitions_header = true;
                section = FsmSection::Transitions;
            }
            Rule::fsm_state_decl => {
                if section != FsmSection::States {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.fsm_state_outside_section",
                        "state declaration must be inside states",
                        offset,
                        source,
                    );
                    continue;
                }
                if states.len() >= MAX_FSM_STATES {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.fsm_too_many_states",
                        format!("more than {MAX_FSM_STATES} states"),
                        offset,
                        source,
                    );
                    continue;
                }
                let fsm_state = find_text(body.clone(), Rule::fsm_state_name).unwrap_or_default();
                let terminal =
                    find_text(body, Rule::fsm_state_kind).is_some_and(|value| value == "terminal");
                if !is_rust_keyword(&fsm_state) {
                    states.push(FsmState {
                        name: fsm_state,
                        terminal,
                        offset,
                    });
                } else {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.fsm_reserved_name",
                        format!("state name '{fsm_state}' is a Rust keyword"),
                        offset,
                        source,
                    );
                }
            }
            Rule::fsm_transition_decl => {
                if section != FsmSection::Transitions {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.fsm_transition_outside_section",
                        "transition declaration must be inside transitions",
                        offset,
                        source,
                    );
                    continue;
                }
                flush_transition(&mut current, &mut transitions);
                if transitions.len() >= MAX_FSM_TRANSITIONS {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.fsm_too_many_transitions",
                        format!("more than {MAX_FSM_TRANSITIONS} transitions"),
                        offset,
                        source,
                    );
                    continue;
                }
                let id = find_text(body, Rule::fsm_kebab).unwrap_or_default();
                if !is_canonical_kebab(&id) {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.fsm_invalid_name",
                        format!("transition id '{id}' must use canonical kebab-case"),
                        offset,
                        source,
                    );
                }
                current = Some(PartialTransition {
                    id,
                    offset,
                    from: None,
                    on: None,
                    to: None,
                    guard: None,
                });
            }
            Rule::fsm_transition_field => {
                let Some(transition) = current.as_mut() else {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.fsm_field_outside_transition",
                        "transition field must follow a transition id",
                        offset,
                        source,
                    );
                    continue;
                };
                let field = find_text(body.clone(), Rule::fsm_field_name).unwrap_or_default();
                let value = find_text(body.clone(), Rule::fsm_state_name)
                    .or_else(|| find_text(body, Rule::fsm_kebab))
                    .unwrap_or_default();
                let slot = match field.as_str() {
                    "from" => &mut transition.from,
                    "on" => &mut transition.on,
                    "to" => &mut transition.to,
                    "guard" => &mut transition.guard,
                    _ => unreachable!("grammar restricts fsm field names"),
                };
                if slot.is_some() {
                    push_diagnostic(
                        &mut diagnostics,
                        "pa.fsm_duplicate_field",
                        format!("{field} is repeated"),
                        offset,
                        source,
                    );
                } else {
                    *slot = Some(value);
                }
            }
            _ => {}
        }
    }
    flush_transition(&mut current, &mut transitions);

    let header = FsmHeader {
        version,
        kind,
        name,
        initial,
        seen_states_header,
        seen_transitions_header,
    };
    validate_document(source, &mut diagnostics, &header, &states, &transitions);
    if !diagnostics.is_empty() {
        return Err(Diagnostics { diagnostics });
    }
    Ok(FsmDocument {
        version: header.version.unwrap_or_default(),
        // validate_document 已保证 name / initial 存在。
        name: header.name.map(|(value, _)| value).unwrap_or_default(),
        initial: header.initial.map(|(value, _)| value).unwrap_or_default(),
        states,
        transitions,
        comments,
    })
}

/// 解析 FSM 文档并返回其规范表示。
pub fn format_source(source: &str) -> Result<String, Diagnostics> {
    let document = parse(source)?;
    Ok(format_document(&document))
}

/// 格式化已校验的 FSM 文档，保持状态与转移的声明顺序。
pub fn format_document(document: &FsmDocument) -> String {
    let mut lines = vec![
        format!("version: {}", document.version),
        "kind: fsm".to_owned(),
        format!("name: {}", document.name),
        format!("initial: {}", document.initial),
        String::new(),
    ];
    for comment in &document.comments {
        lines.push(comment.clone());
    }
    if !document.comments.is_empty() {
        lines.push(String::new());
    }
    lines.push("states:".to_owned());
    for fsm_state in &document.states {
        lines.push(format!(
            "  {}: {}",
            fsm_state.name,
            if fsm_state.terminal {
                "terminal"
            } else {
                "active"
            }
        ));
    }
    lines.push("transitions:".to_owned());
    for transition in &document.transitions {
        lines.push(format!("  {}:", transition.id));
        lines.push(format!("    from: {}", transition.from));
        lines.push(format!("    on: {}", transition.on));
        lines.push(format!("    to: {}", transition.to));
        if let Some(guard) = transition.guard.as_deref() {
            lines.push(format!("    guard: {guard}"));
        }
    }
    lines.push(String::new());
    lines.join("\n")
}

/// FSM 名称 / 事件 / guard 共用的 kebab → UpperCamel 转换。
pub fn upper_camel_case(kebab: &str) -> String {
    kebab
        .split('-')
        .map(|segment| {
            let mut characters = segment.chars();
            characters
                .next()
                .map(|first| first.to_ascii_uppercase())
                .into_iter()
                .chain(characters)
                .collect::<String>()
        })
        .collect()
}

/// FSM 名称 → 生成模块 / 输出文件名的 snake_case。
pub fn snake_case(kebab: &str) -> String {
    kebab.replace('-', "_")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FsmSection {
    Header,
    States,
    Transitions,
}

struct PartialTransition {
    id: String,
    offset: usize,
    from: Option<String>,
    on: Option<String>,
    to: Option<String>,
    guard: Option<String>,
}

fn flush_transition(current: &mut Option<PartialTransition>, transitions: &mut Vec<FsmTransition>) {
    let Some(transition) = current.take() else {
        return;
    };
    transitions.push(FsmTransition {
        id: transition.id,
        from: transition.from.unwrap_or_default(),
        on: transition.on.unwrap_or_default(),
        to: transition.to.unwrap_or_default(),
        guard: transition.guard,
        offset: transition.offset,
    });
}

/// parse 阶段收集的头部数据；`validate_document` 对其做静态校验。
struct FsmHeader {
    version: Option<u32>,
    kind: Option<String>,
    name: Option<(String, usize)>,
    initial: Option<(String, usize)>,
    seen_states_header: bool,
    seen_transitions_header: bool,
}

fn validate_document(
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
    header: &FsmHeader,
    states: &[FsmState],
    transitions: &[FsmTransition],
) {
    match header.version {
        None => push_diagnostic(
            diagnostics,
            "pa.missing_version",
            "version is required",
            0,
            source,
        ),
        Some(1) => {}
        Some(_) => push_diagnostic(
            diagnostics,
            "pa.unsupported_version",
            "only version 1 is supported",
            0,
            source,
        ),
    }
    match header.kind.as_deref() {
        None => {
            push_diagnostic(
                diagnostics,
                "pa.missing_kind",
                "kind is required",
                0,
                source,
            );
        }
        Some("fsm") => {}
        Some(_) => push_diagnostic(
            diagnostics,
            "pa.fsm_kind_mismatch",
            "fsm documents require kind: fsm",
            0,
            source,
        ),
    }
    if header.name.is_none() {
        push_diagnostic(
            diagnostics,
            "pa.fsm_missing_name",
            "name is required",
            0,
            source,
        );
    }
    if header.initial.is_none() {
        push_diagnostic(
            diagnostics,
            "pa.fsm_missing_initial",
            "initial is required",
            0,
            source,
        );
    }
    if !header.seen_states_header || states.is_empty() {
        push_diagnostic(
            diagnostics,
            "pa.fsm_missing_section",
            "states section with at least one state is required",
            0,
            source,
        );
    }
    if !header.seen_transitions_header {
        push_diagnostic(
            diagnostics,
            "pa.fsm_missing_section",
            "transitions section is required",
            0,
            source,
        );
    }

    let state_index: BTreeMap<&str, usize> = states
        .iter()
        .enumerate()
        .map(|(index, fsm_state)| (fsm_state.name.as_str(), index))
        .collect();
    for (index, fsm_state) in states.iter().enumerate() {
        if state_index
            .get(fsm_state.name.as_str())
            .is_some_and(|first| *first != index)
        {
            push_diagnostic(
                diagnostics,
                "pa.fsm_duplicate_state",
                format!("state '{}' is repeated", fsm_state.name),
                fsm_state.offset,
                source,
            );
        }
    }

    let mut seen_transitions: BTreeSet<&str> = BTreeSet::new();
    let mut edges: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for (index, transition) in transitions.iter().enumerate() {
        if !seen_transitions.insert(transition.id.as_str()) {
            push_diagnostic(
                diagnostics,
                "pa.fsm_duplicate_transition",
                format!("transition id '{}' is repeated", transition.id),
                transition.offset,
                source,
            );
        }
        for (value, field) in [
            (&transition.from, "from"),
            (&transition.on, "on"),
            (&transition.to, "to"),
        ] {
            if value.is_empty() {
                push_diagnostic(
                    diagnostics,
                    "pa.fsm_missing_field",
                    format!("transition '{}' requires {field}", transition.id),
                    transition.offset,
                    source,
                );
            }
        }
        if !transition.from.is_empty() && !state_index.contains_key(transition.from.as_str()) {
            push_diagnostic(
                diagnostics,
                "pa.fsm_unknown_state",
                format!(
                    "transition '{}' references unknown state '{}'",
                    transition.id, transition.from
                ),
                transition.offset,
                source,
            );
        }
        if !transition.to.is_empty() && !state_index.contains_key(transition.to.as_str()) {
            push_diagnostic(
                diagnostics,
                "pa.fsm_unknown_state",
                format!(
                    "transition '{}' references unknown state '{}'",
                    transition.id, transition.to
                ),
                transition.offset,
                source,
            );
        }
        if edges
            .insert((transition.from.as_str(), transition.on.as_str()), index)
            .is_some()
        {
            push_diagnostic(
                diagnostics,
                "pa.fsm_ambiguous_edge",
                format!(
                    "state '{}' already has an edge on '{}'",
                    transition.from, transition.on
                ),
                transition.offset,
                source,
            );
        }
    }

    if let Some((initial_name, initial_offset)) = header.initial.as_ref() {
        match state_index.get(initial_name.as_str()) {
            None => push_diagnostic(
                diagnostics,
                "pa.fsm_unknown_state",
                format!("initial references unknown state '{initial_name}'"),
                *initial_offset,
                source,
            ),
            Some(&index) if states[index].terminal => {
                push_diagnostic(
                    diagnostics,
                    "pa.fsm_invalid_initial",
                    format!("initial state '{initial_name}' must be active"),
                    *initial_offset,
                    source,
                );
            }
            Some(_) => {}
        }
    }

    // 终态出边；同时构建忽略 guard 的邻接表供可达性检查使用。
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); states.len()];
    for transition in transitions {
        let Some(&from) = state_index.get(transition.from.as_str()) else {
            continue;
        };
        let Some(&to) = state_index.get(transition.to.as_str()) else {
            continue;
        };
        if states[from].terminal {
            push_diagnostic(
                diagnostics,
                "pa.fsm_terminal_outgoing",
                format!(
                    "terminal state '{}' must not have outgoing edges",
                    states[from].name
                ),
                transition.offset,
                source,
            );
            continue;
        }
        adjacency[from].push(to);
    }

    if states.is_empty() || state_index.len() != states.len() {
        // 名称冲突已单独报告；图检查只在状态名可索引时有意义。
        return;
    }
    let Some(&initial_index) = header
        .initial
        .as_ref()
        .and_then(|(initial_name, _)| state_index.get(initial_name.as_str()))
    else {
        return;
    };

    let reachable = reachable_from(initial_index, states.len(), |index| {
        adjacency[index].clone()
    });
    for fsm_state in states {
        if !reachable[state_index[fsm_state.name.as_str()]] {
            push_diagnostic(
                diagnostics,
                "pa.fsm_unreachable_state",
                format!("state '{}' is not reachable from initial", fsm_state.name),
                fsm_state.offset,
                source,
            );
        }
    }
    let reaching_terminal = terminal_closure(states, &adjacency);
    for fsm_state in states {
        if !fsm_state.terminal && !reaching_terminal[state_index[fsm_state.name.as_str()]] {
            push_diagnostic(
                diagnostics,
                "pa.fsm_no_terminal_path",
                format!(
                    "state '{}' has no structural path to a terminal state",
                    fsm_state.name
                ),
                fsm_state.offset,
                source,
            );
        }
        if !fsm_state.terminal && adjacency[state_index[fsm_state.name.as_str()]].is_empty() {
            push_diagnostic(
                diagnostics,
                "pa.fsm_dead_state",
                format!(
                    "active state '{}' requires an outgoing edge",
                    fsm_state.name
                ),
                fsm_state.offset,
                source,
            );
        }
    }

    validate_event_and_guard_names(source, diagnostics, transitions);
}

fn validate_event_and_guard_names(
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
    transitions: &[FsmTransition],
) {
    // 事件与 guard 生成两个独立枚举：同名变体不构成冲突，按种类分表检查。
    let mut seen_events: BTreeMap<String, String> = BTreeMap::new();
    let mut seen_guards: BTreeMap<String, String> = BTreeMap::new();
    for transition in transitions {
        for (value, label, seen) in [
            (transition.on.as_str(), "event", &mut seen_events),
            (
                transition.guard.as_deref().unwrap_or_default(),
                "guard",
                &mut seen_guards,
            ),
        ] {
            if value.is_empty() {
                continue;
            }
            if !is_canonical_kebab(value) {
                push_diagnostic(
                    diagnostics,
                    "pa.fsm_invalid_name",
                    format!("{label} '{value}' must use canonical kebab-case"),
                    transition.offset,
                    source,
                );
                continue;
            }
            let converted = upper_camel_case(value);
            if is_rust_keyword(&converted) {
                push_diagnostic(
                    diagnostics,
                    "pa.fsm_reserved_name",
                    format!("{label} '{value}' converts to the Rust keyword '{converted}'"),
                    transition.offset,
                    source,
                );
                continue;
            }
            if let Some(previous) = seen.insert(converted.clone(), value.to_owned())
                && previous != value
            {
                push_diagnostic(
                    diagnostics,
                    "pa.fsm_name_collision",
                    format!("{label} '{value}' and '{previous}' both convert to '{converted}'"),
                    transition.offset,
                    source,
                );
            }
        }
    }
}

fn terminal_closure(states: &[FsmState], adjacency: &[Vec<usize>]) -> Vec<bool> {
    let mut reaching = vec![false; states.len()];
    let mut stack: Vec<usize> = states
        .iter()
        .enumerate()
        .filter(|(_, fsm_state)| fsm_state.terminal)
        .map(|(index, _)| index)
        .collect();
    for &index in &stack {
        reaching[index] = true;
    }
    while let Some(index) = stack.pop() {
        // 反向传播：所有能到达 index 的前驱同样能到达终态。
        for (candidate, edges) in adjacency.iter().enumerate() {
            if edges.contains(&index) && !reaching[candidate] {
                reaching[candidate] = true;
                stack.push(candidate);
            }
        }
    }
    reaching
}

/// 从 `start` 出发沿邻接表可达的状态集合；遍历次序不影响结果。
fn reachable_from(start: usize, total: usize, edges_of: impl Fn(usize) -> Vec<usize>) -> Vec<bool> {
    let mut seen = vec![false; total];
    let mut queue = vec![start];
    seen[start] = true;
    while let Some(index) = queue.pop() {
        for next in edges_of(index) {
            if !seen[next] {
                seen[next] = true;
                queue.push(next);
            }
        }
    }
    seen
}

fn is_canonical_kebab(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        && value
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase())
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "become", "box", "break", "const", "continue", "crate", "do", "dyn",
    "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let",
    "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "try", "type", "typeof", "union",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

fn is_rust_keyword(value: &str) -> bool {
    RUST_KEYWORDS.contains(&value)
}

fn find_text(body: pest::iterators::Pair<'_, Rule>, rule: Rule) -> Option<String> {
    if body.as_rule() == rule {
        return Some(body.as_str().to_owned());
    }
    body.into_inner().find_map(|pair| find_text(pair, rule))
}
