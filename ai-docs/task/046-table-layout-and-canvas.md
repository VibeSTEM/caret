# 046 — M3：表格布局与画布

- 状态：in-progress
- 依赖：027、043、044、045
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-22

## 目标与背景

把 045 的表格模型与现有 Rust 布局、Qt 画布连接起来，使网格、单元格内容、点击命中和键盘焦点使用一致的单元格几何。负责表格布局并接入已有 MathBox；044 提供正文公式布局能力，本任务负责把公式几何组合到单元格区域。总体验收见 [012 M3 表格](012-impl-table.md)。

## 必读

- [表格](../architecture/editor/tables.md)、[页面画布与命中测试](../task/027-canvas-and-hit-testing.md)、[段落排版](../task/026-paragraph-layout.md)
- [分层](../standards/layering.md)、[数值规范](../standards/numeric-values.md)、[测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

实现表格行/列/单元格的尺寸计算、网格绘制快照、单元格内容布局、分页/裁剪、命中测试与焦点几何，并在单元格区域组合 044 的数学快照。单元格内容保持通用块序列；布局快照须在单元格区域内承载 044 的公式几何，并提供稳定坐标变换与裁剪边界。

不实现公式解析或复制 MathBox 排版策略，不让表格布局模块直接依赖 `caret-math`；engine 布局编排复用 044 的公式快照，并提供单元格坐标变换、裁剪、命中与光标几何。对完成列宽求解和内容重排后的测量结果，优先复用 `caret-grid-layout` 的纯几何定位；该 crate 不负责列宽策略、内容重排或分页。共享行框算法也由该 crate 提供，正文表格和数学宿主消费同一基础行度量。Equation 作为单行单格、Align 作为多行多格，复用同一网格装配与行列几何；嵌套数学矩阵保留数学自身策略。编辑态使用白色虚线显示行列分割辅助线，以区别于持久化边框；虚线是导航与焦点导引，不写入文档边框样式。不实现单元格合并，也不承诺表格级增量性能门槛。

## 前置条件与待决策

045 表格结构、043 数学编辑能力、044 正文数学布局能力、027 画布命中能力和 071 宿主焦点/键盘意图协议已完成。045 交接给本任务的前置：① engine 的显示/行内公式块定位、相邻公式查找与组合公式定位仍沿环境链实现，必须统一改为容器路径感知，才能接单元格内公式编辑/导航（045 对 cell 内公式编辑显式返回 `edit.table_cell_math_unsupported`）；② `\table` 命令候选与 native/QML 表格键位保持禁用，画布就绪后统一启用；③ 整表复制/结构范围删除与单元格内多段合并当前显式拒绝或未接入；④ `caret-structure` 的 `block_list_by_table_cell(_mut)` 暂无生产消费者，收敛共享 locator 时决定删除或落地。采用的列宽/行高默认及页面溢出行为应符合现有段落排版契约，不在此任务新定义分页语义。

## 实施步骤

1. 计算单元格可用矩形、边框/网格和内容起点，使用统一整数度量。
2. 布局单元格中的多个段落和嵌套块；跨页片段保留单元格/块路径映射。
3. 将鼠标命中和焦点导航映射到同一单元格布局数据，确认滚动、缩放与分页后坐标一致。
4. 复用 044 的 MathBox 几何并放入单元格坐标/裁剪范围；命中、选区、公式内光标与高亮使用同一映射，不把公式排版策略放入表格模块。
5. 为 Equation 单格、Align 多格及普通文档表格复用 `caret-grid-layout` 行列装配；行框使用调用方提供的正文基准度量，行身份和裁剪范围贯穿绘制、命中与 caret。数学宿主不得另建专用行高实现。
6. 绘制编辑态白色虚线行列辅助线；测试不同焦点、缩放、背景与高对比度下的可见性，并确认不改变保存格式。
7. 补多列宽、多段、空单元格、跨页、命中和缓存行为测试；记录性能基线，不在没有数据时设时限门槛。
8. 更新 012、027 关联说明，独立架构与代码评审后复验。

## 预计改动

`crates/caret-layout` 表格布局；现有 Qt 画布/FFI 快照仅添加承载表格几何所需字段。具体文件边界按仓库拆分准则确定。

## 验收标准

- [ ] 网格和单元格内容跨页正确显示；单元格多段内容、长内容和空段布局可验证。
- [ ] 点击、拖选起点、键盘焦点与滚动/缩放后的命中共用同一几何结果。
- [ ] 单元格内 InlineMath/Equation/Align 的布局、绘制、命中、光标和高亮与单元格坐标、分页及裁剪一致；表格基础布局不依赖 `caret-math`。
- [ ] Equation、Align 和文档表格复用共享行列装配；编辑态分隔辅助线为白色虚线，不持久化为边框。
- [ ] 页面 generation 变化时旧的表格命中/绘制快照不会覆盖新结果。
- [ ] 行为测试、跨层构建/lint 与独立评审通过；性能报告记录实际输入与配置。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Rust 表格布局测试 | 单元格尺寸及页片段几何稳定 | 未执行 |
| — | Qt 画布命中/焦点验证 | 绘制和输入定位一致 | 未执行 |
| — | 多段与跨页夹具 | 内容不重叠、不丢失，映射稳定 | 未执行 |
| — | 044 接口检查 | 公式快照有明确定位及裁剪信息 | 未执行 |

## 风险与回退

表格与段落基线/分页耦合较强。复用现有 PageFragment、段落布局和 hit-test 契约，避免在 Qt 重写坐标或在表格里复刻段落算法。若分页复杂度超出首版，保留完整文档模型并限缩布局行为，不序列化成图片。

## 决策与工作记录

- 2026-11-08：045 引入宿主焦点协议前置；由 045 依赖 071，间接包含 073 公式内容保全，不重复登记依赖边。
- 2026-11-09：将数学宿主的表格式行列组装、正文基准行高和编辑态白色虚线分隔辅助线纳入本任务；共用几何落在已有 `caret-grid-layout`，不新建转发 crate。
- 2026-10-29：由 012 拆出表格的布局和画布职责；表格模块与公式 backend 分离，engine 复用 044 几何并在单元格坐标内组合。
- 2026-11-22：045 已合并到 main（PR #167）。开始实现（分支 `yuki/task-046-table-layout`）：先审计现有页面/段落布局、`caret-grid-layout` 行列装配、引擎快照与 Qt 画布命中链路，确定表格布局 slice 与 shared 行框复用；按“单元格几何/内容布局→绘制与命中→caret/选区→分页/裁剪→用户入口（\table、native 键位与 cell 公式定位器）”的纵向顺序推进。
- 2026-11-22：S1a：`caret-layout::table` 表格网格几何（等分列宽、行高/首基线、单元格内容盒、最小行度量），复用 `caret-grid-layout`，7 项测试。
- 2026-11-22：S1b：分页协议扩展（`FragmentClip`、段落/公式片段 x/clip、`TableFragment` 行范围/原点/完整网格），`PaginationItem::Table` 行原子分页 + 续页负原点；3 项分页测试。
- 2026-11-22：S1c：engine 接入 Table 布局（段落宽度进入塑形/复用键、cell 表格测量与网格、分页展开 cell 行位置与片段、page_glyphs 输出 cell 文本与表格边框）；hit/caret/range 使用同一行放置几何；`\table`/Alt+T/Shift+Tab/行列键/删除行列 native 意图、app 映射与 codec `table` kind；engine 954/app 354/workspace/clippy 通过。当前限制：cell 内公式/环境/嵌套表布局、cell 公式编辑/导航、整表复制与跨 cell 选择未接，遇到显式拒绝。
- 2026-11-22：架构审计（只读完整 diff）后完成 Gate0/1 收敛：表格几何单点持有（`snapshot.table_layouts`，页面片段只存放置引用）、规则线由 `caret-layout::table` 统一生成并供 painter 读取、表格测量/展开拆入 `layout/table_layout.rs`、塑形参数请求化并删除 clippy allow、默认样式集中；修复表宽超版心、边框线坐标、cell 行内公式布局拒绝、表格插入单一谓词（标题/单元格内拒绝）。engine 956/caret-layout 160/workspace/clippy 通过。
- 2026-11-22：Gate2 基础：容器定位器下沉 `caret-structure`（`BlockContainerPath`/`TableCellPath`/`block_list_by_container(_mut)`）；`DocumentNodeLocation.container` 记录环境+表格单元格真实容器；`find_block_container_path` 与 `find_block_by_id`、`find_inline_math_in_blocks` 改为容器感知，cell 内块/行内公式可解析；engine 删除私有容器解析副本，遍历状态收敛。结构测试 15/engine 957/workspace/clippy 通过。剩余：`find_block_path` 兼容投影迁移（entry_exit/range_extension/history/clipboard）、layout `LaidOutParagraph` 改持容器路径。

## 当前状态与后续切片

已完成 S1（可插入→布局→绘制→命中/光标，命令面板与 native 表键入口）：`caret-layout::table` 网格几何、分页表格片段、engine 表格测量/放置、cell 段落行位置与矩形裁剪、page_glyphs 文本与边框、hit/caret/range 同源几何、`\table` 与 Alt+T/Shift+Tab/行列插入/删除行列意图、codec `table` kind。engine 954、caret-app 354、workspace 测试与 clippy 通过。

剩余（按优先级）：
1. **单元格内公式布局与编辑/导航**：`shape_table` 目前对 Equation/Align/InlineMath 单元格或环境/嵌套表返回 `layout.unsupported_content`；需先把 `find_block_path`/`find_inline_math_in_blocks`/`paragraph_beside_block`/composition 公式定位统一为容器路径（共享 `BlockContainerPath`），再开放 cell 内公式编辑（移除 `edit.table_cell_math_unsupported` 门禁），并补 `collect_math_hosts`/`collect_math_projections` 的 cell 递归、命中/高亮/选区矩形裁剪。
2. **表格几何快照与编辑辅助线**：向 app/native 暴露表格片段/单元格矩形（命中可选），在布局快照中加入 `tableGuidelines`（焦点相关虚线，主题 token + QSG 虚线层），完成白色虚线在浅色主题的可对比策略（需维护者确认产品意图）。
3. **整表复制/结构删除与跨 cell 选择**：结构范围对象、剪贴板投影与文本回退；当前仍显式 `edit.table_content_unsupported`。
4. **单元格环境/嵌套表/ExplicitBreak 语义**：环境标题/子块布局、嵌套表（模型允许 24 层）、cell 内分页符策略；当前显式拒绝。
5. **分页细化**：行原子分页已可跨页并记录负原点；单元格内多段跨页拆分、行高/列宽策略（auto/固定）、宽内容裁剪测试与性能基线。
6. **验证与收口**：native/Qt 键位与快照严格校验测试、真实窗口验收、Rust/C++/QML 测试补齐、领域评审 + 独立 final review、PR 与 CI。
