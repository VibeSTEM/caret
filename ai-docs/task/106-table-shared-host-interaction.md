# 106 — 表格接入共享宿主交互与编辑几何（046 后续）

- 状态：in-progress
- 依赖：045、046、076、080、082、083、027
- 优先级：P1
- 创建 / 更新：2026-11-22 / 2026-11-22

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

- [ ] 真实窗口/快照中单元格焦点使用共享 block 高亮层；高亮与虚线几何同源，前后台切换、缩放、翻页一致。
- [ ] 辅助线定位正确；Tab/方向键/点击切换单元格时旧虚线清除，离开表格无残留；native 严格快照校验通过。
- [ ] 方向键在 cell 内容边界移动相邻 cell（左右行优先、上下同列优先），有文本时保持文本导航；失败不改变文档/焦点。
- [ ] `table_keys.rs` 删除，Shift+Tab 在 tab.rs、Alt+T 在 alt_t.rs、Alt+方向键在对应方向键文件、Alt(+Shift)+Backspace/Delete 在 backspace/delete 文件；helpers 不复制上下文优先级。
- [ ] 2×2 表格左右起始/几何对称，测试断言列宽、内容 x、规则线和辅助线一致。
- [ ] 行为/native/QML/快照回归、fmt/format/clippy/lint 通过；领域评审与独立 final review 通过，PR 创建并 CI 通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-22 | `cargo test -p caret-engine --lib` | 键盘/表格/方向键/高亮几何行为 | 通过：971 项 |
| 2026-11-22 | `cargo test -p caret-app --lib` | app 快照高亮/辅助线/方向键换格、wire 命令 | 通过：359 项 |
| 2026-11-22 | `cargo test -p caret-tests --test native` | native CTest 与指南线替换渲染 | 通过：CTest 20/20，新 C++ 用例验证旧矩形清除 |
| 2026-11-22 | `cargo fmt --all -- --check`、`cargo format --check`、`cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | 格式与静态检查 | 通过 |

## 风险与回退

表格导航/几何接入共享层会影响 caret、命中、选区和高亮；所有改动保持快照同代校验与失败原子性，必要时回退到显式空高亮而不是保留旧几何。键盘迁移若破坏既有焦点优先级，按基础键 handler 与 helper 的职责边界逐步切分并保留旧测试。

## 决策与工作记录

- 2026-11-22：根据真实窗口测试与维护者反馈创建；确认按 080 共享宿主协议、076 键盘架构收敛，不继续扩大 table_keys.rs。
- 2026-11-22：键盘结构收敛：删除 `table_keys.rs`；Shift+Tab 进 `tab.rs`，Alt+T 新建 `alt_t.rs`，Alt+↑/↓ 行列插入进 `up_arrow.rs`/`down_arrow.rs`/`left_arrow.rs`/`right_arrow.rs`，Alt(+Shift)+Backspace/Delete 行列删除进 `backspace.rs`/`delete.rs`；每类 EditorKey 按具体按键一行分派，`helpers/table.rs` 只保留共享上下文判定。
- 2026-11-22：app 级方向键换格回归（visual/vertical navigation 路径）通过；键盘路由/快捷键文档同步为按实体键归档。
- 2026-11-22：领域评审修复：vertical app phase1/phase2 双提交收敛（表格边界消费一次、失败不改焦点）；空 paragraph 纵向边界识别；单元格内多段 Enter/Left/Up 首末段落点统一；环境内表格允许方向键换格；删除调试输出；app 测试改为断言目标行/列 x/offset/末行 no-op，并补 wire 行列命令用例；几何测试断言表格外框/规则线而非仅相对关系。
- 2026-11-22：表格 cell 外框改为 `TableGridLayout` 网格矩形单点派生：同一矩形进入共享 block 高亮层和 `tableGuidelines`（含边框/内边距拆分后的对称左右边界），修复 2×2 左右起始不一致与辅助线内容盒内缩；新增 engine 对称几何/共享高亮测试、app 切换 cell 高亮/辅助线替换测试、C++ native 指南线整体替换测试（红色像素验证旧矩形清除）。
- 2026-11-22：方向键单元格边界移动：单元格首/末文本边界按行优先进入相邻 cell（上下同列），表格边缘消费为 no-op；新增 2×2 行为测试。
