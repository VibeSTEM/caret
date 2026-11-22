# 106 — 表格接入共享宿主交互与编辑几何（046 后续）

- 状态：implemented
- 依赖：045、046、076、080、082、083、027
- 优先级：P1
- 创建 / 更新：2026-11-22 / 2026-11-22
- PR：[#169](https://github.com/VibeSTEM/caret/pull/169)

## 目标与背景

046 已合入表格模型、布局与画布，但真实窗口测试暴露表格仍像“外挂”而不是 080 共享宿主的成员：

1. 单元格焦点没有使用统一 block 焦点高亮（奶油黄块层），只有独立的表格虚线。
2. 表格编辑辅助线定位错误，切换单元格后旧虚线仍残留。
3. 方向键不能在单元格边界像 Tab/Shift+Tab 一样移动；单元格内仅空文本时几乎无法用方向键换格。
4. 键盘结构不符合 076/080 逐键架构：`table_keys.rs` 把 Shift+Tab、Alt+T、行列键混在一起，`keyboards.rs` 对 ShiftTab 单独分支；修饰变体没有归入基础键 handler。
5. 2×2 表格左右起始位置/几何不一致（真实窗口可观察）。

目标：按 080 的 Block 宿主共享协议接入 Table 适配器，复用统一焦点高亮、边界导航、快照几何与键盘仲裁；删除平行视觉/路由，不复制 Paragraph/Math 的交互状态。

## 必读

- [080 共享宿主交互底座](080-shared-host-boundary-interaction.md)
- [Block 宿主共享交互协议](../architecture/editor/document-model/block-interaction.md)
- [结构导航](../architecture/editor/structure-navigation.md)
- [画布与交互背景](../architecture/interface/canvas-highlighting.md)
- [快捷键管理架构](../architecture/interface/keyboard-shortcut-architecture.md)
- [键盘路由](../architecture/interface/keyboard-routing.md)
- [表格架构](../architecture/editor/tables.md)

## 范围与非目标

范围：
- 表格单元格焦点高亮进入共享 block 高亮层；辅助线只从同一宿主/网格几何派生，不再维护第二套坐标。
- 辅助线切换单元格时原子替换，离开表格清空；修复页面裁剪、缩放与分页下的定位。
- 方向键在单元格内容边界进入相邻单元格（左右按行优先，上下按列），与 Tab/Shift+Tab 共用表格导航适配器；单元格内仍有文本时保持文本导航。
- 键盘按 076/080 重组：`table_keys.rs` 删除；Shift+Tab 并入 `tab.rs`，Alt+T 新建 `alt_t.rs`，Alt+方向键分别并入 `left_arrow.rs`/`right_arrow.rs`/`up_arrow.rs`/`down_arrow.rs`，Alt(+Shift)+Backspace/Delete 分别并入 `backspace.rs`/`delete.rs`；表格领域动作保留在 `editing/session/table.rs`，`keyboards/helpers/table.rs` 只放共享上下文判定，不重新决定上下文优先级。
- 2×2 表格左右几何来源单一化并修复真实窗口起始位置不一致；用同一 `TableGridLayout`/页面片段导出高亮与辅助线。
- 行为、快照、native/QML 回归与文档同步。

非目标：整表复制/结构范围删除、跨 cell 拖选、ceil 内嵌套表/ExplicitBreak、超高行行内分页、单元格合并。

## 前置条件与待决策

- 080 已 done；Table 在文档中按 Block/容器身份接入。
- 需确认“方向键换格”的边界语义：文本 caret 移到单元格首/末字素时，再按同方向进入相邻单元格；不把方向键无条件变成纯网格导航。若维护者要求无条件网格导航，另行改验收。
- 辅助线视觉是否保留虚线由本任务保留 046 验收；但必须消费统一 block 高亮几何，不做第二套脑裂状态。

## 实施步骤

1. 复现并记录五个问题的最小行为/快照证据；确认 2×2 几何差异来源（列宽、页边距、片段 x、guideline x）。
2. 在布局快照提供 Table 宿主的统一几何查询：当前 cell/table 的页面矩形、网格线与 clip，仅供高亮、辅助线、命中、caret 共用；删除 app/native 中的平行坐标计算。
3. 把 cell 焦点矩形并入既有 block 高亮来源（与 Section/Equation 同一列表/渲染层）；辅助线改为共享几何的派生数据，交互快照全量替换，离开表格发布空。
4. 在 Session 共享边界导航中接入 Table 适配器：方向键到达 cell 文本/行边界时解析相邻 cell，复用 block boundary/visual candidate 的提交与失败原子性；Tab/Shift+Tab 复用同一 cell 导航操作。
5. 按逐键架构迁移键盘文件，删除 `table_keys.rs`；每个修饰变体留在基础键 handler，helper 只放共享校验/提交与表格动作。
6. 修复 2×2 左右几何；补 engine/app/native/QML 回归，更新架构与任务文档。
7. 分域评审 + 独立 final review；PR CI 通过后授权合并。

## 预计改动

`crates/caret-engine/src/editing/session/keyboards*`、`editing/session/table.rs`、`layout/*`、`caret-layout/src/table*`、`caret-app/src/document_view/*`、`native/canvas/*`、`qml/PageSurface.qml`、相关测试与文档。

## 验收标准

- [ ] 单元格焦点使用共享 block 高亮层；高亮与虚线几何同源，前后台切换、缩放、翻页一致。（headless engine/app/native 快照与像素测试通过；真实窗口人工视觉未执行，见限制）
- [x] 辅助线定位正确；Tab/方向键/点击切换单元格时旧虚线清除，离开表格无残留；native 严格快照校验通过。
- [x] 方向键在 cell 内容边界移动相邻 cell（左右行优先、上下同列优先），有文本时保持文本导航；失败不改变文档/焦点。
- [x] `table_keys.rs` 删除，Shift+Tab 在 tab.rs、Alt+T 在 alt_t.rs、Alt+方向键在对应方向键文件、Alt(+Shift)+Backspace/Delete 在 backspace/delete 文件；helpers 不复制上下文优先级。
- [x] 2×2 表格左右起始/几何对称，测试断言表格外框/规则线/内容 x/辅助线一致。
- [ ] 行为/native/QML/快照回归、fmt/format/clippy/lint 通过；领域评审与独立 final review 通过，PR 创建并 CI 通过。（PR #169 已创建，12/12 CI 通过）

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-22 | `cargo test -p caret-engine --lib` | 键盘/表格/方向键/高亮几何行为 | 通过：972 项 |
| 2026-11-22 | `cargo test -p caret-app --lib` | app 快照高亮/辅助线/方向键换格、wire 命令、进出/错误收敛 | 通过：369 项 |
| 2026-11-22 | `cargo test -p caret-tests --test native` | native CTest、辅助线替换、边框像素厚度 | 通过：CTest 20/20，新 C++ 用例验证旧矩形清除与亚像素轴线厚度一致 |
| 2026-11-22 | PR #169 后续截图反馈 | 整表高亮、块进出、QML 错误、边框粗细 | 修复并补测试；待复评/CI |
| 2026-11-22 | `cargo fmt --all -- --check`、`cargo format --check`、`cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | 格式与静态检查 | 通过 |
| 2026-11-22 | `cargo test --locked --offline --workspace` | 全量 Rust/native/QML 回归 | 通过：2084 项（engine 972/app 369/layout 160/structure 15 + native 聚合） |
| 2026-11-22 | `cargo lint --check` | Rust/C++/QML/CMake/依赖 9 阶段 | 通过：9/9 |
| 2026-11-22 | PR [#169](https://github.com/VibeSTEM/caret/pull/169) CI | 适用作业通过 | 通过：12/12 作业成功 |
| — | 真实窗口人工视觉：焦点高亮、虚线对齐、缩放/翻页/前后台 | 桌面可观察 | 未执行：本环境只有 headless app/native 与 CTest；与 046 先例一致，作为外部验收限制记录 |
| — | bidi/RTL 视觉边界、跨格 Shift 扩选、cell 内嵌环境/公式停靠点 | 后续同族任务 | 未验证/未实现：不属于本任务范围，已列限制 |

## 已知限制与未执行项

- 真实窗口人工视觉验收未执行（headless 证据：engine/app 快照、native CTest 20/20、C++ 辅助线像素替换与边框厚度测试）。
- bidi/RTL 单元格边界仍按逻辑 offset 判定，未验证视觉边界。
- 环境（Section）body 含表格时，表格结构选区跨越宿主会触发 `layout.stale_selection_geometry` 粘滞错误（发布端 `crates/caret-app/src/document_view/snapshot.rs`）；表格结构选区/范围删除属后续同族任务，本任务未改。
- 表格边缘进入/离开直接落到 cell 内部位置，不提供 Equation/Align 式块外 BlockEdge 停靠；与“块行为一致”的差异已记录，后续 080 表格宿主三态可继续收敛。
- 整表 block 高亮在 native 复用环境高亮层（统一 padding/radius），奶油黄可能比实线边框外扩约 2px 且四角圆角化；真实窗口需确认是否符合预期，跨页整表高亮暂无自动化用例。
- 跨 Story 表格候选守卫在水平/纵向/共享选择提交点各有一处，行为已收敛但尚未合并为单一 helper。
- cell 内嵌环境/公式的 Escape 与显示公式停靠点仍按既有复杂块路径，未纳入本轮表格进出测试。
- 表格 cell 内**跨 cell 扩选**收敛为 no-op（保持 cell 内扩选）；cell 内嵌环境的方向键落点、cell 内显示公式宿主停靠点错误为后续同族任务，本任务未改。
- 命中框仍按 046 既有列内缩推导，未与新 cell 外框完全统一；记录为后续几何收敛项。
- `table_vertical_block_boundary` 只表示 cell 首/末块，不感知视觉行；折行 text 的视觉行导航由布局候选负责。
- 整表“选中态”与“cell 编辑态”共用整表 block 高亮；cell 编辑用当前 cell 虚线区分，暂不提供独立的整表结构选中光标。
- 未新增 QML 代码；QML 仅沿用既有 `tableGuidelines`/`blockHighlightRects` 绑定，由 native CTest Qml.ShellLoad 覆盖加载。

## 独立评审

- **领域评审（未参与实现，只读）**：三轮。首轮发现纵向方向键 app phase1/phase2 双提交、空 cell 边界 no-op、多段目标落点、测试弱断言等 P1/P2；修复后复验发现折行 cell 文本纵向导航回归（P1）；再修复后确认 P1 清零，仅剩记录/覆盖 P2/P3。评审同时核对 076/080 键盘与宿主结构、wire 协议、高亮/辅助线同源、C++ 像素测试真实性。
- **截图反馈轮独立终审（未参与实现，只读完整 diff）**：范围 `2fc395a...e181a2c`。复核整表高亮/cell 虚线、块级进出、QML 错误收敛、native 边框像素对齐；发现并修复 `[正文, Equation, Table]` 水平边缘被表格候选吃掉、表格外结构键 `invalid_position`、两处权威文档矛盾；补结构键与表格外 intent 回归。复验 P1/P2 清零，P3 为视觉/跨页/守卫去重/代理测试等记录项。
- **独立最终评审（未参与实现，只读完整 diff）**：范围 `70985b0...2fc395a`。独立复跑 workspace 2077 项、native CTest 20/20、fmt/format/clippy/lint 9/9；差分探针验证 `edit.position_story_mismatch` 旧问题消失、2×2 高亮/辅助线逐字段一致、离开表格清空、正文折行导航无回归；确认 `table_keys.rs` 无残留、逐键归档与 wire 拆分为真。无 P1/P2 代码阻断；P2 仅要求本记录补齐（本节），P3 为注释/测试强度/既有 hit clip 等后续项。结论：可创建 PR；本记录与 PR CI 齐备后可标 `implemented`。

## 风险与回退

表格导航/几何接入共享层会影响 caret、命中、选区和高亮；所有改动保持快照同代校验与失败原子性，必要时回退到显式空高亮而不是保留旧几何。键盘迁移若破坏既有焦点优先级，按基础键 handler 与 helper 的职责边界逐步切分并保留旧测试。

## 决策与工作记录

- 2026-11-22：根据真实窗口测试与维护者反馈创建；确认按 080 共享宿主协议、076 键盘架构收敛，不继续扩大 table_keys.rs。
- 2026-11-22：键盘结构收敛：删除 `table_keys.rs`；Shift+Tab 进 `tab.rs`，Alt+T 新建 `alt_t.rs`，Alt+↑/↓ 行列插入进 `up_arrow.rs`/`down_arrow.rs`/`left_arrow.rs`/`right_arrow.rs`，Alt(+Shift)+Backspace/Delete 行列删除进 `backspace.rs`/`delete.rs`；每类 EditorKey 按具体按键一行分派，`helpers/table.rs` 只保留共享上下文判定。
- 2026-11-22：app 级方向键换格回归（visual/vertical navigation 路径）通过；键盘路由/快捷键文档同步为按实体键归档。
- 2026-11-22：结构导航键在 body↔table 边界的目标 Story 改为从段落索引推导，进入/离开表格按块邻接处理，消除 `struct.next: edit.position_story_mismatch`。
- 2026-11-22：终审修复：跨 Story 表候选在表格不直接相邻（如 `[正文, Equation, Table]`）时回落到既有复杂块边缘逻辑，避免水平方向键被吃掉；表格行列插入/删除键在表格外统一 no-op，消除其余 `edit.invalid_position` QML 警告；同步 canvas-highlighting/structure-navigation 唯一权威文档。
- 2026-11-22：按维护者截图反馈继续修复：block 高亮从当前 cell 改为整张表格（所有 cell 完整奶油黄）且边框保持实线；当前 cell 仅保留网格外框虚线；方向键在表格边缘按块宿主离开/进入相邻正文段落，Escape 从 cell 退出；普通/Shift 方向键与结构导航键的跨 Story 表格候选统一收敛（扩选 no-op、普通移动进入相邻格），消除 `edit.position_story_mismatch`；不可用表格插入键 no-op，消除 `table_insertion_unsupported`；native 轴线按设备像素对齐，修复 Enter 后边框粗细不均。补 engine/app/native 回归。
- 2026-11-22：领域评审三轮修复：方向键 cell 目标确实改用共享 `focus_table_cell` 提交；补 app 非空目标 cell Up/Down 回归。纵向 app phase1 只由视觉边界触发，`table_vertical_block_boundary` 仅用于 phase1 后早退；折行 cell 纵向文本行导航恢复并补 app 回归；方向键目标提交改用共享 `focus_table_cell`；补正文空段纵向 no-op 测试。
- 2026-11-22：领域评审修复：vertical app phase1/phase2 双提交收敛（表格边界消费一次、失败不改焦点）；空 paragraph 纵向边界识别；单元格内多段 Enter/Left/Up 首末段落点统一；环境内表格允许方向键换格；删除调试输出；app 测试改为断言目标行/列 x/offset/末行 no-op，并补 wire 行列命令用例；几何测试断言表格外框/规则线而非仅相对关系。
- 2026-11-22：表格 cell 外框改为 `TableGridLayout` 网格矩形单点派生：同一矩形进入共享 block 高亮层和 `tableGuidelines`（含边框/内边距拆分后的对称左右边界），修复 2×2 左右起始不一致与辅助线内容盒内缩；新增 engine 对称几何/共享高亮测试、app 切换 cell 高亮/辅助线替换测试、C++ native 指南线整体替换测试（红色像素验证旧矩形清除）。
- 2026-11-22：方向键单元格边界移动：单元格首/末文本边界按行优先进入相邻 cell（上下同列），表格边缘消费为 no-op；新增 2×2 行为测试。
