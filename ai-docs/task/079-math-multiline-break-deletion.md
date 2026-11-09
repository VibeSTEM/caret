# 079 — 数学多行换行与删除事务

- 状态：ready
- 依赖：071、074、076
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-09

## 目标与背景

维护者实测发现，在数学编辑中按 Enter 可以创建额外行，但在行首按 Backspace 无法删除该行边界。需要让数学多行结构具有可预测的行边界删除、合并、光标落点和撤销行为，并确保合并不会丢内容或未知元数据。

“换行”在当前模型中有多种表示，不能混为一个软换行节点：Align 的 MathTree Enter 会拆分 cell 内容并创建 `MathAlignRow`；Align 源码分支插入 `ALIGN_ROW_BREAK`；MathTree 中的 `MathContent::SoftBreak` 是另一种节点；TextFlow 还可包含字符形式的强制换行。MathText 中 Enter 当前返回 `UnsupportedNewline`。实现前必须用用户路径确认维护者看到的是哪一种边界。

当前 Tree Align 的行首 Backspace 对非空 cell 无前驱节点时为空操作；空 cell 且 Align 其他位置有内容时，只移动到相邻 cell，不删除行。现有架构文档将此行为写作契约，因此本 task 需要明确更新该契约。源码 `ALIGN_ROW_BREAK` 已有按一个导航/删除单位处理的测试，但不覆盖 Tree 结构行的合并。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[提交规范](../standards/commits.md)
- [数学公式架构](../architecture/editor/math.md)、[数学宿主行框与光标高度统一](077-stable-math-caret-height.md)、[数学光标候选与统一仲裁](074-math-caret-routing-and-geometry.md)
- [宿主焦点协议与键盘意图路由](071-host-focus-and-key-routing.md)、[逐键状态仲裁](076-per-key-state-arbitration.md)

## 范围与非目标

- 定义 Enter 创建的数学多行边界如何由 Backspace/Delete 删除，尤其是 Align MathTree 行与 cell 的合并语义。
- 明确空行、非空行、多 cell 行的内容合并顺序、光标落点、Undo/Redo 和保存重开行为。
- 对齐 MathTree 结构行与源码分隔符的用户可观察删除语义；只有数据模型允许且测试证明安全时才复用同一删除事务。
- 未知或不兼容的行/cell 元数据无法安全合并时，保持文档不变并明确拒绝，不丢弃元数据或相邻内容。
- 不负责数学行框和光标高度策略；删除导致行数变化时，本 task 验证新布局几何有效、焦点落在合并后的正确位置，不修改 077 的验收范围。
- 不为 MathText 开放换行输入，除非单独修改其 `UnsupportedNewline` 产品契约并证明输入法行为兼容。

## 前置条件与待决策

- 通过鼠标/键盘的真实用户路径确认复现场景和活动分支：Align Tree、Align Source、MathTree SoftBreak 或 TextFlow 强制换行。
- 确认行首 Backspace 与行尾 Delete 是否对称；确认跨 Align cell 的合并规则，以及目标行具有不同 cell 数量时的行为。
- 审查现有 `math_tree_align_break`、`math_tree_delete`、源码行分隔符删除及 MathContent 序列删除事务，找出可安全复用的最小边界。
- 更新 `math.md` 当前“空 cell Backspace 只移动到相邻 cell”的条款，使规范明确新旧契约和拒绝条件。

## 实施步骤

1. 追踪 Enter 插入与 Backspace/Delete 删除的完整键盘路由，增加复现真实用户路径的用例并确认边界表示。
2. 确定行合并和光标落点契约，先覆盖空行、非空行、多 cell 与不兼容元数据的安全失败。
3. 在权威 Rust Session/MathTree 事务层实现删除与合并；源码表示仅在语义和失败边界等价时复用代码。
4. 验证 Undo/Redo、文档保存重开、焦点恢复、相邻行内容顺序及旧 generation 状态；同步架构和任务文档。
5. 由独立 subagent 审查完整 diff，修复问题后重跑受影响验证；行框/光标高度策略由 077 独立定义，本 task 只验证删除后的布局和焦点几何有效。

## 预计改动

预计涉及 `crates/caret-engine/src/editing/session/keyboards/enter.rs`、数学编辑的行拆分/删除模块及其测试，以及 `ai-docs/architecture/editor/math.md`。Qt 仅在现有事件传递无法表达必要意图时调整；不得在 Qt 或 QML 重建 MathTree 合并规则。

## 验收标准

- [ ] 用户报告的 Enter 创建行场景有可复现测试，并准确断言其行边界表示及当前焦点位置。
- [ ] 行首 Backspace 删除前一行边界并按确定契约合并内容；行尾 Delete 行为对称或差异有明确文档理由。
- [ ] 空行、非空行、多 cell、不同 cell 数量、TextFlow/SoftBreak 边界均有明确行为和测试，不把 Align 结构行误当普通字符。
- [ ] 不兼容/未知行或 cell 元数据导致安全拒绝时，内容、修订、焦点和撤销历史保持有效且不丢字段。
- [ ] Undo/Redo 保留行结构、内容顺序和可用的焦点映射；保存重开保留行结构与内容顺序，重新进入数学编辑后光标可定位。
- [ ] 删除导致行数变化时，布局快照仍包含有效行几何，caret 落在合并后预期位置，不能指向已删除行或旧 cell。
- [ ] `math.md` 与实现一致，源码分隔符现有删除语义仍通过回归。
- [ ] Rust 行为测试、格式/lint 与独立评审通过；未运行的真实窗口验收如实记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | MathTree Align Enter→行首 Backspace 用户路径测试 | 行边界删除、内容合并和焦点映射正确 | 未执行 |
| — | 空/非空行、多 cell、未知元数据与 Undo/Redo 测试 | 安全合并或失败原子性成立 | 未执行 |
| — | Rust / native 相关验证与独立评审 | 事件链与文档契约一致 | 未执行 |

## 风险与回退

把结构行边界合并成前一行可能移动多个 cell 的内容。只有明确的列映射和事务行为才允许合并；若元数据或 cell 结构无法安全映射，必须保留原文档并拒绝该次操作。回退时保留结构行，只恢复明确的导航行为，不能悄悄删除用户输入。

## 决策与工作记录

- 2026-11-09：维护者报告 Enter 可创建数学额外行，但 Backspace 无法删除行边界。
- 2026-11-09：Terra 只读审计确认 Align Tree Enter 创建 `MathAlignRow`；Tree 行首退格当前不合并行；源码行分隔符已有删除测试；MathText Enter 不支持换行。将行边界事务单列本 task；本 task 测试删除后的几何有效性与焦点落点，077 独立处理高度策略。

## 完成摘要

未完成。
