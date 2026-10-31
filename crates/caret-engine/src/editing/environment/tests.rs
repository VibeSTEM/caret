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
    assert!(EnvironmentKind::Equation.capabilities().atomic);
}

#[test]
fn innermost_unwrappable_skips_atomic_math() {
    let entry = |kind| CurrentEnvironment {
        id: NodeId::generate(),
        kind,
        inline_range: None,
    };
    let section = entry(EnvironmentKind::Section);
    let stack =
        CurrentEnvironmentStack::new(vec![section.clone(), entry(EnvironmentKind::InlineMath)]);
    assert_eq!(
        stack.innermost().map(|entry| entry.kind),
        Some(EnvironmentKind::InlineMath)
    );
    assert_eq!(stack.innermost_unwrappable(), Some(&section));
}
