# 095 — caret-engine 会话与数学编辑模块拆分

- 状态：done
- 依赖：093；建议先完成 083 相关回归
- 优先级：P1
- 创建 / 更新：2026-11-17 / 2026-11-18

## 目标与背景

`crates/caret-engine/src/editing/session/math_edit.rs` 6464 行仍在入口文件承担 6 类职责（命令分派、进入/退出与块邻接导航、树编辑、选择与光标、源码写入事务、符号输入），其中 `write_math_source_with_metadata` 约 646 行仍含字节级重复的 validate/bind 与三份 record 序列；`session.rs` 1899 行中 `run_inner` 447 行混合命令路由、13 个 before_* 快照、40+ 变体分派与历史提交，`Session` 23 个字段被 5 个模块直接写（`command_line` 14 处、`host_caret_position` 24 处、`math_edit` 11 处），6 个月 29 次提交为全仓最高。

完成后：按模块职责拆为 `math_edit.rs` 入口 + 子模块（源码提交、进入/退出、树操作、符号输入），`run_inner` 拆为命令族分派 + 统一提交；各子状态有唯一设置/清除方法，直接字段写入收敛到方法内。

## 必读

[仓库文件结构](../standards/repository-hygiene.md#模块文件结构)、[分层](../standards/layering/rust.md)、[验证与评审](../standards/validation-and-review.md)、[代码生命周期](../standards/code-lifecycle.md)。

## 范围与非目标

范围内：纯职责拆分的文件移动与 `pub(super)` 边界调整；唯一 commit/clear 方法；不改变可观察编辑行为。非目标：新增编辑能力、改变撤销合并策略或 public API。

## 前置条件与待决策

先补关键路径特征化测试（撤销/IME 原子提交/数学文本分支）。`session/tests.rs` 2400 行、`math_edit/tests.rs` 15000 行已有覆盖，拆分前确认失败定位能力。

## 实施步骤

1. 在干净基线上跑 `cargo test -p caret-engine` 记录基线。
2. 先把剩余手写事务点（`math_edit.rs` 4604/5404、`text_inline_environment.rs` 222/261、`text_run_property.rs` 159）与未收敛的 3 处 `MathEdit` 构造点（`inline_content.rs`）接入既有入口。
3. 按职责搬移 `math_edit.rs` 的源码提交/进入退出/树操作/符号输入到子模块，入口只留类型与分派。
4. 拆 `run_inner`：命令族分派函数 + 统一提交调用；直接字段写入改为 `clear_*` 方法。
5. 每步后跑 `cargo clippy -p caret-engine --all-targets -- -D warnings` 与 `cargo test -p caret-engine`；完成后跑全量 workspace 门禁并独立评审。

## 预计改动

`crates/caret-engine/src/editing/session.rs`、`session/math_edit.rs` 及 `math_edit/*`、`session/composition.rs`、`session/structure.rs`、`session/formatting.rs`、`session/selection.rs`、`session/inline_content.rs`。无 public API 变化。

## 验收标准

- [x] `math_edit.rs` 从 6440 行收敛到约 1600 行；源码提交、进入/退出、树操作、符号输入、数学文本投影各有独立模块。
- [x] `run_inner` 保留前置命令族路由；普通命令由 `run_document_command` 采集快照并统一提交，`dispatch_edit_command` 负责领域命令分派。
- [x] `Session` 的 composition、command_line、math_edit、host_caret_position 仅通过 `set_*`、`clear_*`、`take_*` 入口修改（测试代码除外）。
- [x] 全量门禁通过；独立分领域检查和最终只读评审通过，范围与限制已记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-18 | macOS；`cargo test --locked -p caret-engine`（拆分前） | 固定基线 | 876 passed，0 failed |
| 2026-11-18 | macOS；`cargo test --locked -p caret-engine`（最终实现） | 与拆分前同数通过 | 876 passed，0 failed |
| 2026-11-18 | macOS；`cargo clippy --locked -p caret-engine --all-targets -- -D warnings` | 无警告 | 通过 |
| 2026-11-18 | macOS；`cargo test --workspace --locked` | workspace 与 native CTest 全绿 | 通过；CTest 20/20 |
| 2026-11-18 | macOS；`cargo lint --check` | 9 阶段全绿 | 9/9 阶段通过 |
| 2026-11-18 | macOS；`cargo format --check` | 格式一致 | 通过 |

## 风险与回退

撤销/IME 语义对顺序敏感；只做搬移与等价重构，不顺手改行为。每步独立 commit，失败按提交回退。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已清理 `MathTextEdit` 死代码并统一 `commit_document_edit`（40 个调用点中的 5 个），本任务承接剩余拆分。
- 2026-11-18：按职责把源码提交、进入/退出与命令分派、数学树命令、符号输入和选区/投影逻辑拆入 session 子模块；保留现有测试，不改变编辑、撤销、PendingStyle 状态 ID 或文档代际语义。
- 2026-11-18：历史提交共享 `commit_document_edit_with_states`，保留显式提交后快照，并由调用方传入是否推进文档代际；`Session` 四类子状态通过唯一的设置、清除和取出方法修改。`inline_content.rs` 的两个内联 `MathEdit` 字面量改用既有构造器；task 创建时预计的第三处在当前基线上不存在。
- 2026-11-18：分领域评审修复了 `run_math_edit` 注释错位和 composition/math_edit 直接 `.take()` 两处；复核通过。最终评审确认完整 diff 无阻塞问题。

## 完成摘要

数学编辑入口缩减约 75%，命令快照/分派/历史提交边界更清晰；现有 876 项 caret-engine 测试与 workspace/native 门禁全部通过。改动限于内部职责拆分与状态入口收敛，无 public API 或预期行为变化。独立领域评审和最终评审的限制是静态检查未逐行证明搬移前后的每个函数文本等价；回归证据来自拆分前后相同测试集及全量门禁。
