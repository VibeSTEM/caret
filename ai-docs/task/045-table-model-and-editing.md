# 045 — M3：表格模型与交互

- 状态：implemented
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

- [x] 表格、行、单元格在 `.cdoc` 保存重开后 ID、顺序和内容一致（`caret-doc` 模型/存储用例 + `tests/rust/document.rs` 黑盒往返，含单元格内 InlineMath/Equation）。
- [x] 插入默认 2×2；插入行、插入列、删除行、删除列各有专用命令；无合并单元格。快捷键意图（Tab/Shift+Tab、结构方向键、删除行 Alt+Shift+Backspace/Delete）已在 Rust 实现并先更新唯一快捷键权威文档；native 物理键位与命令面板入口按决策随 046 接线。
- [x] Enter、Tab、Shift+Tab 和表格边界移动符合可观察行为；删除/插入失败不留下半更新状态（候选文档克隆校验后才替换，失败保留文档、选区、generation 与历史）。单元格移动只改选区，不推进 generation/历史。
- [x] 单元格接受通用块、行内公式、显示公式与 Align 的模型与持久化；表格模块不直接依赖 `caret-math`，不复制公式模型。**限制**：单元格内公式的编辑目标/进入/导航依赖 046 单元格画布与容器感知公式定位器，当前明确返回 `edit.table_cell_math_unsupported`，不会进入半坏状态。
- [x] 命令/模型行为测试、格式与 lint 通过；领域评审与独立 final review 通过；PR CI 全绿（PR #167）。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-22 | `cargo test -p caret-doc --lib`；`cargo test -p caret-tests --test document table` | 模型/存储往返、严格字段、结构不变量、能力推导与缺失声明预览 | 通过：113 项 + 黑盒 `.cdoc` 往返（含单元格内 InlineMath/Equation） |
| 2026-11-22 | `cargo test -p caret-engine layout_errors_reject` | 含表文档得到明确 `layout.unsupported_content`，不发布半成品快照 | 通过 |
| 2026-11-22 | `cargo test -p caret-engine --lib` | 单元格文本/Enter/继续键入、行列增删原子性、Tab/Shift+Tab 与结构边界、公式显式拒绝、section 标题拆分/合并索引 | 通过：952 项 |
| 2026-11-22 | workspace `cargo test --locked --offline --workspace` | 全量 Rust/native/QML 回归 | 通过（见完成摘要） |
| 2026-11-22 | `cargo fmt`/`cargo format --check`、`cargo clippy -D warnings`、`cargo lint --check` 9/9 | 格式、跨层与静态检查 | 通过 |
| 2026-11-22 | 领域评审（只读完整 diff） | 架构复用、容器路径、原子性、导航历史、测试真实性、文档一致性 | 完成：P1-1（cell Enter 后续输入 WrongStory）、P2-1（表格命令非原子）、P2-2（导航进历史）已修复；P1-2（cell 公式编辑/导航）降级为显式拒绝并移交 046；P2-5（文档夸大/矛盾）已按实际重写；P2-3/P2-4/P2-6 边界记录于下 |
| 2026-11-22 | PR #167 CI（ubuntu：detect/format/clippy/lint 9/9/math-backend/check-build/audit） | 合并前检查 | 通过：全部 pass（部分路径跳过）；合并到 main |
| 2026-11-22 | 独立 final review（只读完整 diff） | 最终合并前检查 | 首轮：无 P1，6 项 P2（结构方向键历史、纵向边界回退、复制过度拒绝、table-only 打开、单元格环境标题 Story、行内公式箭头进入）→ 已修复并补回归；二次复审：6 项全部确认解决、无新增 P1/P2，可创建 PR。P3（翻译条目、边界测试、错误码、空表复制）已顺手修复 |
| — | macOS 表格快捷键检查 | 导航键和插入键按规范工作 | 未执行：native/QML 键位与窗口验证随 046/012 完成 |

## 风险、限制与 046 前置

- **单元格内公式编辑/导航**：模型与持久化完整，但编辑目标解析/相邻公式查找/结构导航仍是环境链实现；045 对 cell 内 `\math`/`\equation`/`lign` 显式返回 `edit.table_cell_math_unsupported`。046 必须先把 `find_block_path`、`find_inline_math_in_blocks`、`paragraph_beside_block`、composition 公式定位统一改为容器路径感知，再接画布。
- **整表复制/结构范围删除**：当前显式拒绝（`edit.table_content_unsupported`）；结构范围/剪贴板只有与表格相交时拒绝，未覆盖表格的正文复制可继续使用。单元格内多段文本复制同样因表格区间相交显式拒绝，随 046 的单元格范围协议接入。046 接入画布后补齐整表对象范围、剪贴板与文本投影。
- **定位器边界**：engine 的 `BlockContainerPath`/`TableCellPath`/`document_path` 是编辑定位权威；`caret-structure` 新增的 `block_list_by_table_cell(_mut)` 目前无生产消费者（仅结构测试），`find_document_node` 的父链尚未压入表格步骤。046 前应收敛为一个共享 locator（082 方向），或删除孤儿 API；不得再引入第三套寻址。
- **跨 cell 位置**：跨单元格文本位置比较返回 `WrongStory`，跨 cell 选择/删除随 046 的单元格几何与范围协议接入。
- **性能**：表格结构命令整表快照 + 全文校验 + 索引重建，未设性能门槛；列宽/行高/分页/命中/增量布局全部属 046。
- **导航细节**：Shift+Tab 在首格 no-op；离开表格只处理相邻 Paragraph，邻块为公式/环境时暂 no-op；单元格内多段只能拆分，跨段合并随 046 导航接入。

## 决策与工作记录

- 2026-10-29：由 012 拆出表格模型、持久化和编辑交互；公式节点模型复用 043，排版由 044/046 组合。
- 2026-11-01：加入 043 前置依赖，单元格公式模型验收归入本任务，解除 043 与 045 的循环等待。
- 2026-11-08：加入 071 宿主焦点/键盘意图协议前置；该协议先完成 073 的公式内容保全修复。
- 2026-11-09：为专用表格新增行/列与删除行/列操作补齐任务契约；快捷键绑定只在权威文档维护。
- 2026-11-21：依赖 010、021、043、071、073 均已完成，状态由 planned 改为 ready。
- 2026-11-22：开始实现（分支 `yuki/task-045-table-model`）；先审计 Block 基座与 `.cdoc` 存储影响面，按“Table 作为 Block、单元格持 Block 序列、复用共享适配器”落模型与编辑。
- 2026-11-22：决策：① 045 先合并，`\table` 与 Alt+T 在 046 布局完成前保持 UI 不可用；布局层对含表文档返回明确 `layout.unsupported_content`；② 删除行快捷键 Alt+Shift+Backspace/Delete（macOS Option+Shift+Backspace/Delete）先更新权威文档。
- 2026-11-22：S0 模型与存储：`Table`/`TableRow`/`TableCell`、严格 codec、结构校验、`caret.table.v1` 能力与 raw/Opaque/能力递归、剪贴板与结构范围显式拒绝。
- 2026-11-22：S1 容器路径与只读定位：递归 `BlockContainerPath`（Environment/TableCell）+ 容器相对路径 + 文档顺序键；`caret-structure` 物理定位/索引覆盖表内节点；`StoryId::TableCell`，跨 cell 文本位置拒绝。
- 2026-11-22：S2 单元格共享编辑事务：`ParagraphState`/`DocumentPatch::Paragraphs` 携带容器；cell 内文本/Enter 拆分/合并/Undo/Redo 复用同一历史；section 标题拆分/合并索引按容器修正；显示公式插入先容器化，后按评审降级为显式拒绝。
- 2026-11-22：S3 表格命令与导航：`insert.table`、行列增删专用命令、Tab/Shift+Tab、结构方向键；命令失败候选克隆校验原子回滚，移动只改选区不进历史。
- 2026-11-22：领域评审修复：cell Enter 新焦点/PositionMap Story 改为继承原 Story（P1-1）；表格命令改候选文档校验后提交并补 63 列失败原子性测试（P2-1）；单元格导航不再产生历史步/generation（P2-2）；cell 公式编辑显式拒绝并移交 046（P1-2 降级）；命令 ID 统一为 `table.insert_row_before/after` 等并补 `\table` 不可用策略；文档架构同步与限制记录（P2-5/P2-6）。

## 完成摘要

完成：表格作为 Block 的模型、`.cdoc` 持久化、严格校验与 `caret.table.v1` 能力；单元格持有通用 Block 序列，可持久化多段、行内公式、显示公式与 Align。编辑定位重构为递归容器路径 + 文档顺序键，cell 内文本、Enter 拆分在共享历史/选区事务下工作（跨段合并待 046 导航接入）；表格插入 2×2、行列增删、Tab/Shift+Tab 与结构方向导航在 Session 层完成，失败原子、移动无副作用。随 046 的入口启用、单元格内公式编辑/导航、整表复制/结构删除与窗口验收仍属 046/012；045 的用户可见验收不以本任务单独宣称完成。
