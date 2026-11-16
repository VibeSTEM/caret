# 100 — 测试确定性与诊断基础设施

- 状态：planned
- 依赖：093
- 优先级：P2
- 创建 / 更新：2026-11-17 / 2026-11-17

## 目标与背景

Brooks sweep 在测试维度留下的残余：
- `crates/caret-engine/src/layout/tests.rs` 有 49 处 `FontCatalog::system()`，塑形依赖宿主已安装字体，CI/开发机字体差异可能导致行为不一致；
- `crates/caret-engine/src/editing/session/math_edit/tests.rs` 约 15,000 行、300+ 测试无模块分区；
- 测试中大量 `map_err(|_| EditError::InvalidPosition)` / `ok_or(...)` 把真实错误替换为通用错误（math_edit/tests.rs 约 167 + 442 处），失败时无法定位原因；
- `LayoutViewModelTestProbe`（friend struct）直接读私有成员（含 autosave timer 的停止）、`paragraphTextBoundaries` 等测试专用包装；
- 恢复安装失败（`installSnapshot`）路径不刷新恢复列表（评审发现的既有缺口）。

完成后：布局/字体相关测试使用确定性的内嵌字体夹具或显式声明系统依赖；`math_edit/tests.rs` 按职责分区（`mod`/子文件）；测试错误保留原始诊断；测试探针收敛为公开可观察 API 或最小化。

## 必读

[测试规范](../standards/testing.md)、[Rust 断言风格](../standards/rust.md)、[验证与评审](../standards/validation-and-review.md)。

## 范围与非目标

范围内：测试夹具与结构；不改生产行为（恢复刷新缺口需单独确认是否属既有契约）。非目标：提高覆盖率数字；不为覆盖率增加无行为意义的测试。

## 前置条件与待决策

确认测试字体夹具策略（仓库已有 `resources/fonts/` 静态字体，可复用）；`LayoutViewModelTestProbe` 的目标验证点是否可由公开属性/信号替代需逐项核对。

## 实施步骤

1. 列出 `FontCatalog::system()` 的 49 处，区分"必须系统字体"与"可用夹具"；后者迁移到内嵌字体并记录选择理由。
2. `math_edit/tests.rs` 按 `mod` 分区（源码提交、树操作、选择/光标、符号输入、IME），用 `#[path]` 或子文件组织，保持测试名与数量不变。
3. 测试错误诊断：改 `map_err` 为保留原错误（如 `EditError::InvalidPosition` 仅在确需归一化处保留，并附来源信息）。
4. 探针与其消费者逐项迁移到公开 API；不可替代的保留并注释理由。
5. 全量门禁 + 独立评审。

## 预计改动

`crates/caret-engine/src/layout/tests.rs`、`editing/session/math_edit/tests.rs`（拆分为子文件）、`crates/caret-app/src/document_view/tests/*`、`native/viewmodels/document/layout_view_model.hpp` 及其测试。无生产行为变化。

## 验收标准

- [ ] 测试字体不再隐式依赖宿主安装字体（或逐项记录不可改理由）。
- [ ] `math_edit/tests.rs` 有清晰分区；测试名/数量/断言不变。
- [ ] 测试失败信息能直接指向真实错误。
- [ ] 全量门禁与独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --workspace --locked` | 全绿 | 未执行 |

## 风险与回退

字体夹具迁移可能改变断言的几何期望值；逐文件迁移并对比旧值，差异需证明等价。测试文件拆分只搬移不改断言。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已完成 eager 测试拆分与重复测试合并，本任务承接剩余测试基础设施。

## 完成摘要

未完成。
