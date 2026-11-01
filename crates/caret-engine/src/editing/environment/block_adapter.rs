//! 将普通块环境和数学块适配为统一的 Block 交互语义。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod block_environment;
mod math_block;

use super::{EnvironmentKind, EnvironmentSource};

pub(crate) use block_environment::for_block_environment;
pub(crate) use math_block::for_math_block;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlockAdapter {
    pub(crate) source: EnvironmentSource,
    pub(crate) kind: EnvironmentKind,
}
