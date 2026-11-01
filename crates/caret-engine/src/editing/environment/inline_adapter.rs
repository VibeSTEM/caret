//! 将不同 Inline 文档载体适配为统一的交互语义。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod inline_environment;
mod inline_math;
mod run_property;

use super::{EnvironmentKind, EnvironmentSource};

pub(crate) use inline_environment::for_inline_environment;
pub(crate) use inline_math::INLINE_MATH_ADAPTER;
pub(crate) use run_property::{
    RUN_PROPERTY_ENVIRONMENTS, RunPropertyEnvironment, run_property_for_key, run_property_for_kind,
};

/// 统一适配后的行内语义来源与 kind。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InlineAdapter {
    pub(crate) source: EnvironmentSource,
    pub(crate) kind: EnvironmentKind,
    pub(crate) precedence: u8,
}

pub(crate) fn precedence_for(source: EnvironmentSource, kind: EnvironmentKind) -> u8 {
    match source {
        EnvironmentSource::InlineEnvironment => inline_environment::precedence(kind),
        EnvironmentSource::RunProperty => run_property_for_kind(kind)
            .map(|descriptor| descriptor.adapter.precedence)
            .unwrap_or(u8::MAX),
        EnvironmentSource::InlineMath => inline_math::precedence(kind),
        _ => u8::MAX,
    }
}
