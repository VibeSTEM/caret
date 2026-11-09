# 079 — 数学多行换行与删除事务

- 状态：implemented
- 依赖：071、074、076
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-09

## 目标与背景

维护者实测发现，在数学编辑中按 Enter 可以创建额外行，但 Backspace 没有按空行/非空行执行预期的多行块行为。统一规则是：空白逻辑行按 Backspace 删除该行；非空逻辑行在行首按 Backspace 不执行操作，不合并行、不移动光标、不改变文档。此规则适用于拥有内部逻辑行/cell 模型的多行块；共享 Session 策略统一作出删行/no-op/委派给 cell 编辑的判断，各文档模型适配器映射自己的行结构和安全边界。

“换行”在当前模型中有多种表示，不能混为一个软换行节点：Align 的 MathTree Enter 会拆分 cell 内容并创建 `MathAlignRow`；Align 源码分支插入 `ALIGN_ROW_BREAK`；MathTree 中的 `MathContent::SoftBreak` 是另一种节点；TextFlow 还可包含字符形式的强制换行。MathText 中 Enter 当前返回 `UnsupportedNewline`。当前唯一可用 Enter 创建逻辑多行块行的是 Align；Section/Custom 是块容器而非行模型，Equation 是单行块，普通段落视觉折行和 `SoftBreak` 不因此变成结构行。后续加入的多行块必须遵循相同的 Backspace 行为契约，并由自身适配器映射结构，不在 UI 复制规则。

当前 Tree Align 的行首 Backspace 对非空 cell 无前驱节点时为空操作；空 cell 且 Align 其他位置有内容时，只移动到相邻 cell，不删除行。目标行为以整行为单位判断：当前行所有 cell 都为空时删除该行；空占位符和无文本的 TextFlow 算空内容，SoftBreak、显式空 Group/Environment 不算空内容；非空行首不改变任何状态。未知 Align/行/cell/tree/flow/run/environment 元数据不能因删行或删除唯一空宿主而丢失，无法无损保留时原子拒绝。源码 `ALIGN_ROW_BREAK` 与 Tree `MathAlignRow` 必须呈现相同的 Backspace 行为；源码行中包含注释时拒绝删除以保留注释。源码编辑在不完整 TeX 草稿中仍按源码草稿编辑处理；草稿未形成有效逻辑行时不适用结构行退格契约。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[提交规范](../standards/commits.md)
- [数学公式架构](../architecture/editor/math.md)、[数学宿主行框与光标高度统一](077-stable-math-caret-height.md)、[数学光标候选与统一仲裁](074-math-caret-routing-and-geometry.md)
- [宿主焦点协议与键盘意图路由](071-host-focus-and-key-routing.md)、[逐键状态仲裁](076-per-key-state-arbitration.md)

## 范围与非目标

- 对当前 Align Tree/Source 实现并抽出可供所有逻辑多行块复用的 Backspace 规则；Section 即使跨多段并归类为 `MultiLine`，也是块容器而非内部行模型，继续按 Section/Paragraph 规则编辑。
- 空行删除时确定前后行的焦点恢复、Undo/Redo 和保存重开行为；非空行行首退格完全无操作、不产生文档历史。
- 整行判空须检查所有 cell；删除行前检查将被移除的行、cell、MathTree/MathFlow 元数据。只有表示和失败边界等价时才复用 Tree/Source 事务。
- 本 task 只定义 Backspace；Delete 的对称行为不在范围内。普通段落视觉折行、TextFlow SoftBreak、Equation 和 Section/Custom 容器不是 Align 逻辑行。
- 空白行、Align 宿主或其 cell/tree/flow/run/environment 携带无法安全丢弃的未知元数据时，保持文档不变并明确拒绝，不丢弃元数据或相邻内容；源码行包含注释时也拒绝删除。
- 不负责数学行框和光标高度策略；删除导致行数变化时，本 task 验证新布局几何有效、焦点落在仍存在的相邻行，不修改 077 的验收范围。
- 不为 MathText 开放换行输入，除非单独修改其 `UnsupportedNewline` 产品契约并证明输入法行为兼容。

## 已决策行为

- 行起点指首 cell 的逻辑起点。非空行首不移动到其他 cell，也不改动焦点。空行删除后优先聚焦前一行末端；删除首行时聚焦新首行起点；唯一空 Align 沿用删除整个空公式块的既有语义。
- 不拼接或重映射 cell。不同 cell 数量不影响非空行 no-op；删行必须拒绝任何会丢弃未知元数据的事务。
- 源码行分隔符、Tree `MathAlignRow`、TextFlow 空行适配共享 Session 层的行 Backspace 决策；各模型适配器负责自己的结构和原子安全边界。

## 实施步骤

1. 追踪 Enter 插入与 Backspace 的完整 Tree/Source 路由，增加用户路径测试并确认边界表示。
2. 覆盖空行删除、非空行首无操作、多 cell、行首/末行/唯一行和不兼容元数据的安全失败。
3. 在权威 Rust Session 层实现整行判空及删除事务；非空行首不产生状态变化；源码表示仅在语义和失败边界等价时复用代码。
4. 验证 Undo/Redo、文档保存重开、焦点恢复、相邻行内容顺序及旧 generation 状态；同步架构和任务文档。
5. 由独立 subagent 审查完整 diff，修复问题后重跑受影响验证；行框/光标高度策略由 077 独立定义，本 task 只验证删除后的布局和焦点几何有效。

## 预计改动

实际改动涉及 Session 通用块行策略、MathEdit Tree/Text/Source 适配器、caret-math 源码行范围投影及相关测试，以及数学和结构导航架构文档。Qt/QML 无需调整。

## 验收标准

- [x] Align Tree/Source 行边界的删除路径有行为测试，覆盖行选择与焦点恢复。
- [x] Backspace 在空行删除该逻辑行；非空行在首 cell 行首完全无操作，不改变文档、generation、焦点或撤销历史。
- [x] 整行判空覆盖全部 cell；首行、末行、相邻空行、唯一空 Align 行、不同 cell 数、TextFlow/SoftBreak 边界行为有明确测试，不混淆逻辑行与视觉折行。
- [x] 不兼容/未知行或 cell 元数据导致安全拒绝时，内容、修订、焦点和撤销历史保持有效且不丢字段。
- [x] Undo/Redo 精确恢复删行前后的行结构和焦点；非空行退格不产生撤销步。保存重开保留行结构与内容顺序，重新进入数学编辑后光标可定位。
- [x] 删除导致行数变化时，布局快照仍包含有效行几何，caret 落在仍存在的邻行，不能指向已删除行或旧 cell。
- [x] 数学架构和结构导航文档把行为记为所有逻辑多行块的契约；当前 Align Tree/Source 与实现一致，既有源码分隔符和 SoftBreak 回归通过。
- [x] Rust 行为测试、格式/lint 与独立评审通过；真实窗口验收未执行，见验证记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-09 | `cargo test -p caret-math --locked` | parser/projection 与布局测试通过 | 135 passed |
| 2026-11-09 | `cargo test -p caret-engine --locked` | Tree/Source/Text 行为、失败原子性、Undo/Redo、保存重开和几何相关测试通过 | 692 passed |
| 2026-11-09 | `cargo fmt --all --check`; `cargo clippy --locked -p caret-engine -p caret-math --all-targets -- -D warnings`; `git diff --check HEAD` | 格式、lint、差异检查通过 | 全部通过 |
| 2026-11-09 | Terra/medium 领域检查；未参与实现的 Sol/medium 完整 diff 只读 final review | 检查状态安全、结构复用、测试与文档 | 无阻塞发现；无桌面人工验收 |

## 风险与回退

空行删除会移除整行及其所有 cell；若它们携带无法安全丢弃的元数据，必须保留原文档并拒绝该次操作。非空行首退格是无操作，因此不会触发行合并或隐式导航。

## 决策与工作记录

- 2026-11-09：维护者明确期望：空白逻辑行 Backspace 删除该行；非空逻辑行行首 Backspace 完全无操作。该契约面向所有逻辑多行块。
- 2026-11-09：当前唯一由 Enter 创建逻辑多行块行的是 Align；Section/Custom 是容器，Equation 是单行，段落视觉折行和 TextFlow SoftBreak 不属于该行模型。
- 2026-11-09：Terra 只读审计确认 Align Tree Enter 创建 `MathAlignRow`；Tree 行首退格当前不合并行；源码行分隔符已有删除测试；MathText Enter 不支持换行。将行边界事务单列本 task；本 task 测试删除后的几何有效性与焦点落点，077 独立处理高度策略。

## 完成摘要

实现和仓库验证已完成。共享 Session 层定义通用逻辑多行块行退格策略，Align Tree/Source/Text 适配器遵循同一契约；空行安全删除，非空行首完全无操作，无法证明元数据可无损保留时原子拒绝。数学和结构导航架构文档已同步。外部窗口实测未执行。
