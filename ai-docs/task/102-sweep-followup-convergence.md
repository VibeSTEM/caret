# 102 — Brooks sweep 后续收敛项

- 状态：planned
- 依赖：093、095
- 优先级：P2
- 创建 / 更新：2026-11-17 / 2026-11-17

## 目标与背景

Brooks sweep 的独立复核在已修复项之外给出若干"同族未收敛"点，均属低风险但值得一次收口：
- `commit_document_edit` 成为正文/结构/IME/格式记账入口后，仍有 5 处手写事务：块公式删除（`math_edit.rs` 约 4604）、数学树记账（约 5404）、数学文本待输入格式（`text_inline_environment.rs` 约 222/261）、文本 run 属性（`text_run_property.rs` 约 159）；
- `MathEdit` 构造入口收敛后，`inline_content.rs` 约 385/433 与 `MathEdit::command_snapshot` 仍有 3 处直接构造；
- `math_node_attributes_supported` 只返回 bool，把“未来未知节点格式（应保留为 Opaque 只读预览）”与“当前 schema 非法结构（应报格式错误）”折叠，4 个调用点各自解释。

完成后：事务入口覆盖全部文档提交点，MathEdit 只有构造入口，文档校验用显式结果类型区分“未知/非法”。

## 必读

[验证与评审](../standards/validation-and-review.md)、[代码生命周期](../standards/code-lifecycle.md)、[分层](../standards/layering/rust.md)、任务 085（严格字段与消费者校验）、093。

## 范围与非目标

范围内：上述同族收敛；`math_node_attributes_supported` 的返回类型改造（含调用点与测试）。非目标：改格式接受范围（除 085 已定义者）；无证据的性能改写。

## 前置条件与待决策

`math_node_attributes_supported` 改返回类型属 `pub(crate)` 语义变化，需与 085 的严格校验方案对齐后再动；事务收敛需保证 PendingStyle 不分配新 ID、不推进代际的既有语义。

## 实施步骤

1. 逐个手写事务点接入 `commit_document_edit`（或说明为何口径无法表达，补 `DocumentEditSnapshot` 变体）。
2. `inline_content.rs` 与 `command_snapshot` 改用构造入口。
3. 文档校验改为枚举结果（`Known` / `FutureOpaque` / `Invalid`），调用点按语义分支；补行为测试钉住“未知→只读预览、非法→报错”。
4. 全量门禁 + 独立评审。

## 预计改动

`crates/caret-engine/src/editing/session/math_edit.rs`、`math_edit/text_inline_environment.rs`、`math_edit/text_run_property.rs`、`session/inline_content.rs`、`crates/caret-doc/src/model/math.rs`、`storage_format/*`、相关 tests。

## 验收标准

- [ ] 文档提交点全部走统一入口（或记录不可表达口径并加注释）。
- [ ] `MathEdit` 直接构造点清零（测试除外）。
- [ ] 未知/非法节点在 open 与预览路径上行为有测试且与 085 一致。
- [ ] 全量门禁与独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --locked -p caret-engine -p caret-doc` | 全绿 | 未执行 |

## 风险与回退

第 3 点会改 `open()` 的边界语义，必须与 085 一起设计并补差分测试（sweep 曾在同类改动上出现预览↔硬错误回归，已由策略参数化修复，可复用该模式）。

## 决策与工作记录

- 2026-11-17：复核提出上述同族点；本任务收口，避免“修了一半”的分叉继续扩散。

## 完成摘要

未完成。
