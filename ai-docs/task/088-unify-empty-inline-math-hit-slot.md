# 088 — 收敛空 InlineMath 插入槽与正文命中优先级

- 状态：planned
- 依赖：087、069
- 优先级：P2
- 创建 / 更新：2026-11-12 / 2026-11-12

## 目标与背景

[087](087-empty-inline-math-advance-and-highlight.md) 为“空 InlineMath 不进入正文推进、聚焦高亮用插入槽、仍可点击进入且不吞相邻文字”实现了三条规则，但其中两条在 engine 与 app 各有一份实现：

- 插入槽几何：engine `hit_test_math_structure`（`crates/caret-engine/src/layout.rs`）与 app `insertion_slot_rect_from_caret`（`crates/caret-app/src/document_view/snapshot.rs`）各自按 `max(0.5em, 4pt)`、caret 居中计算（app 额外 clamp 页面）。
- 正文优先判定：engine `point_inside_body_text`（`layout.rs`）与 app `body_text_contains_point`（`document_view/hit_test.rs`）逐字重复 1.5pt/25% 内缩规则。

同一策略两处持有容易漂移；087 的增量评审建议合并后单独开任务下沉（[083](../task/083-cross-host-selection-and-transactions.md) 复查意见同）。

## 必读

- [数学公式](../architecture/editor/math.md)、[画布与交互背景](../architecture/interface/canvas-highlighting.md)
- [分层](../standards/layering.md)、[数值](../standards/numeric-values.md)、[验证与评审](../standards/validation-and-review.md)
- [087](087-empty-inline-math-advance-and-highlight.md)

## 范围与非目标

- 把插入槽宽高规则与“正文优先/字素内缩”判定收敛到 engine 单一权威 helper，app 与 engine 命中共享同一实现或通过快照字段消费，避免两份常量与内缩算法。
- 保持 087 的可观察契约：空公式 slot 中心可进入、相邻文字内部点击落正文、非空公式与 Equation/Align 不受影响。
- 非目标：不改变视觉槽宽度、不改空公式删除/IME/Undo 文本模型、不重开 087 的实现。

## 前置条件与待决策

- 需确认 helper 归属：命中入口在 engine（`layout.rs`），app 高亮投影只消费几何；若 helper 需同时被 app 高亮使用，可考虑由 engine 快照提供 slot rect，而不是 app 重算。
- 1.5pt/25% 内缩常量是否应进入共享数值规范或保持局部常量，需按[数值规范](../standards/numeric-values.md)核对。

## 实施步骤

1. 定位两处重复实现及全部调用点，记录行为差异（页面 clamp、字号来源、亲和性）。
2. 抽取 engine 侧单一 helper（或快照字段），app 改为消费；保留页面 clamp 在发布边界。
3. 用 087 的命中/高亮回归与新增跨 crate 一致性测试验证行为不变；确认无 Qt 侧改动需求。
4. 按验证标准做独立评审。

## 预计改动

`crates/caret-engine/src/layout.rs`、`crates/caret-engine/src/layout/tests.rs`、`crates/caret-app/src/document_view/snapshot.rs`、`crates/caret-app/src/document_view/hit_test.rs`（可能删除重复 helper）、app 相关测试。

## 验收标准

- [ ] 插入槽几何与正文优先判定各自只有一个实现权威，app/engine 不再持有重复常量或算法。
- [ ] 087 的空公式点击、邻字点击、粉色槽宽度/居中与页面 clamp 行为不变，有回归。
- [ ] 格式、lint、相关 crate 测试与独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --locked -p caret-engine -p caret-app` | 命中/高亮行为不变 | 未执行 |

## 风险与回退

若 helper 下沉导致快照接口扩张，优先保留 app 侧消费只读字段；行为变化时回退到 087 的两份实现并记录例外。

## 决策与工作记录

- 2026-11-12：PR #136 复查建议合并后单独任务下沉，087 任务卡同步记录。
