# 073 — 块环境内容判定保全公式节点

- 状态：ready
- 依赖：010、021
- 优先级：P1
- 创建 / 更新：2026-11-08 / 2026-11-08

## 目标与背景

块环境的内容判断目前检查标题、非空段落、子环境、显式换行和 opaque 块，但遗漏 `Block::Equation` 与 `Block::Align`。后续空段首退格和外部停靠点删除复用该判断，可能把包含有效公式的环境当成空环境整体移除，造成文档内容丢失。本任务把公式块纳入正确的保留规则，并为嵌套环境、Undo/Redo 建立回归。发现来自对 [issue #105](https://github.com/VibeSTEM/caret/issues/105) 的扩展审查；不依赖先实施焦点路由重构。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[文档模型](../architecture/editor/document-model.md)、[结构导航](../architecture/editor/structure-navigation.md)
- [010 结构导航](010-impl-structure.md)、[021 文档树与存储](021-document-tree-and-storage.md)

## 范围与非目标

- 明确环境“有内容”的定义，检查所有 `Block` 变体的空/非空语义；对不透明或未来未知块采用不会丢数据的保留默认。
- 空 Equation/Align 根是否算作可保留内容需按模型与用户删除契约明确；无论公式是否为空，删除环境不能绕过显式公式对象删除规则。
- 修复 `environment_has_content` 及所有复用方，使后续段首 Backspace 和 `block_environment_exit` 删除判断一致。
- 覆盖直接子节点、嵌套 Environment、Equation、Align、空正文及组合内容；删除失败保留原树和选择，Undo/Redo 恢复完整对象与 ID。
- 不重构整体焦点模型或导航路由（由 071 负责），不扩张 `is_block_environment` 语义到公式块。

## 前置条件与待决策

- 010、021 已完成。
- 开始实现前核对 `.cdoc` 的 Block 变体及 opaque/未来节点前向兼容要求，并明确空数学节点在环境删除中的语义。
- 复现两条消费者路径：空环境后的段首 Backspace 与环境外部停靠点 Backspace；确认失败原子性和 undo 历史。

## 实施步骤

1. 为现有内容检查建立直达回归用例，覆盖含 Equation/Align 的环境及嵌套组合。
2. 统一内容分类，使每种 Block 类型显式处理，未知/不透明对象默认保留。
3. 验证两条退格删除路径、空环境保留/删除规则、撤销重做与状态副作用。
4. 更新结构导航文档和本任务，完成 engine 领域检查与独立 final review 并复验。

## 预计改动

`crates/caret-engine/src/editing/session/structure.rs` 及 `session`/structure 行为测试、必要的结构导航架构说明。无需改数据格式、FFI、QML 或新增 crate。

## 验收标准

- [ ] 含非空 Equation 或 Align 的块环境不会因后续空段首 Backspace 或外部停靠点删除而被当作空环境移除。
- [ ] 嵌套子环境、非空标题/段落、显式换行、opaque 节点及所有 Block 变体的内容保留规则明确且一致。
- [ ] 真正空环境仍按既定键盘契约可删除；空公式节点的行为显式定义，不因通用环境清理而静默丢失。
- [ ] Undo/Redo 精确恢复环境、公式树、节点 ID、焦点/选择；无效位置或事务失败保留文档与会话状态。
- [ ] 相关行为测试、格式/lint、分域审查和独立 final review 通过；未执行验证如实记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | engine structure/session 定向测试 | 公式内容不会被环境空判定误删 | 未执行 |
| — | 退格与外部停靠点删除回归 | 同一内容规则覆盖两条路径 | 未执行 |
| — | Undo/Redo 和失败原子性测试 | 删除恢复完整、失败无副作用 | 未执行 |

## 风险与回退

扩大“环境有内容”判定可能改变原本自动删除仅含空结构的行为。实现前须区分空与非空公式节点并记录决定；如果变化影响其他对象，先收窄到安全保留语义，不能通过删除对象换取一致性。

## 决策与工作记录

- 2026-11-08：Terra/medium 只读扩展审查指出 `environment_has_content` 的 Block 匹配漏掉 Equation/Align，两个退格删除入口复用该判断，存在公式随环境丢失风险。静态调用链已核实，尚未执行复现或测试。

## 完成摘要

未完成。
