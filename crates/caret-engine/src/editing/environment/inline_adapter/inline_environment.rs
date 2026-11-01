//! InlineEnvironment 载体到统一 Inline 语义的映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::{EnvironmentKind, EnvironmentSource};
use super::InlineAdapter;
use caret_doc::InlineEnvironmentKind;

pub(crate) const fn for_inline_environment(kind: InlineEnvironmentKind) -> InlineAdapter {
    InlineAdapter {
        source: EnvironmentSource::InlineEnvironment,
        precedence: precedence(match kind {
            InlineEnvironmentKind::Bold => EnvironmentKind::Bold,
            InlineEnvironmentKind::Italic => EnvironmentKind::Italic,
            InlineEnvironmentKind::Font => EnvironmentKind::Font,
        }),
        kind: match kind {
            InlineEnvironmentKind::Bold => EnvironmentKind::Bold,
            InlineEnvironmentKind::Italic => EnvironmentKind::Italic,
            InlineEnvironmentKind::Font => EnvironmentKind::Font,
        },
    }
}

pub(super) const fn precedence(kind: EnvironmentKind) -> u8 {
    match kind {
        EnvironmentKind::Bold => 0,
        EnvironmentKind::Italic => 1,
        EnvironmentKind::Font => 2,
        _ => u8::MAX,
    }
}
