//! Run.props 行内语义的属性描述与查找。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::{EnvironmentKind, EnvironmentSource};
use super::InlineAdapter;
use serde_json::Value;

/// 可作为 Inline 候选的 Run 属性。属性键只在此表登记，投影和编辑逻辑按表驱动。
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunPropertyEnvironment {
    pub(crate) key: &'static str,
    pub(crate) adapter: InlineAdapter,
    pub(crate) is_active: fn(Option<&Value>) -> bool,
    pub(crate) value_for_state: Option<fn(bool) -> Value>,
}

fn boolean_is_active(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}

fn boolean_value(enabled: bool) -> Value {
    Value::Bool(enabled)
}

pub(crate) const RUN_PROPERTY_ENVIRONMENTS: &[RunPropertyEnvironment] = &[RunPropertyEnvironment {
    key: "underline",
    adapter: InlineAdapter {
        source: EnvironmentSource::RunProperty,
        kind: EnvironmentKind::Underline,
        precedence: 3,
    },
    is_active: boolean_is_active,
    value_for_state: Some(boolean_value),
}];

pub(crate) fn run_property_for_kind(
    kind: EnvironmentKind,
) -> Option<&'static RunPropertyEnvironment> {
    RUN_PROPERTY_ENVIRONMENTS
        .iter()
        .find(|descriptor| descriptor.adapter.kind == kind)
}

pub(crate) fn run_property_for_key(key: &str) -> Option<&'static RunPropertyEnvironment> {
    RUN_PROPERTY_ENVIRONMENTS
        .iter()
        .find(|descriptor| descriptor.key == key)
}
