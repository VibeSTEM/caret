# 080 — Block/Inline 共享交互底座（Paragraph 为基线）

- 状态：planned
- 依赖：071、074、077、081、082、083
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-09

## 目标与背景

Caret 的 Block/Inline 编辑目前分散管理焦点位置、边界移动、选区、删除、历史与几何投影。维护者确定的模型是两层：Block 是外层宿主，Paragraph 是结构最简单、编辑规则最完整的 Block；Inline 是 Paragraph 或明确支持 Inline 流的 Block 内部内容。Section、Equation、Align 及未来 Table/Figure 等 Block 可以拥有不同内部结构，但不可各自复制同一交互仲裁。

080 是整合和验收任务，不再承载单个大型实现切片。实施顺序如下：

1. [081 — Block/Inline 共享三态焦点仲裁](081-host-three-state-caret-arbitration.md)（P0）
2. [082 — 共享文档位置、全序与宿主定位](082-shared-document-position-and-order.md)
3. [083 — 跨宿主选区、编辑事务与几何投影](083-cross-host-selection-and-transactions.md)

## 必读

- [文档树与结构](../architecture/editor/document-model/document-structure.md)
- [块宿主交互协议](../architecture/editor/document-model/block-interaction.md)
- [文档位置与限制](../architecture/editor/document-model/document-position.md)
- [结构导航](../architecture/editor/structure-navigation.md)
- [验证与评审标准](../standards/validation-and-review.md)

## 范围与非目标

- 在 081–083 完成后验收 Paragraph 与当前复杂 Block/Inline 共享位置仲裁、全树顺序、焦点、选区、删除/替换/Undo/Redo 和同代几何。
- Paragraph 已验证行为作为基线；不得把其 UTF-8 坐标强制成所有宿主的位置类型。
- 适配器保留 Section/Equation/Align、InlineMath/InlineEnvironment 的真实位置与事务；不复制权威边界状态、焦点、文档顺序或快捷键仲裁。
- 保留用户已确认的三态焦点、Block 外侧停靠、空 Inline 粉色槽位和 079 Align 行首退格规则。
- Table/Figure 仅定义接入契约，不在本任务实现节点、单元格编辑或完整网格选区；不新建 crate。

## 前置条件与待决策

- 071、074、077 的仓库内能力可用。
- 081、082、083 分别完成并通过各自验证和独立评审后，执行跨域整合；不同 Session 状态只有语义相同且消费者重复时才能合并。

## 实施步骤

1. 完成 081 的共享宿主三态焦点仲裁。
2. 完成 082 的文档位置、排序与宿主 locator。
3. 完成 083 的跨宿主选区、编辑事务和几何投影。
4. 对完整链路检查 Paragraph 回归、Block/Inline adapter 一致性、Undo/Redo、失败原子性与同代快照。
5. 更新本 task 的整合验证结果；执行相关分域审查，并由未参与实现的 Sol/medium subagent 对最终完整 diff 做只读评审。

## 预计改动

整合涉及 `caret-engine` Session、`caret-structure` 顺序/范围、`caret-layout` 几何、`caret-app` 导航与快照、Qt/FFI 消费及跨层行为测试。具体代码边界以子任务为准；不因缩短文件或移动代码而新建 crate。

## 验收标准

- [ ] 081、082、083 均完成且各自验收通过。
- [ ] Paragraph、Section/Custom、Equation/Align 与 InlineEnvironment/InlineMath 使用单一共享交互协议，适配器只提供领域局部位置、事务与几何。
- [ ] 连续 Block 删除落点、Section 邻接 Backspace、Inline/Block 双侧扩选、复制/替换和空 Inline 槽位均符合权威行为文档。
- [ ] 位置、选区、事务与历史使用同一宿主身份/顺序；完整快照、增量刷新和 Qt 绘制使用同代投影。
- [ ] 无重复权威状态、任意 Paragraph 隐藏锚点、逐类型补丁、U+FFFC 泄漏、静默对象丢失或失败后非法焦点。
- [ ] Paragraph、MathEdit、Section、Align 行/cell 的真实局部坐标和行为不因统一底座被抹平；新 Block 只需 adapter 接入。
- [ ] 相关行为测试、格式/lint、覆盖率门槛、分域检查及独立最终评审通过；未执行的桌面平台验收明确记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 081/082/083 子任务验证 | 各自契约通过 | 未执行 |
| — | 跨宿主完整链路回归 | 焦点、选区、编辑、历史和几何一致 | 未执行 |
| — | Terra 分域检查、Sol final review | 完整 diff 无阻断发现 | 未执行 |

## 风险与回退

整合变更影响普通文本、Section 标题语义、连续 Block 导航和数学坐标映射。子任务逐步合入；若任何边界无法无损映射，保持文档和有效状态并原子拒绝，不回退到任意 Paragraph 或重复状态机。

## 决策与工作记录

- 2026-11-09：维护者确认 Block/Inline 两层模型和 Paragraph 基线。
- 2026-11-09：Issue #126 审查完成：H1/H2 拆到 081/082；M1/M3/M4 拆到 082；M2 及跨宿主选区/删除落点拆到 083；M5 文档矛盾已修正；M6 未复现报告转入未完成的 069 复核；未经验证的模型 ID/复杂度方案不作为结论。
- 2026-11-09：将原 L 级混合实现 task 拆为 081（P0）、082 和 083；080 保留为跨子任务整合验收，不重复维护子任务设计细节。
- 2026-11-09：本 PR 仅更新/整理 Markdown 文档，不包含代码改动或平台验证。

## 完成摘要

未完成。
