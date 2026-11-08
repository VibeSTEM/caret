# 073 — 块环境内容判定保全公式节点

- 状态：done
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

- [x] 含非空 Equation 或 Align 的块环境不会因后续空段首 Backspace 或外部停靠点删除而被当成空环境移除。
- [x] 嵌套子环境、段落软换行、opaque 节点及所有 Block 变体的内容保留规则明确且一致。
- [x] 真正空环境仍按既定键盘契约可删除；空公式节点作为显式对象保留，通用环境清理不会静默丢失。
- [x] 含公式环境显式删除后的 Undo/Redo 精确往返完整文档树（含公式对象与节点 ID）及文本选区；公式保护路径验证了文档、焦点/选区及 generation 不变，未知行内内容错误保持失败原子性。
- [x] 相关行为测试、格式/lint、分域审查和独立 final review 通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-08 | `cargo test --locked -p caret-engine editing::session::structure::tests` | 环境内容分类、段首退格、外侧停靠点、Undo/Redo 和失败原子性 | 35 passed |
| 2026-11-08 | `cargo fmt --check` | Rust 格式符合仓库规则 | 通过 |
| 2026-11-08 | `cargo clippy --locked -p caret-engine --all-targets -- -D warnings` | Engine 及依赖目标无 lint 警告 | 通过 |

## 风险与回退

扩大“环境有内容”判定可能改变原本自动删除仅含空结构的行为。实现前须区分空与非空公式节点并记录决定；如果变化影响其他对象，先收窄到安全保留语义，不能通过删除对象换取一致性。

## 决策与工作记录

- 2026-11-08：Terra/medium 只读扩展审查指出 `environment_has_content` 的 Block 匹配漏掉 Equation/Align，两个退格删除入口复用该判断，存在公式随环境丢失风险。静态调用链已核实，尚未执行复现或测试。
- 2026-11-08：核对 `.cdoc` 的 `Block` 变体后，决定 Equation 与 Align 即使数学树为空，也作为显式块对象保留；用户必须通过对应公式删除操作删除它们，通用空环境清理不能隐式移除。Opaque 与 ExplicitBreak 同样视为内容；Paragraph 按 TextMap 是否为空判定；软换行、行内对象视为内容，遇到无法解释的行内 opaque 节点时返回错误并保持操作原子性。Environment 递归判定标题和后代。
- 2026-11-08：实现内容分类的穷尽匹配，覆盖段首环境解除、前一空环境删除和外侧停靠点删除的共享判定；增加空/非空 Equation 与 Align、嵌套环境、Paragraph 软换行、ExplicitBreak、Opaque、公式树 Undo/Redo 和失败原子性回归测试。engine structure/session 35 项测试、`cargo fmt --check` 和 engine clippy 通过；Terra/medium 领域检查与 Sol/medium 独立 final review 均通过，未发现阻断项。

## 完成摘要

已实现并验证：35 项 engine structure/session 测试、`cargo fmt --check`、engine clippy、Terra/medium 领域检查及 Sol/medium 独立 final review 均通过。
