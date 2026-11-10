# 081 — Block/Inline 共享三态焦点仲裁

- 状态：done
- 依赖：071、074、077
- 优先级：P0
- 创建 / 更新：2026-11-09 / 2026-11-10

## 目标与背景

Block/Inline 边界与内部焦点目前分散在不同状态路径中；零长度 InlineEnvironment 还没有内部插入位置。为避免每种宿主增加一套边界状态，统一使用三类位置：**外前｜内部(宿主局部位置)｜外后**。

这是三种状态类型，不是固定三个 caret 坐标。内部位置始终由局部结构决定：单个空 InlineEnvironment/编辑槽只有一个位置，复合 Block 即使内容为空也可有多个 cell、标题或子宿主位置。Paragraph 作为最简单 Block，继续使用真实 TextMap 首尾位置，不增加虚拟外侧停靠。Section/Custom/Equation/Align 等复杂 Block 与 InlineEnvironment/InlineMath 共用边界仲裁，由窄适配器提供内部位置与几何。

## 必读

- [文档位置与限制](../architecture/editor/document-model/document-position.md)
- [块宿主交互协议](../architecture/editor/document-model/block-interaction.md)
- [结构导航](../architecture/editor/structure-navigation.md)
- [行内与块级环境视图](../architecture/editor/document-model/environment-views.md)
- [验证与评审标准](../standards/validation-and-review.md)
- [源码署名规范](../standards/source-attribution.md)

## 范围与非目标

- 为 Section/Custom/Equation/Align 与 InlineEnvironment/InlineMath 接入共同的外前、内部、外后三态焦点仲裁。
- 三态位置载体定义稳定宿主身份、宿主类别（Inline/Block）、状态类别（外前/内部/外后）和 adapter 局部位置。局部数学坐标是当前有效 Session/文档代际的即时投影；源码位置还依赖当前数学编辑上下文，不是持久或跨代 locator。Align Tree 位置必须保留行/单元格宿主身份。
- Block 的外前/外后停靠保持可到达：相邻正文水平键先到对应外侧，再进入内部或越过到相邻内容；垂直视觉候选可直接进入内部首/末位置。Paragraph 不增加虚拟外侧状态。
- Inline 从外前/外后进入内部起点/终点；若结构边界是合法字素边界，不跳过首末字素；若边界位于字素簇内，按合法字素映射规则进入/退出。InlineMath 使用 U+FFFC 外部位置，内部 MathEdit 位置继续由数学适配器管理。
- 环境边界与相邻文本共同组成扩展字素时，不创建字素内部 caret；进入/退出分别对称映射到环境内/外方向最近的合法字素边界，无法唯一映射时原子拒绝并保留焦点、选区与格式上下文。
- 可编辑空 InlineEnvironment 在合法字素边界具有唯一内部插入点；嵌套空环境在同一位置合成完整环境链，不为每层增加停靠点。空光标处 `PendingInlineStyle` 显示相同粉色槽位，但不创建文档节点。
- 不实现全树位置排序/locator（082）、跨宿主结构选区与删除/几何事务（083），也不实现 Table/Figure 内部编辑。

## 前置条件与待决策

- 071、074、077 的仓库内能力可用；077 的外部平台验收不是开始本任务的前置条件。
- 本任务必须确定可承载三态的宿主相对位置契约，并提供需要该身份的最小运行时载体；具体 Rust enum 形状不预先规定。完整父链、跨宿主全序、通用 locator 与代际映射由 082 在此载体上扩展，不是 081 的前置条件。

## 实施步骤

1. 为复杂 Block 和各类 Inline 建立边界正反向行为表，覆盖空/非空宿主、宿主首尾、相邻宿主及失败保持状态。
2. 在共享位置转换层表达携带稳定宿主身份的外前、内部、外后三态载体；各 adapter 只产出合法局部位置、首尾候选和同代几何。该宿主相对身份与状态载体在本任务落定，供 082 扩展文档全序。
3. 接入 Paragraph/Section/Custom/Equation/Align 与 Bold/Italic/Font/InlineMath，统一边界仲裁，不复制 stop/exit 状态机。
4. 让空 InlineEnvironment 内部位置可解析、可输入、可退出，并复用粉色槽位几何；`PendingInlineStyle` 保持独立状态来源。
5. 覆盖四方向、块外停靠、TextFlow/MathEdit 局部移动、格式继承和 Undo/Redo，不改变 079 的逻辑行退格语义。
6. 更新实现记录并按验证标准委派 Terra/medium 检查；修复后由未参与实现的 Sol/medium subagent 对最终 diff 只读评审。

## 预计改动

预计涉及 `caret-engine` Session 位置转换与键盘边界提交、宿主 adapter、TextMap/MathEdit 局部位置适配、同代 caret 几何及相关 Rust 行为测试。只复用现有 crate 内职责，不新建 crate、不改持久化 schema、不在 QML 另行推导状态。

## 验收标准

- [x] 所有具独立边界语义的 Block/Inline 共用外前、内部(宿主局部位置)、外后三类 carrier；不按 kind 复制边界状态机。
- [x] 三态 carrier 保存宿主 ID、宿主类别、状态类别和 adapter 局部位置；082 可扩展父链、跨宿主全序与代际映射。
- [x] 内部局部位置保留各自坐标：Paragraph/TextFlow 使用 Pos，MathTree/TextFlow/source 使用窄数学坐标；Align Tree 坐标包含 MathPositionHost 的 row/cell 身份。
- [x] 复杂 Block 外前/外后可独立到达；仅当边界落在同一可编辑间隙且插入目标一致时合并，共享候选进入有效内部位置；Paragraph 始终使用真实 TextMap 首尾。
- [x] InlineEnvironment 与 InlineMath 的外侧/内部切换使用同一方向转换分类；合法字素边界不跳过首末位置，候选按 generation 和局部位置校验。
- [x] 空 InlineEnvironment 在同一合法字素偏移有唯一内部插入点；嵌套/相邻空环境按稳定宿主 ID 维护环境链，不增加冗余停靠点。
- [x] 多个 Align cell 的空位置保留不同 row/cell 宿主身份；Section 等复合 Block 的内部结构仍由 adapter 表达。
- [x] 字素边界继续由共享 TextMap/grapheme 映射校验；当前的跨 Run/Environment combining、variation selector、ZWJ 用例确认 caret 不落入 cluster。
- [x] 空 Inline 内部槽位和待输入格式槽位发布非零粉色几何；输入继承格式，撤销/重做、输入法预编辑取消和边缘 no-op 保持有效焦点及历史。
- [x] 无效候选、过期代际、空环境兄弟切换、相邻公式切换和文档边缘无后继方向均有行为测试；失败/无操作不破坏选区、宿主身份或撤销合并边界。
- [x] engine/app 格式、测试、clippy 与 Terra/Sol 独立评审通过；外部平台验收不属于 081。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-10 | `cargo test --locked -p caret-engine` | 三态边界、局部宿主、失败原子性与历史恢复 | 703 tests passed；覆盖空 inline 内外、无效/视觉候选、三兄弟环境双向连续输入、Undo/Redo、输入法取消、Align cell 和公式相邻边界 |
| 2026-11-10 | `cargo test --locked -p caret-app` | 粉色槽位快照及文档视图回归 | 284 tests passed；含槽位非零几何与页面左右边缘 |
| 2026-11-10 | `cargo clippy --locked -p caret-engine -p caret-app --all-targets -- -D warnings` | 目标 crate 全 target 无警告 | 通过 |
| 2026-11-10 | Terra/medium 分领域只读审查 | 焦点迁移、跨层类型隔离与宿主身份 | 修复并复核：空 Inline 外侧先转到独立 Block 停靠；往返恢复邻接空 Inline 外侧；按精确宿主 ID 维护空兄弟输入；候选提交不覆盖新的宿主状态；无操作保留 host/Pos/history merge epoch；MathTree Align 位置含 row/cell ID |
| 2026-11-10 | Sol/medium 最终只读审查 | 审查最终完整 diff | 修复前向空兄弟连续输入、候选覆盖新宿主和空 sibling 边界投影问题后复核通过；最终 diff 无 blocker |

## 风险与回退

若外侧与内部位置被折叠，空环境会失去焦点归属；若适配器复用错误的局部坐标，数学/Section 内部焦点会失真。无法唯一映射时，保留文档与有效状态并原子拒绝，不静默创建/删除环境或跳过内容。

## 决策与工作记录

- 2026-11-09：从 080 拆出 P0 焦点仲裁切片。维护者确认统一 Block/Inline 三态为外前｜内部(宿主局部位置)｜外后；内部点数由局部结构决定，单槽空环境唯一，复合 Block 可有多个空槽。
- 2026-11-09：Issue #126 位置、边界转移和重复 Session 状态核查已完成；当前实现差异仅记录在任务，不反向写入架构权威。
- 2026-11-10：Inline/Block 边界只有在编辑目标一致时才共用规范位置；若块前输入会创建同级 Paragraph，而 Inline 外侧输入落入已有 Paragraph，则保留两个可到达状态。Inline carrier 在段落映射与块边界仲裁验证后转换；数学 Tree 局部投影保留 Align cell 宿主身份。

## 完成摘要

已完成共享宿主三态 carrier、Block/Inline 边界转换、空 Inline 内部槽位、数学局部坐标适配及会话历史恢复；分领域与独立最终评审通过，engine/app 回归、格式和 clippy 检查通过。
