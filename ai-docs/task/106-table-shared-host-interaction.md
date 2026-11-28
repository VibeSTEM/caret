# 106 — 表格接入共享宿主交互与编辑几何（046 后续）

- 状态：implemented
- 依赖：045、046、076、080、082、083、027
- 优先级：P1
- 创建 / 更新：2026-11-22 / 2026-11-28
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

## 初次合入范围

本节记录 2026-11-22 的实现切片。反馈后的范围见[复查与修复](#2026-11-27-复查与修复)。

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

- [x] 单元格焦点使用共享 block 高亮层，高亮与虚线几何同源。Cocoa 窗口去激活与恢复、缩放、翻页的几何和像素证据见后续窗口验收。
- [x] 辅助线定位正确；Tab/方向键/点击切换单元格时旧虚线清除，离开表格无残留；native 严格快照校验通过。
- [x] 方向键在 cell 内容边界移动相邻 cell（左右行优先、上下同列优先），有文本时保持文本导航；失败不改变文档/焦点。
- [x] `table_keys.rs` 删除，Shift+Tab 在 tab.rs、Alt+T 在 alt_t.rs、Alt+方向键在对应方向键文件、Alt(+Shift)+Backspace/Delete 在 backspace/delete 文件；helpers 不复制上下文优先级。
- [x] 2×2 表格左右起始/几何对称，测试断言表格外框/规则线/内容 x/辅助线一致。
- [ ] 行为/native/QML/快照回归、fmt/format/clippy/lint 及独立评审通过，当前分支创建 PR 并通过 CI。历史 PR #169 的 12 项作业已通过，本轮 CI 待执行。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-22 | `cargo test -p caret-engine --lib` | 键盘/表格/方向键/高亮几何行为 | 通过：973 项 |
| 2026-11-22 | `cargo test -p caret-app --lib` | app 快照高亮/辅助线/方向键换格、wire 命令、进出/错误收敛 | 通过：369 项 |
| 2026-11-22 | `cargo test -p caret-tests --test native` | native CTest、辅助线替换、边框像素厚度 | 通过：CTest 20/20，新 C++ 用例验证旧矩形清除与亚像素轴线厚度一致 |
| 2026-11-22 | PR #169 后续截图反馈 | 整表高亮、块进出、QML 错误、边框粗细 | 修复并补测试；待复评/CI |
| 2026-11-22 | `cargo fmt --all -- --check`、`cargo format --check`、`cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | 格式与静态检查 | 通过 |
| 2026-11-22 | `cargo test --locked --offline --workspace` | 全量 Rust/native/QML 回归 | 通过：2083 passed / 1 ignored（34 个测试 target，合计 2084 项，含 native 聚合；engine 973/app 369/layout 160/structure 15） |
| 2026-11-22 | `cargo lint --check` | Rust/C++/QML/CMake/依赖 9 阶段 | 通过：9/9 |
| 2026-11-22 | PR [#169](https://github.com/VibeSTEM/caret/pull/169) CI（HEAD `71318c7`，run 45027580206） | 适用作业通过 | 通过：12/12 作业成功 |
| 2026-11-28 | 生产浅色及深色主题的表格抓帧 | 整表背景与当前格虚线清晰可辨 | Cocoa 与离屏软件渲染通过。应用间前后台切换仍待桌面验收。 |
| — | bidi/RTL 视觉边界、跨格 Shift 扩选、cell 内嵌环境/公式停靠点 | 后续同族任务 | 未验证/未实现：不属于本任务范围，已列限制 |

## 2026-11-27 复查与修复

| 真实窗口反馈 | 原因与修改 |
| --- | --- |
| 方向键或点击换格后旧虚线停留到输入文字 | Qt 在交互快照路径把新矩形追加到旧列表。改为先在空列表校验，再原子替换。空列表清除旧虚线，过期快照不改已发布状态。 |
| 表格缺少前、内部、后停靠 | 表格边界原先直接跳段。与环境、Equation、Align 共用块边界与相邻宿主提交，表格提供首末格入口。 |
| 整表无法选中 | Table 加入共享原子块范围。Shift 扩选、逐级扩大选区与跨格拖选进入同一结构选区。删除与替换沿用结构事务与 Undo/Redo。 |
| 第二列起没有格首空隙 | 表格原先只为首末列预留外侧内缩。现在每格都预留两侧内边距，内容、网格线、命中与辅助线消费同一几何。 |
| 公式与表格宿主路径分离 | 相邻块与容器定位移至 Session 共享模块。单元格内部入口复用块序列，公式投影保留完整表格容器、统一投影行框与绘制。 |
| 行列快捷键缺失或被窗口抢占 | 画布为行列编辑接受 ShortcutOverride。Linux/Windows 可用 Alt+方向键或 Ctrl+Alt+Shift+方向键，macOS 可用 Control+Option+Shift+方向键。删除键见[快捷键表](../architecture/interface/keyboard-shortcuts.md#正文与结构)。 |

### 首轮验证

回归沿用 Session、app 快照与 Qt/QML 原生画布。覆盖虚线更换和离开清除、过期交互快照、跨页整表选区与共享外框、边缘停靠、格内公式进出和局部替换撤销、列内边距命中、物理快捷键到行列事务。

| 本地验证 | 结果 |
| --- | --- |
| `cargo test --locked --offline --workspace` | 2093 通过，1 忽略；native CTest 20/20。终审补充有标题的嵌套环境边界后，定向回归通过。 |
| `cargo coverage` | 函数 86.26%，行 88.78%，通过 85% 门槛。使用宿主已安装的 LLVM 23 工具读取覆盖数据。 |
| `cargo coverage native` | native CTest 20/20。产品源码函数 91.36%，行 86.52%，通过 85% 门槛。LLVM 汇总提示 4 个函数的覆盖数据不匹配。 |
| `cargo lint --check` | 9/9 通过。 |
| `cargo format --check`、文档相对链接与 `git diff HEAD --check` | 通过。 |

领域评审覆盖容器路径、原子删除、状态投影、分页几何与架构文档。独立终审以 `29d3c1a` 为基线，补齐空单元格补位后的替换落点、标题候选、公式跨格导航与临时格式清理。终审复核确认最后一项标题边界已修复。

### Brooks-Lint 复查修复

- 在单元格内子块的外侧，Tab 和 Shift+Tab 从宿主的容器路径解析当前格。表末增行后焦点进入新行；撤销恢复原宿主停靠，重做恢复新格光标。
- 水平移动、Shift 扩选和格内首末边界判定共用可导航同级块查找，跳过分页符和不透明块后按共享停靠顺序进入相邻内容。
- Control+Space 保留当前结构范围，再沿实际父容器逐级扩大。到达最外层时保持范围和高亮。
- 单元格正文加入共享拖选命中图。正文、行内公式文本、公式树端点校验后使用同一跨格提升路径。嵌套格按端点首次分叉的容器选择表格，表外公式保留范围内的正文切口。有效拖选与预编辑取消一起提交，过期命中不改当前状态。

Qt 鼠标回归从较长单元格拖到较短单元格的空白处，检查拖动中和松手后的整表选区。普通方向键跳过无内部停靠的空环境，Shift 仍可将它纳入结构选区。

领域评审与独立终审覆盖相对 `29d3c1a` 的工作树差异。修复后复核了末格增行的历史状态、空环境导航、按单元格约束的指针命中和同代回放。

| 复查验证（macOS arm64） | 结果 |
| --- | --- |
| `cargo test --locked --offline --workspace` | 2108 项通过，1 项忽略，native CTest 20/20。Qt 鼠标不等高单元格拖选用例通过。 |
| `cargo coverage` | Rust 函数 86.43%，行 88.96%，通过 85% 门槛。 |
| `cargo coverage native` | CTest 20/20。产品源码函数 91.36%，行 86.55%，通过 85% 门槛。LLVM 汇总仍提示 4 个函数覆盖数据不匹配。 |
| `cargo lint --check` | 9/9 通过。 |
| `cargo format --check`、文档相对链接与 `git diff HEAD --check` | 通过。 |

### Brooks Health 后续优化

- 正文拖选先校验预编辑版本和布局端点，再映射到已提交文档。取消预编辑和安装选区共用 Session 事务，画布在新布局中沿用 Rust 返回的逻辑锚点。离屏 Qt 回归覆盖从新增预编辑字节发起的段内拖选与跨格拖选，检查锚点、整表选区及文档代际。
- 块外多行预编辑生成的临时段均映射到原块边界。纯公式格的树停靠先校验预览页与端点，再用同一公式位置映射回已提交布局。回归让 120 行预编辑实际推动公式换页并验证续拖。
- 单元格命中区与正文命中裁剪共用网格投影。画布每页缓存命中区和停靠点，起点归属只查询一次，位移使用同代缓存。对照原先每次位移查询 Rust 的方案，用两格命中图模拟一次按下、两格各 500 次位移。归属查询由 1001 次降为 1 次，页命中图读取 1 次、选区提交 2 次。此对照统计调用次数，桌面延迟未测。
- 格内块环境在上下键触及边缘时沿用块外侧停靠，再按同列换格，标题自身也可向上退出。Equation 与 Align 的文本边缘入口和通用块入口复用同一公式落点解析。只含显示公式的格通过公式的块边界参与整表拖选。
- 跨页整表选区以页面绘制的外框逐页核对四条边。Qt 局部替换用例直接构造两段文档，检查相邻段落、选区清空和交互通知。

保存冲突与恢复协调的分层优化记录在 [037](037-autosave-implementation.md#2026-11-28-恢复协调下沉)。

| 2026-11-28 分支本地验证 | 结果 |
| --- | --- |
| `cargo test --locked --offline --workspace` | 2126 项通过，1 项忽略。原生 CTest 20/20。 |
| Qt Cocoa 实际窗口 | 虚线刷新、不等高格拖选、纯公式格双向拖选和行列快捷键 4 项通过。 |
| `cargo coverage` | Rust 产品源码函数 86.15%，行 89.06%，通过 85% 门槛。 |
| `cargo coverage native` | CTest 20/20。原生产品源码函数 91.36%，行 86.58%。LLVM 仍报告 4 个函数的覆盖数据不匹配。 |
| `cargo lint --check` | 9/9 通过，含 Rust、C++、QML 与 CMake 检查。 |
| `cargo format --check`、文档链接与 `git diff HEAD --check` | 通过。 |

分领域检查和独立终审比对 `29d3c1a` 以来的工作树，复核确认预编辑同代与锚点、纯公式格命中、标题反向出口和恢复失败清理。整套变更的最终复核同时覆盖 [012](012-impl-table.md) 的换行片段与 [084](084-document-content-shapes-and-boundaries.md) 的行内形状回归。

### 后续窗口验收

- `tableThemeTokensRenderAcrossZoomAndWindowFocus` 在 Cocoa 中验证两套注入主题 token 经实际绑定绘制到块背景和虚线。缩放 100% 与 200% 均检查单边上的虚线段间隙。两个实际窗口切换时观察画布几何，重新激活后确认原单元格、caret 和虚线绘制恢复。
- `tableGuidelineUsesTheVisiblePageFragment` 打开 70 行 `.cdoc`，滚到第二页并实际点击同代命中图的格心。快照只为第二页发布当前格的辅助线，完整 PageSurface 帧验证该页单元格的虚线。软件后端用完整视口抓帧，避免窗口截图只观察到部分重绘内容。离屏组合复跑连续 3 次通过。
- macOS Cocoa 的最终定向组合通过 7 项，包含两项表格窗口验收、换格虚线刷新、不等高和纯公式格拖选、物理行列快捷键及空公式缩放。窗口测试使用 Qt Quick 软件渲染。
- [088](088-unify-empty-inline-math-hit-slot.md) 将空公式命中和高亮裁到同一单元格内容区，窄版心中点击相邻空格不会进入公式。独立终审已复核槽位、单边虚线判定与分页抓帧。
- 窗口验收时工作区测试通过 2128 项、忽略 1 项，原生 CTest 通过 20/20，`cargo format --check` 通过，`cargo lint --check` 通过 9/9。
- 窗口验收时产品覆盖率通过门槛。Rust 函数 86.16%、行 89.07%，原生 C++ 函数 91.59%、行 86.70%。原生覆盖率构建的 CTest 也通过 20/20，LLVM 仍报告 4 个函数的覆盖数据不匹配。
- `tablePaintUsesBuiltinThemeColors` 读取 Rust 内置主题目录，在同一张表切换浅色和深色。旧浅色虚线 `#6f6f6f` 与实线 `#707070` 在抓帧中难以区分。调整为深灰 `#383838` 后，两套主题均在完整 PageSurface 帧中绘制整表底色和同一边上有间隙的虚线。测试在旧配色下复现失败，修复后经离屏及 Cocoa 通过。

### 格内内容横向裁切

- 行内、Equation 和 Align 的同页公式投影携带单元格归属。应用层从投影读取格 ID，用于拖选域和停靠身份。
- 超宽公式的树与文本岛命中、数学选区和绘制消费内容区横向裁切。显示公式仍提供可进入整块选区的两端停靠，坐标收进可见内容区。
- 正文字形、公式字形、装饰和数学图形的绘制数据携带页面局部裁切区。画布把 x 和 y 与当前页取交集，再交给纹理及形状绘制。
- 点击格内边距仍由所在格处理，返回的正文 caret 与交互快照采用同一内容区裁切。数学字素只有一部分可见时，拖选停靠点落在可见边缘，双向拖选仍提交完整字素。
- 格内显示公式外沿通过共享 `BlockEdge` 参与结构选区。与邻格或表外行内公式双向拖选时，由会话归一表格范围，并保留表外公式的局部端点。
- MathText 停靠携带预编辑 revision，旧端点保留当前预编辑。有效端点在取消正文预编辑时按公式路径重投影到已提交页，并把原锚点交给 QML 延续拖选。回归用例实际把两个端点推到后续页，再逐字段核对取消后的原锚点。
- engine 回归覆盖超宽行内、Equation 和 Align 中的 Tree 与 TextFlow；Qt 像素用例验证字形、装饰、填充矩形和线段在裁切区内外的实际绘制。
- 对比 `29d3c1a` 的独立领域检查与终审确认裁切 wire 兼容、预编辑停靠点验代和跨宿主范围。工作区测试通过 2132 项、忽略 1 项，原生 CTest 通过 20/20，`cargo format --check` 及 `cargo lint --check` 通过。
- 内置主题回归加入后，产品覆盖率继续通过门槛。Rust 函数 86.17%、行 89.11%，原生 C++ 函数 91.59%、行 86.88%。覆盖率构建的原生 CTest 通过 20/20，LLVM 报告 4 个函数的覆盖数据不匹配。
- 此阶段 Cocoa 通过画布像素测试及其余 7 个表格交互用例。锁屏时两窗焦点用例停在窗口激活断言，解锁后的复跑见下节。Cocoa 窗口测试使用 Qt Quick 软件渲染。

### 外侧光标与纵向扩选复查

- 表格前后光标按首末可见网格行垂直居中。前侧保留左边线，后侧保留右边线，跨页沿用对应表格片段。
- 布局确认段落已到首末视觉行后，由 Session 取相邻 Table、Equation 或 Align 的远侧 `BlockEdge`。Shift+↑／↓ 一次跨完整块，保留原锚点。继续扩选使用相邻段落的视觉行。
- 从块外侧朝外移动时，Session 先解析同级相邻块的真实入口。普通上下键从该块外侧进入导航序列，Shift 按完整块扩选。相邻表格逐个纳入范围，以表格为入口的块环境沿用子块停靠和完整环境选区。
- 对已覆盖的块反按 Shift+↑／↓ 时，Session 按块边界收缩。双向用例各按一次撤出整棵环境，前一张表、原锚点和环境近侧焦点保持一致。
- 越过表格外框时，候选行按页面及片段裁切后的范围参与排序。空段落的单一插入点和超高行中裁掉的文本不会充当不存在的相邻行。单行单元格内的上下扩选保持原位置。
- 回归分别复现了外侧光标贴顶／底、相邻段落扩选逐字、格内扩选误走横向以及超高表格行挡住跨页。用例同时检查折行段落、分页光标、公式反向收缩和 Qt 焦点下真实键事件。
- 解锁后在 Cocoa 通过实键中心高度及整表扩选、双窗焦点恢复、70 行表格分页辅助线三项用例。实窗验证使用 Qt Quick 软件渲染。
- 本次工作区测试通过 2138 项、忽略 1 项，原生 CTest 20/20。领域复核和独立终审确认了相邻表格及环境表格入口的双向回归。

### 限制

- 跨格拖选以整表为结构单元。矩形单元格范围仍待实现。整表与格内 Blocks 片段的剪贴板由 [012](012-impl-table.md) 验收。
- 从显示公式的 MathText 文本岛内部发起的拖选沿用岛内局部选区。纯公式格的结构区域与空白区域支持跨格拖选。
- 应用间前后台切换、其他平台真实窗口及各系统输入法候选界面优先级仍待验收。
- 表格的超高行仍以原子行分页。内容超出单页时按现有版心裁切规则绘制。
- Session 对嵌套表格和含格内分页符或不透明块的模型执行了选区、导航回归。这些结构仍受 [046 的桌面排版限制](046-table-layout-and-canvas.md)约束。

## 2026-11-22 合入前评审

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
- 2026-11-22：命令面板 `\table` 在 cell 内候选不可用时接受路径补引擎回归（不报错、generation 不变）。
- 2026-11-22：终审修复：跨 Story 表候选在表格不直接相邻（如 `[正文, Equation, Table]`）时回落到既有复杂块边缘逻辑，避免水平方向键被吃掉；表格行列插入/删除键在表格外统一 no-op，消除其余 `edit.invalid_position` QML 警告；同步 canvas-highlighting/structure-navigation 唯一权威文档。
- 2026-11-22：按维护者截图反馈继续修复：block 高亮从当前 cell 改为整张表格（所有 cell 完整奶油黄）且边框保持实线；当前 cell 仅保留网格外框虚线；方向键在表格边缘按块宿主离开/进入相邻正文段落，Escape 从 cell 退出；普通/Shift 方向键与结构导航键的跨 Story 表格候选统一收敛（扩选 no-op、普通移动进入相邻格），消除 `edit.position_story_mismatch`；不可用表格插入键 no-op，消除 `table_insertion_unsupported`；native 轴线按设备像素对齐，修复 Enter 后边框粗细不均。补 engine/app/native 回归。
- 2026-11-22：领域评审三轮修复：方向键 cell 目标确实改用共享 `focus_table_cell` 提交；补 app 非空目标 cell Up/Down 回归。纵向 app phase1 只由视觉边界触发，`table_vertical_block_boundary` 仅用于 phase1 后早退；折行 cell 纵向文本行导航恢复并补 app 回归；方向键目标提交改用共享 `focus_table_cell`；补正文空段纵向 no-op 测试。
- 2026-11-22：领域评审修复：vertical app phase1/phase2 双提交收敛（表格边界消费一次、失败不改焦点）；空 paragraph 纵向边界识别；单元格内多段 Enter/Left/Up 首末段落点统一；环境内表格允许方向键换格；删除调试输出；app 测试改为断言目标行/列 x/offset/末行 no-op，并补 wire 行列命令用例；几何测试断言表格外框/规则线而非仅相对关系。
- 2026-11-22：表格 cell 外框改为 `TableGridLayout` 网格矩形单点派生：同一矩形进入共享 block 高亮层和 `tableGuidelines`（含边框/内边距拆分后的对称左右边界），修复 2×2 左右起始不一致与辅助线内容盒内缩；新增 engine 对称几何/共享高亮测试、app 切换 cell 高亮/辅助线替换测试、C++ native 指南线整体替换测试（红色像素验证旧矩形清除）。
- 2026-11-22：方向键单元格边界移动：单元格首/末文本边界按行优先进入相邻 cell（上下同列），表格边缘消费为 no-op；新增 2×2 行为测试。
