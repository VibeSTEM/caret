# 088 — 收敛空 InlineMath 插入槽与正文命中优先级

- 状态：implemented
- 依赖：087、069
- 优先级：P2
- 创建 / 更新：2026-11-12 / 2026-11-12

## 目标与背景

[087](087-empty-inline-math-advance-and-highlight.md) 规定空 InlineMath 不参与正文推进，焦点高亮使用插入槽，槽内仍可点击进入且相邻正文优先。

- 任务开始时，engine 命中以公式根盒计算槽宽，app 高亮以树光标与有效字号算宽并另外裁切到页面。空公式靠近页边时，可见槽与实际命中范围不同。
- 正文优先已收敛到 engine `body_text_contains_point`，供公式结构与 app 的零宽数学文本命中复用。

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
2. 由 engine 投影页面和单元格内的可见槽。app 消费矩形，公式命中复用同一投影。
3. 用 087 的命中/高亮回归与新增跨 crate 一致性测试验证行为不变；确认无 Qt 侧改动需求。
4. 按验证标准做独立评审。

## 预计改动

`crates/caret-engine/src/layout.rs`、`crates/caret-engine/src/layout/tests.rs`、`crates/caret-app/src/document_view/snapshot.rs`、`crates/caret-app/src/document_view/hit_test.rs`（可能删除重复 helper）、app 相关测试。

## 验收标准

- [x] 插入槽几何与正文优先判定各自只有一个实现权威，app/engine 不再持有重复常量或算法。
- [x] 087 的空公式点击、邻字点击、粉色槽宽度/居中与页面 clamp 行为有回归。窄表中的命中与高亮一起裁到单元格内容区。
- [x] 格式、相关 crate 的 Clippy 与行为测试、独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 本轮 | `cargo test --locked --offline -p caret-app empty_inline_` | 空槽两端、邻字优先及待输入格式槽 | 10 项通过。空流与单个 Placeholder 根流均覆盖 0.9pt、12pt 页边命中。72pt 版心内，96pt 公式槽和待输入格式槽留在各自单元格，右格点击回到右格段落。 |
| 本轮 | `cargo test --locked --offline -p caret-engine -p caret-app` | 布局、公式命中、应用投影及其直接回归 | engine 1000 项、app 382 项通过。 |
| 本轮 | Qt `emptyInlineMathFocusRendersInsertionSlotAcrossZoom` | 打开真实文档、点入公式并观察粉色槽 | 离屏与 macOS Cocoa 窗口通过，zoom=1/2 抓帧像素与槽宽、居中断言通过。 |
| 本轮 | `cargo coverage` | Rust 产品函数及行覆盖率均至少 85% | 函数 86.16%，行 89.07%，两项通过。 |
| 本轮 | `cargo format --check`、`cargo clippy --locked --offline -p caret-engine -p caret-app --all-targets -- -D warnings` | 格式与相关 crate 静态检查 | 通过。 |
| 本轮 | 分领域检查及独立终审 | 字号与命中来源一致、非空公式保持原路径、格内裁切阻止越格 | 终审发现窄表中槽位越格，修复后由同一回归和独立复核确认关闭。 |

## 风险与回退

若 helper 下沉导致快照接口扩张，优先保留 app 侧消费只读字段；行为变化时回退到 087 的两份实现并记录例外。

## 决策与工作记录

- 2026-11-12：PR #136 复查建议合并后单独任务下沉，087 任务卡同步记录。
- 共享底座复查确认正文优先已由 engine `body_text_contains_point` 供两个层复用。本轮收敛剩余的插入槽投影并核对本地行为，跨平台窗口验收沿用 069 的验证项。
- 拿到实际树光标与有效字号后，命中和高亮共同计算槽位。槽在纸张或单元格内容边缘整体内移，宽度超过可见区域时以该区域为上限。普通空格式、待输入格式和公式 Source 分支共用同一槽位计算，从正文布局取得位置与裁切。

## 完成摘要

空公式点击与焦点高亮共用 engine 解析的实际树光标、字号和页面可见槽。普通空格式复用同一套槽位投影，正文优先与垂直结构命中容差保持。跨平台窗口验收由 [069](069-math-interaction-and-platform-acceptance.md) 跟进，本轮 PR CI 待执行。
