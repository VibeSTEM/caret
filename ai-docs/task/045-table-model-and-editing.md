# 045 — M3：表格模型与交互

- 状态：in-progress
- 依赖：010、021、043、071
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-22

## 目标与背景

实现可持久化的表格节点、单元格块序列和表格编辑命令。单元格是 Caret 文档结构的容器，不依赖公式解析/排版 crate；公式节点按通用块/行内节点存放，复用 043 的数学树事务与 044 的公式插入入口。045 负责公式节点在单元格中的持久化、编辑目标与导航归属；排版几何由 046 负责。总体验收见 [012 M3 表格](012-impl-table.md)。

## 必读

- [文档模型](../architecture/editor/document-model.md)、[表格](../architecture/editor/tables.md)、[结构导航](../architecture/editor/structure-navigation.md)
- [快捷键](../architecture/interface/keyboard-shortcuts.md)、[测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

表格/行/单元格稳定 ID 和保存格式；单元格由 `Block` 序列组成；创建默认 2×2 表格，增删行列；单元格内 Enter 新建段落；实现表格上下文优先于正文默认行为的 Tab/Shift+Tab 导航，最后一格 Tab 增加一行。保留当前快捷键约定，包括 macOS 的 Option+↑/↓ 导航及 Control+Option+Shift+方向键插入行列。插入行、插入列、删除行、删除列都必须有专用命令和明确快捷键仲裁；具体绑定只维护在[快捷键权威文档](../architecture/interface/keyboard-shortcuts.md)，本任务不另存一份按键表。

首版不支持单元格合并，不实现画布网格布局，不直接依赖公式后端 `caret-math`；公式专用字形绘制和单元格坐标裁剪由 046 负责。

## 前置条件与待决策

开始前须完成 010 结构导航、021 `.cdoc` 文档存储、043 公式模型与编辑，以及 071 宿主焦点/键盘意图协议。071 依赖的 073 公式内容保全也须完成，避免表格单元格复用结构删除路径时误删内容。表格内容不得将单元格限制为纯文本或固定一个段落。

## 实施步骤

1. 定义和验证表格、行与单元格持久化结构及稳定 ID。
2. 实现插入、增删行列、单元格光标和方向键/Tab 导航命令；为增删行列各提供专用命令与快捷键仲裁；失败时保持事务原子。
3. 测试空表、首尾格、最后一格 Tab 新增行、删除边界、保存重开及未知可选字段。
4. 使单元格接纳通用块/行内节点；验证 Equation、Align 和 InlineMath 可持久化、可编辑、可导航，不让表格模块依赖数学后端类型。
5. 更新 012 与架构文档，独立评审后复验。

## 预计改动

`crates/caret-doc` 表格节点、验证和序列化；`crates/caret-engine` 表格命令与导航；必要时更新 CLI/命令行为测试。

## 验收标准

- [ ] 表格、行、单元格在 `.cdoc` 保存重开后 ID、顺序和内容一致。
- [ ] 插入默认 2×2；插入行、插入列、删除行、删除列各有专用命令，快捷键符合唯一快捷键权威文档；无合并单元格。
- [ ] Enter、Tab、Shift+Tab 和表格边界移动符合可观察行为；删除/插入失败不留下半更新状态。
- [ ] 单元格接受通用块/行内公式节点并完成保存重开、编辑目标与导航验证；表格模块不直接依赖 `caret-math`，不复制公式模型。
- [ ] 命令/模型行为测试、格式与 lint 通过；独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 表格模型与保存重开测试 | 表格内容和 ID 往返稳定 | 未执行 |
| 2026-11-22 | `cargo test --locked --offline -p caret-doc --lib` | 表格 DTO 往返、严格字段、结构不变量、能力推导与缺失声明预览 | 通过：113 项；新增表格模型/存储用例 |
| 2026-11-22 | `cargo test --locked --offline -p caret-engine layout_errors_reject` | 含表文档得到明确 `layout.unsupported_content`，不发布半成品快照 | 通过 |
| 2026-11-22 | workspace `cargo check --all-targets`；`caret-engine` 931、`caret-structure` 13 项测试 | 新增 Block 变体的全部穷尽匹配点以显式保守语义补齐 | 通过 |
| — | 表格编辑/导航命令测试 | 边界和失败原子性正确 | 未执行 |
| — | macOS 表格快捷键检查 | 导航键和插入键按规范工作 | 未执行 |

## 风险与回退

单元格结构过窄会阻挡多段内容和后续语义块。本任务提供公式节点在单元格内的模型、编辑与导航边界；公式排版和画布几何由 046 实现，表格模型不保存公式布局副本。

## 决策与工作记录

- 2026-11-08：加入 071 宿主焦点/键盘意图协议前置；该协议先完成 073 的公式内容保全修复。
- 2026-11-09：为专用表格新增行/列与删除行/列操作补齐任务契约；快捷键绑定仍只在快捷键权威文档维护，避免任务文档复制一套映射。
- 2026-11-01：加入 043 前置依赖，状态从 ready 调整回 planned；单元格公式模型验收统一归入本任务，解除 043 与 045 的循环等待。

- 2026-10-29：由 012 拆出表格模型、持久化和编辑交互；公式节点模型复用 043，排版由 044/046 组合。
- 2026-11-21：依赖 010、021、043、071、073 均已完成，开始条件满足，状态由 planned 改为 ready；M3 唯一缺失的内容类型功能（表格布局/画布与 011/012 验收仍分别由 046、012、011 承接）。
- 2026-11-22：开始实现（分支 `yuki/task-045-table-model`）。先审计现有 Block 基座（Session 宿主/位置/边界/历史协议）与 `.cdoc` 存储影响面，再按“Table 作为 Block、单元格持 Block 序列、复用共享适配器”的方向落模型与编辑；布局/画布与单元格几何不在本任务。
- 2026-11-22：确认两个实现决策：① 045 先合并，`\table` 与 Alt+T 在 046 布局完成前保持 UI 不可用；布局层对含表文档返回明确的 `layout.unsupported_content`，Session 级命令与编辑行为照常测试；② 删除行新增快捷键 Alt+Shift+Backspace/Delete（macOS Option+Shift+Backspace/Delete），已先更新[快捷键权威文档](../architecture/interface/keyboard-shortcuts.md)。
- 2026-11-22：落地 S0（模型与存储）：`caret-doc` 新增 `Table`/`TableRow`/`TableCell` 与 `Block::Table`，手写 codec 严格拒绝未知字段，`Document::validate` 校验行/列上限、矩形行列、单元格非空与表格嵌套 24 层；新增能力 `caret.table.v1`（含单元格内语言/公式/环境能力递归推导、raw 扫描递归、显式声明缺失时只读安全预览）；剪贴板片段拒绝表格、结构范围暂不接受表格对象，避免静默丢内容。新增模型与存储测试；`caret-doc` 113 项、`caret-engine` 931 项、`caret-structure` 13 项通过，workspace `cargo check --all-targets` 通过；含表文档布局门禁由新增用例验证返回 `layout.unsupported_content`。- 2026-11-22：落地 S0（模型与存储）：`caret-doc` 新增 `Table`/`TableRow`/`TableCell` 与 `Block::Table`，手写 codec 严格拒绝未知字段，`Document::validate` 校验行/列上限、矩形行列、单元格非空与表格嵌套 24 层；新增能力 `caret.table.v1`（含单元格内语言/公式/环境能力递归推导、raw 扫描递归、显式声明缺失时只读安全预览）；剪贴板片段拒绝表格、结构范围暂不接受表格对象，避免静默丢内容。新增模型与存储测试；`caret-doc` 113 项、`caret-engine` 931 项、`caret-structure` 13 项通过，workspace `cargo check --all-targets` 通过；含表文档布局门禁由新增用例验证返回 `layout.unsupported_content`。
- 2026-11-22：落地 S1（容器路径与只读定位）：段落位置从“环境 ID 链 + 根相对 block_path”重构为递归容器路径（Environment / TableCell 步骤）+ 容器相对块路径 + 文档顺序键；caret-structure 物理定位/结构索引/BlockEdge 覆盖表内节点与单元格段落，engine 段落索引/定位/可变定位/相邻段落/格式范围/选区排序全部改用统一容器解析；StoryId 增加 TableCell 身份，跨 cell 文本位置比较拒绝。测试：表格段落索引与 Story、cell 内拆分、cell 内/跨 cell 位置、结构顺序；caret-engine 934、caret-structure 14 项通过。
- 2026-11-22：落地 S2（单元格内共享编辑事务）：ParagraphState/DocumentPatch::Paragraphs 携带容器路径与文档顺序，单元格内打字/删除/Enter 拆分/合并/Undo/Redo 复用同一历史补丁；显示公式插入 BlockRange 改为容器寻址，单元格内 `\equation`/`\align` 插入/进入/撤销走同一结构事务；行内格式深度按环境层数计算。测试：cell 文本替换与撤销、cell Enter 拆分、cell 显示公式插入与撤销；caret-engine 937 项通过、workspace clippy 通过。- 2026-11-22：落地 S2（单元格内共享编辑事务）：ParagraphState/DocumentPatch::Paragraphs 携带容器路径与文档顺序，单元格内打字/删除/Enter 拆分/合并/Undo/Redo 复用同一历史补丁；显示公式插入 BlockRange 改为容器寻址，单元格内 `\equation`/`\align` 插入/进入/撤销走同一结构事务；行内格式深度按环境层数计算。测试：cell 文本替换与撤销、cell Enter 拆分、cell 显示公式插入与撤销；caret-engine 937 项通过、workspace clippy 通过。
- 2026-11-22：落地 S3（表格命令与导航）：新增 `insert.table`、行列插入/删除与删除行命令（`table.insert_row_before/after`、`table.delete_row`、`table.insert_col_before/after`、`table.delete_col`），插入默认 2×2、整表替换一次原子提交；Tab/Shift+Tab 在单元格间移动，末格 Tab 增加一行；结构横向键在格间移动、纵向结构键按行移动，到边缘离开表格到相邻段落；单元格内 Enter/公式/历史复用前置切片。命令面板 `\table` 仍保持不可用、native/QML 未接键位，用户可见入口等 046 布局完成后再启用（本轮决策）。测试：插入 2×2 与撤销、Tab/Shift+Tab 与末行增长、行/列增删撤销、section 标题拆分/合并索引；caret-engine 942 项通过。