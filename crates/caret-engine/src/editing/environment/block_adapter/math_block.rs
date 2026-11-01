//! 显示公式块载体到统一 Block 语义的映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::BlockAdapter;
use crate::editing::{EnvironmentKind, EnvironmentSource};

pub(crate) const fn for_math_block(align: bool) -> BlockAdapter {
    BlockAdapter {
        source: EnvironmentSource::MathBlock,
        kind: if align {
            EnvironmentKind::Align
        } else {
            EnvironmentKind::Equation
        },
    }
}
