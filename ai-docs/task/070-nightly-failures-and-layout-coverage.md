# 070 — Nightly 失败修复与布局测试补齐

- 状态：implemented
- 依赖：006
- 优先级：P1
- 创建 / 更新：2026-11-08 / 2026-11-08

## 目标与背景

修复 [2026-11-07 Nightly diagnostics](https://github.com/VibeSTEM/caret/actions/runs/42376387135) 的可复现失败，并补足 inline object 布局测试对占位符字体差异的覆盖。该运行中 Rust/native 覆盖率门禁通过；Miri 因新增长文本命中测试加载系统字体而失败，Windows workspace 测试因 inline object advance 断言将替换字符的字体宽度纳入固定差值而失败。

## 必读

- [验证与评审](../standards/validation-and-review.md)
- [测试规范](../standards/testing.md)
- [CI 规范](../standards/ci.md)

## 范围与非目标

- 修正 Miri 对不能在解释器中运行的 `document_view::hit_test` 字体访问测试的精确排除，并保留其 workspace 行为测试。
- 将 inline object advance 回归断言改为对比同一段落中的零宽对象与定宽对象，验证用户可见的推进宽度与对象宽度一致。
- 增加相应回归验证并记录覆盖范围。
- 不调整 Rust/native 85% 覆盖率门槛或排除范围，不扩大平台矩阵。

## 前置条件与待决策

- 指定 Actions 运行日志已读取，失败项为 Ubuntu Miri 与 Windows `caret-layout` 单测。
- 通过可用的本地验证确认 Miri skip 选择及 inline object advance 语义。

## 实施步骤

1. 针对 Miri `dlopen` 错误跳过仅在 Miri 下不兼容的命中测试用例。
2. 增加 / 调整 inline object advance 行为断言，避免系统字体、连字或字距差异影响期望值。
3. 运行最小 Rust 测试与格式检查；可用时运行 Miri 或对应验证。
4. 完成领域检查与最终只读评审，更新验证记录。

## 预计改动

- `tests/src/miri.rs`
- `crates/caret-layout/src/tests.rs`
- 本任务文档和 `ai-docs/task-index.md`

## 验收标准

- [x] Miri 不再尝试从目标命中测试加载 Fontconfig；该行为仍由完整 workspace 测试验证。
- [x] advance 回归测试断言 inline object 占用的 30 pt 推进量，与平台字体塑形结果无关。
- [x] Rust、覆盖率门禁范围和阈值没有被缩减。
- [x] 相关测试、评审及未执行验证均有事实记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-08 | `cargo test --locked -p caret-layout --lib inline_object_uses_parley_advance_and_expands_the_shared_line_box` | advance 回归用例通过 | 通过（1 passed） |
| 2026-11-08 | `cargo fmt --all --check` | Rust 格式通过 | 通过 |
| 2026-11-08 | `cargo ub-check` | Miri 用例通过 | 通过（macOS arm64：caret-app 83 passed、1 ignored；DSL 4 组通过） |
| 2026-11-07 | 指定 Nightly run 的 Rust/native coverage jobs | Rust 与 native 门禁保持通过 | 两个覆盖率 job 均成功；本次未重新运行覆盖率报告 |
| — | Windows workspace tests / 修复后远程 Nightly | 跨平台确认 advance 修复 | 未执行；需要推送后由 Actions 复验 |

## 风险与回退

回归断言必须继续验证对象 advance 对段落总推进量的影响；不得仅检查 Parley 中存在 inline box。若 Miri 的模块名匹配不符，应调整过滤模式并核对列出的执行测试。

## 决策与工作记录

- 2026-11-08：创建任务；日志确认两项失败，覆盖率门禁通过。
- 2026-11-08：Miri 过滤精确到失败测试名；布局用例使用同文本零宽对象作为基线，避免字体塑形差异。Terra/medium 领域检查与 Sol/medium 最终只读评审均未留下未处理发现；最终评审未复验 Windows 或远程 Nightly。

## 完成摘要

本地修复和验证完成。Rust/native 覆盖率门禁范围未变，指定 Nightly 中两项 coverage job 均通过。修复后的 Windows runner 和远程 Nightly 尚未执行，保留为外部验收项。
