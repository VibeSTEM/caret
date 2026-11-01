//! InlineMath 作为原子行内载体的语义映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::InlineAdapter;
use crate::editing::{EnvironmentKind, EnvironmentSource};

pub(crate) const INLINE_MATH_ADAPTER: InlineAdapter = InlineAdapter {
    source: EnvironmentSource::InlineMath,
    kind: EnvironmentKind::InlineMath,
    precedence: 4,
};

pub(super) const fn precedence(kind: EnvironmentKind) -> u8 {
    match kind {
        EnvironmentKind::InlineMath => 4,
        _ => u8::MAX,
    }
}
