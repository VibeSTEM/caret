# 081 — Block/Inline 共享三态焦点仲裁

- 状态：ready
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
- 三态位置载体在本任务定义稳定宿主身份、状态类别（外前/内部/外后）和宿主局部位置；Block 内部位置由 Section 子树、Equation MathEdit、Align 行/单元格等模型适配，InlineEnvironment 使用 TextFlow 字素位置，InlineMath 使用 MathEdit。不得强制统一局部坐标类型。
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

- [ ] 所有具独立边界语义的 Block/Inline 共用外前、内部(宿主局部位置)、外后三类状态；不按 kind 复制边界状态机。
- [ ] 三态载体明确保存稳定宿主身份、状态类别和宿主局部位置；082 可直接消费该契约来实现父链与跨宿主全序，不需要反向补建 081 的边界身份。
- [ ] 内部局部位置可表达起点、终点及中间可编辑位置；载体差异由 adapter 保留，不把 TextFlow 偏移强转为 MathEdit/Section/Align 坐标。
- [ ] 复杂 Block 外前/外后可独立到达；相邻正文水平移动先停外侧再进入或越过，垂直视觉候选进入首/末内部位置；Paragraph 始终使用真实 TextMap 首尾。
- [ ] InlineEnvironment 外前 Right 进入内部起点、外后 Left 进入内部终点；内部局部移动到端点后再向外才退出。结构边界本身为合法字素边界时，进入不跳过首末字素；跨字素边界时遵循对称映射规则。
- [ ] InlineMath 保留 U+FFFC 两侧和 MathEdit 内部位置，并与格式 InlineEnvironment 使用同一外侧/内部仲裁。
- [ ] 空 InlineEnvironment 位于合法字素边界时，同偏移处只有唯一内部插入点；多个嵌套空格式环境合并为一个内部 stop 和完整环境链。
- [ ] 空复合 Block 通过适配器保留多个结构性插入槽位；不得将不同 Align cell、Section 标题/子宿主合并成一个内部位置。
- [ ] Run/InlineEnvironment 边界跨越 combining mark、variation selector 和 ZWJ 等组合字素时，进入与退出都不拆分 cluster；无法唯一映射时文档、焦点、选区和格式上下文不变。
- [ ] 空 Inline 内部槽位非零且为粉色，输入继承格式；`PendingInlineStyle` 复用几何但不创建文档节点，移动 caret 或关闭格式后槽位消失。
- [ ] 多行宿主边缘、无合法内部位置、失效 generation、无效局部候选均有行为测试；失败不改写文档、焦点、选区或历史。
- [ ] 相关格式/lint、行为测试和独立评审通过；外部平台验收如未运行则明确记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Block/Inline 边界正反向行为测试 | 共享三态，内部局部位置正确 | 未执行 |
| — | 空 InlineEnvironment 与 PendingInlineStyle 槽位测试 | 唯一内部点，格式和几何一致 | 未执行 |
| — | 数学块、Section、MathTree/MathEdit 与 Paragraph 导航回归 | 适配差异保留，无平行仲裁 | 未执行 |
| — | Terra/Sol 独立审查 | 无状态重复或边界回归 | 未执行 |

## 风险与回退

若外侧与内部位置被折叠，空环境会失去焦点归属；若适配器复用错误的局部坐标，数学/Section 内部焦点会失真。无法唯一映射时，保留文档与有效状态并原子拒绝，不静默创建/删除环境或跳过内容。

## 决策与工作记录

- 2026-11-09：从 080 拆出 P0 焦点仲裁切片。维护者确认统一 Block/Inline 三态为外前｜内部(宿主局部位置)｜外后；内部点数由局部结构决定，单槽空环境唯一，复合 Block 可有多个空槽。
- 2026-11-09：Issue #126 位置、边界转移和重复 Session 状态核查已完成；当前实现差异仅记录在任务，不反向写入架构权威。

## 完成摘要

未完成。
