//! 声明结构索引可组合使用的最小语义原子。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod block_environment;
mod paragraph;

pub(crate) use block_environment::BlockEnvironmentAtom;
pub(crate) use paragraph::ParagraphAtom;
