//! 环境种类、能力与环境栈查询的单元测试。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

#[test]
fn kinds_declare_level_and_capabilities() {
    for kind in [
        EnvironmentKind::Bold,
        EnvironmentKind::Italic,
        EnvironmentKind::Font,
        EnvironmentKind::InlineMath,
    ] {
        assert_eq!(kind.level(), EnvironmentLevel::Inline);
    }
    for kind in [
        EnvironmentKind::Section,
        EnvironmentKind::Custom,
        EnvironmentKind::Equation,
        EnvironmentKind::Align,
    ] {
        assert_eq!(kind.level(), EnvironmentLevel::Block);
    }
    assert!(
        EnvironmentKind::Bold
            .capabilities()
            .unwrap_preserving_content
    );
    assert!(
        !EnvironmentKind::InlineMath
            .capabilities()
            .unwrap_preserving_content
    );
    assert!(EnvironmentKind::Equation.capabilities().atomic_in_host);
}

#[test]
fn innermost_unwrappable_skips_atomic_math() {
    let entry = |kind, source| EnvironmentEntry {
        id: Some(NodeId::generate()),
        kind,
        source,
        inline_range: None,
        depth: 0,
    };
    let bold = entry(EnvironmentKind::Bold, EnvironmentSource::InlineEnvironment);
    let view = CurrentEnvironmentView::new(
        vec![
            bold.clone(),
            entry(EnvironmentKind::InlineMath, EnvironmentSource::InlineMath),
        ],
        vec![entry(
            EnvironmentKind::Section,
            EnvironmentSource::BlockEnvironment,
        )],
    );
    assert_eq!(
        view.innermost_inline().map(|entry| entry.kind),
        Some(EnvironmentKind::InlineMath)
    );
    assert_eq!(view.innermost_unwrappable_inline(), Some(&bold));
    assert_eq!(
        view.innermost_block().map(|entry| entry.kind),
        Some(EnvironmentKind::Section)
    );
}

#[test]
fn carrier_adapters_have_one_mapping_per_inline_and_block_source() {
    use crate::editing::environment::{block_adapter, inline_adapter};
    use caret_doc::{BlockEnvironmentKind, InlineEnvironmentKind};

    assert_eq!(
        inline_adapter::for_inline_environment(InlineEnvironmentKind::Bold),
        inline_adapter::InlineAdapter {
            source: EnvironmentSource::InlineEnvironment,
            kind: EnvironmentKind::Bold,
            precedence: 0,
        }
    );
    assert_eq!(
        inline_adapter::INLINE_MATH_ADAPTER.source,
        EnvironmentSource::InlineMath
    );
    assert_eq!(inline_adapter::RUN_PROPERTY_ENVIRONMENTS.len(), 1);
    let underline = inline_adapter::run_property_for_kind(EnvironmentKind::Underline)
        .unwrap_or_else(|| panic!("Run property adapter missing"));
    assert_eq!(underline.key, "underline");
    assert_eq!(underline.adapter.source, EnvironmentSource::RunProperty);
    assert_eq!(
        inline_adapter::precedence_for(EnvironmentSource::RunProperty, underline.adapter.kind),
        underline.adapter.precedence
    );
    assert_eq!(
        inline_adapter::precedence_for(EnvironmentSource::RunProperty, EnvironmentKind::Bold),
        u8::MAX
    );
    assert_eq!(
        block_adapter::for_block_environment(BlockEnvironmentKind::Section),
        block_adapter::BlockAdapter {
            source: EnvironmentSource::BlockEnvironment,
            kind: EnvironmentKind::Section,
        }
    );
    assert_eq!(
        block_adapter::for_math_block(true),
        block_adapter::BlockAdapter {
            source: EnvironmentSource::MathBlock,
            kind: EnvironmentKind::Align,
        }
    );
}
