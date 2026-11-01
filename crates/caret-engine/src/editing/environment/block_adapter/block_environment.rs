//! 普通 BlockEnvironment 载体到统一 Block 语义的映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::BlockAdapter;
use crate::editing::{EnvironmentKind, EnvironmentSource};
use caret_doc::BlockEnvironmentKind;

pub(crate) const fn for_block_environment(kind: BlockEnvironmentKind) -> BlockAdapter {
    BlockAdapter {
        source: EnvironmentSource::BlockEnvironment,
        kind: match kind {
            BlockEnvironmentKind::Section => EnvironmentKind::Section,
            BlockEnvironmentKind::Custom => EnvironmentKind::Custom,
        },
    }
}
