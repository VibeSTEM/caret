# 080 — Block/Inline 共享交互底座（Paragraph 为基线）

- 状态：in-progress
- 依赖：071、074、077
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-09

## 目标与背景

当前多个 Block 形态分别处理焦点位置、导航、删除和选区，造成相同交互出现不同边界行为，也让修一个公式问题容易演变成逐类型补丁。维护者要求将 Paragraph 基线、复杂 Block 及内部 Inline 内容纳入同一个可扩展交互设计，并统一位置、删除和选区管理。

**Block 是统一的外层宿主类别，Paragraph 是其中结构最简单、规则最完整的一种 Block。** Section、Equation、Align，以及未来的 Table、Figure 都是其他 Block 形态，内部结构可不同。**Inline 是可复用的内层内容结构**，可被 Paragraph 及显式提供 inline-capable 流的 Block 承载。Inline 不直接插入任意块序列。

三类已报告问题作为本任务的起点：

1. 删除一个块级数学环境后，焦点恢复会越过相邻数学块，落到更早的 Paragraph 末尾。删除应只移除目标宿主，并落在删除位置最近且仍有效的边界。
2. 数学块邻接 Section 时，在块边界按 Backspace 会报告 `error:text.delete_backward: edit.invalid_position`。操作成功或安全拒绝都不能产生非法焦点。
3. Shift+方向键不能可靠选中块级数学环境或行内 `InlineMath`。选区锚点、焦点、结构范围、替换/删除与几何必须保持一致。

080 是一次共享底座改造，不是三个缺陷的独立补丁。实现应建立统一的 Block 交互协议，由窄适配器描述不同形态的合法位置、内容事务和布局几何；不同 Block/Inline 载体不各自复制仲裁器或平行焦点状态。

## 权威设计

- [文档树与结构](../architecture/editor/document-model/document-structure.md)：Paragraph、Block、Inline 的持久化层级与合法容器。
- [块宿主交互](../architecture/editor/document-model/block-interaction.md)：Paragraph 基线、共享位置/选区/删除协议及 Block 扩展。
- [环境视图](../architecture/editor/document-model/environment-views.md)：活动环境候选、载体映射与高亮来源。
- [文档位置](../architecture/editor/document-model/document-position.md)：运行时位置与跨宿主位置映射。
- [结构导航](../architecture/editor/structure-navigation.md)：结构索引、方向键、边界停靠和焦点几何。
- [画布高亮](../architecture/interface/canvas-highlighting.md)：活动 Inline/Block 高亮及空范围槽位几何。

以上架构文档维护长期有效的设计；本任务只维护实施步骤、验收和验证结果。

## 范围与非目标

- 以 Paragraph 已验证的文本行为为回归基线，提取统一 Block 位置/焦点/选区协议，覆盖 Paragraph 文本位置、复杂 Block 前后边界、领域内部位置、anchor/focus、文档顺序、事务、Undo/Redo 和失败原子性。复用行为不等于复用 `paragraph_id`/UTF-8 坐标或把它变成其他 Block 的位置类型；不能用 Paragraph 零长度位置伪造复杂 Block 边界。
- 审计本改造涉及的文档模型字段、Session 焦点/选择状态和位置映射：移除经构造、读写、序列化与跨层调用检查后确认冗余或无消费者的字段/状态；对未使用但支撑明确扩展能力或持久化契约的字段记录理由并保留。不得为减字段削弱稳定 ID、未知字段、安全校验或扩展性。
- 为当前 Section/Custom/Equation/Align 实现共同 Block 接入；各适配器只提供稳定 ID、父级路径、顺序、内部位置、合法编辑范围和同代几何。Section 是含 Paragraph/Block 子项的容器，不把它误当作逻辑行；Equation/Align 保留各自数学内部编辑模型。普通 Paragraph 不因共用 Block 协议而增加虚拟块停靠点或块背景高亮。
- 让 `InlineMath`、`InlineEnvironment`（Bold/Italic/Font 等）及数学文本流中的 Inline 能通过同一 Inline 范围、光标、选区、删除和几何底座工作。InlineMath 保留 Paragraph/TextMap 中 U+FFFC 原子外边界，进入公式后使用 MathTree/MathText 坐标；不创建块级停靠状态。
- 统一 Shift+方向键的跨宿主扩选，将 InlineMath 和 Block 数学节点纳入可操作的文档结构范围；选区可用于删除、替换和复制，禁止 U+FFFC 泄漏或静默丢失已选数学对象。
- 统一空 Inline 的活动高亮槽位。空 InlineMath、真实空 Bold/Italic/Font 环境与空光标处启用的 PendingInlineStyle 使用同一粉色占位几何策略；PendingInlineStyle 关闭或光标移动后槽位消失。槽位来自同代布局，不插入文档字符或空环境节点。
- 修复连续 Equation/Align 删除落点与 Section 邻接 Backspace；显式 Escape 离开公式仍保留跳过公式链、寻找正文的既有行为。
- Table/Figure 只定义未来接入共享 Block 协议的扩展点，不在 080 实现表格或图形节点、单元格编辑或完整网格选区。
- 不改变 079 已定义的 Align 逻辑行退格规则：空逻辑行删除，非空逻辑行行首 Backspace 无操作。Section 的首个正文 Paragraph 仍遵循 Section 标题合并语义。

## 现状调查与设计约束

- `caret-doc::Block` 枚举当前包含 Paragraph、Equation、Align、Environment、ExplicitBreak 和 Opaque；不是每种枚举项都是相同的可编辑宿主。Paragraph 的文本链路最完整，但 `Pos`、`PositionMap`、文本历史映射以 Paragraph/UTF-8 为中心，只能作为行为基线，不能直接当作所有 Block 的位置表示。
- 普通文本路径目前由 `Session::selection`、`PositionMap`、`ParagraphIndex`、逐键导航及 `text_edit.rs` 的字素删除/段落合并组成；Paragraph 与数学 TextFlow 已通过 `flow_edit.rs` 共用较低层的文本、Run 和范围编辑原语。MathTree/MathText 继续需要树路径与宿主事务适配。
- Session 目前并列保存普通 selection、math_edit、block_boundary_exit、selected environment 与 structure_selection。Section 转移位于 `structure.rs`，数学块边界转移位于 `math_edit.rs`；共享了部分边界状态字段，但仍有分开的邻接解析和重复块路径访问。结构索引及 app 视觉候选尚未覆盖统一 Block 顺序范围，数学块因此可能被相邻 Paragraph 查询跳过。
- 公式删除目前复用了显式 Escape 所用的跳过相邻公式 helper；删除后段落锚点逻辑还存在从索引任选 Paragraph 的兜底。两者须被共享宿主位置解析替换，Escape 的既有语义保持不变。
- 结构选区删除的既有 Undo/Redo 契约现由[块宿主交互](../architecture/editor/document-model/block-interaction.md)维护；080 必须按其恢复普通插入位置与 affinity，但不恢复结构高亮。单个公式对象删除的 MathEdit 恢复由[数学公式架构](../architecture/editor/math.md)维护。
- 模型字段审计未发现可安全直接删除的持久化字段。Paragraph 的语言/布局/样式/props/extra、环境 definition_id、节点 ID、Opaque 与 schema version 都有布局、模板、未知字段、稳定定位或校验消费者；不能因单一渲染路径未读取就删掉。
- `Session::selected_block_environment` 与 `selected_block_environments` 在通常选择状态重复保存同一选择，但块外删除空环境时当前只设置前者。低风险方案是让删除函数显式接收目标 ID，再由列表派生单值 getter；保留现有 getter 形状，并覆盖单选、多选、块外删除和失败回滚。
- `current_inline_environment()` / `InlineEnvironmentRange` 目前仅由仓库测试消费，但它们是公开 API。只有确认外部嵌入使用和候选排序语义后才可移除或用 `CurrentEnvironmentView` 兼容；外部消费者无法仅由仓库调用搜索确认。
- `selection`、`structure_selection`、`math_edit`、`block_boundary_exit`、`Affinity`/`VisualAffinity`、`StoryId` 和数学 projection identity 各自承载不同位置、Story 或历史语义，不能按字段相似直接合并。数学历史恢复存在同一 projection identity 的重复比较，可在保留历史合并与 projection 重绑行为的测试下评估简化。
- 已确认 `Session::delete_math_object` 误用 `paragraph_beside_math_chain`：该 helper 跳过同级 Equation/Align，遇到 Section 返回 None。保留它用于显式 Escape；删除应使用统一宿主边界解析。
- 已确认 `StructureIndex` 当前仅为 Equation/Align 建零长 paragraph ordinal 范围；应用层相邻段落候选会跳过数学块，现有选区/高亮/删除也不能完整表达数学块。只修改方向键 handler 不足以修复。
- MathTree/Source 的 Shift 水平移动、MathText/MathTree 的部分扩选候选当前无操作，MathTree 复制缺少完整契约。必须先统一跨宿主 selection range，再接入导航、几何与编辑命令。
- 当前 `InlineMath` 是 Inline 原子项，不是格式环境；空 InlineEnvironment 也缺少活动范围的非零几何。两者要共享 Inline 底座和空范围策略，但保留 TextMap、MathBox 各自的几何来源。空选区启用 Bold/Italic 只写 `PendingInlineStyle`，不创建空环境；它仍应投影同一粉色 caret 槽位，但必须保持为无文档写入的独立视图状态。
- 所有状态转换必须有唯一权威；适配器不能各自再持有 boundary exit、selection、删除落点或快捷键优先级副本。若某一宿主坐标无法无损映射，保留原文档与有效焦点并原子拒绝，不猜测跳转。

## 实施步骤

1. 添加三类问题的可复现行为测试，记录输入位置、按键序列、预期 anchor/focus、结构范围和文档/历史状态。
2. 梳理 Paragraph/复杂 Block/Inline 的现有位置、顺序、删除、选择与几何实现，标出重复状态及可复用入口；盘点相关模型字段及 Session 状态的构造、序列化、读写和跨层消费者；先确定共享协议和适配器契约。
3. 以 Paragraph 已有行为和测试为基线，提取共享宿主位置解析与状态转换；保留 Paragraph 自身 TextMap/位置映射，把 Section/Custom/Equation/Align 接入统一 Block 边界、移动、删除和历史协议。移除旧路径中重复的权威状态及不再使用的 helper。
4. 接入跨宿主结构选区：索引与顺序范围、四向扩选、同代高亮几何、删除/替换/复制；使 InlineMath 使用 Inline 原子范围而非 Block 停靠。
5. 统一空 Inline 高亮槽位，将 Bold/Italic/Font/InlineMath 接入同一 Inline geometry helper，同时保留载体特有的范围或 MathBox 投影。
6. 更新权威架构文档，检查实现与契约一致；运行相关行为验证，执行 Terra/medium 领域检查并修复，最后由未参与实现的 Sol/medium subagent 对最终完整 diff 做只读评审。

## 预计改动

预计涉及 `caret-engine` Session 的宿主位置、焦点/选区与事务协调，`caret-structure` 顺序和跨宿主范围，`caret-layout` 选区与 Inline 槽位几何，`caret-app` 同代导航候选/快照投影/剪贴板适配，以及跨层行为测试。Qt/QML 不重写位置或选区仲裁；只有 Rust 快照缺少必要输入/几何时才扩展视图接口。新增模块遵守仓库文件结构及源码署名规范，不为缩短文件而新建 crate。

## 验收标准

- [ ] Paragraph（Block 的简单变体）与 Section/Custom/Equation/Align 共用一个位置仲裁、选区、删除和历史恢复底座；Paragraph 既有行为不退化，复杂 Block 的不同坐标/内容模型仅通过明确适配器提供，不存在平行权威状态或逐类型补丁。
- [ ] Block 边界使用稳定宿主身份及前/后位置；连续 Block 之间的停靠状态可分别到达，删除不跳过仍存在的相邻块去寻找 Paragraph。
- [ ] 数学块与 Section 标题、首个正文段落、嵌套子块相邻时，Backspace 不产生 `InvalidPosition`；安全拒绝也保持文档、焦点、选区、generation 与历史不变。
- [ ] Shift+Left/Right/Up/Down 可从数学块边界扩选 Equation/Align，也能从两侧扩选 InlineMath；反向 anchor/focus、跨 Paragraph 和跨连续 Block 的范围一致。
- [ ] 从 MathText 内部扩选时先遵循数学文本岛语义；越过其宿主边界后，以结构范围包含数学对象且保留正文锚点。
- [ ] 选区高亮、命中、caret stop、anchor/focus 来自同一代布局/文档快照；数学结构选区能正确删除、替换和复制，不泄漏 U+FFFC、不静默丢失节点。
- [ ] InlineMath 外侧仍是 TextMap 的两个 U+FFFC 邻接位置；Bold/Italic/Font 与 InlineMath 共用 Inline 选择/删除/高亮底座。真实空 Inline 与空光标启用的 PendingInlineStyle 均显示非零粉色槽位，关闭 pending 格式或移动 caret 后槽位消失；全程不写入空环境节点或文档字符。
- [ ] Section/Custom 与 Equation/Align 接入共同 Block 协议；Align 逻辑行退格与 Section 标题/首段落规则保持各自已定义语义。
- [ ] Escape 保留原公式链跳过契约；删除路径不再误用该 helper。对结构选区删除，Undo 恢复有效普通插入位置与 affinity，但不恢复结构高亮；Redo 恢复删除后的折叠位置。单个公式对象删除仍按[数学公式契约](../architecture/editor/math.md)恢复 MathEdit 目标、焦点与视觉 affinity。
- [ ] 新 Block 类型的接入契约明确要求复用公共协议；本任务没有提前实现 Table/Figure。
- [ ] 080 涉及的持久化字段和运行时状态均有消费者/扩展职责证据；确认冗余的字段已简化，确认仍需保留的前瞻字段有明确契约和理由，未因字段精简破坏稳定身份、序列化安全或扩展能力。
- [ ] 相关行为测试、格式/lint 和独立评审通过；窗口或平台手测如未执行，明确记录而不写成通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Equation/Align 连续块删除、Section 邻接 Backspace 行为测试 | 有效焦点且只处理目标结构 | 未执行 |
| — | Shift+四向键跨 Paragraph、复杂 Block 与 Inline 选区测试 | anchor/focus、结构范围和高亮一致 | 未执行 |
| — | 相关 Rust 测试、格式/lint、Terra 检查、Sol final review | 修复符合共享交互契约且无回归 | 未执行 |

## 风险与回退

共享位置映射会影响普通段落、Section 标题语义和连续块间导航。每个转换都必须以稳定结构位置及父子关系解析，覆盖无相邻 Paragraph、连续块、嵌套 Section、反向选区和事务失败。若模型暂不能表达某个边界，应原子拒绝并保留有效状态，不回退到随意选取一个 Paragraph。

## 决策与工作记录

- 2026-11-09：创建任务。维护者报告块数学删除落点跳过相邻数学块、Section 前退格产生 `edit.invalid_position`、Shift+方向键不能选中块级和行内数学。
- 2026-11-09：Terra/medium 只读领域审查确认：删除分支误用仅供 Escape 的公式链跳过 helper；Section 侧无相邻 Paragraph 时在 mutation 前失败。Shift 选区还缺结构索引与布局几何支持。
- 2026-11-09：维护者明确总体模型：Block 与 Inline 是两层内容结构，Paragraph 是结构最简单、规则最完整的 Block；Table、Figure、Align、Section 等是其他 Block；Inline 可位于 Paragraph 或显式支持行内流的 Block 内部。080 扩展为统一位置、光标、删除、选区与几何底座的大型改造；Paragraph 行为作为回归基线，其他载体通过窄适配器保留真实差异。

- 2026-11-09：维护者确认空光标处启用 Bold/Italic 后也显示粉色槽位，移动或关闭格式后消失；槽位是 PendingInlineStyle 的视图，不创建空环境节点。
- 2026-11-09：本 PR 仅更新、拆分并校准架构文档与 Task 080；代码改造留待 080 后续实施，不在本 PR 修改。

## 完成摘要

未完成。
