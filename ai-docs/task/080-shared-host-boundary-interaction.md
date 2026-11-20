# 080 — Block/Inline 共享交互底座（Paragraph 为基线）

- 状态：blocked
- 依赖：071、074、077、081、082、083、091
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-21

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

- 在 081–083、091 完成后验收 Paragraph 与当前复杂 Block/Inline 共享位置仲裁、全树顺序、焦点、选区、删除/替换/Undo/Redo、富剪贴板和同代几何。
- Paragraph 已验证行为作为基线；不得把其 UTF-8 坐标强制成所有宿主的位置类型。
- 适配器保留 Section/Equation/Align、InlineMath/InlineEnvironment 的真实位置与事务；不复制权威边界状态、焦点、文档顺序或快捷键仲裁。
- Equation/Align 外侧 Up/Down 必须先消费共享视觉行候选，与 Section/Custom 使用同一块边界仲裁；InlineMath 外部逐项扩选纳入 083 的跨宿主范围适配，不把 U+FFFC 当作内部端点。
- 保留用户已确认的三态焦点、Block 外侧停靠、空 Inline 粉色槽位和 079 Align 行首退格规则。
- Table/Figure 仅定义接入契约，不在本任务实现节点、单元格编辑或完整网格选区；不新建 crate。

## 前置条件与待决策

- 071、074、081、082、083 已完成；077 为 `implemented`（代码与自动验证完成，仅桌面 caret 视觉确认待补）。
- 091 富剪贴板已转为 `ready`（依赖 083、092 均完成）；在本任务完成跨宿主剪贴板与 TeX 回退的整链验收前，080 无法关闭。
- 解除阻塞条件：091 完成并接入剪贴板 → 执行本任务剩余整链验收与独立评审。

## 实施步骤

1. 完成 081 的共享宿主三态焦点仲裁。
2. 完成 082 的文档位置、排序与宿主 locator。
3. 完成 083 的跨宿主选区、编辑事务和几何投影。
4. 完成 091 的结构剪贴板与共享数学片段转换接入。
5. 对完整链路检查 Paragraph 回归、Block/Inline adapter 一致性、MathTree 剪贴板往返、Undo/Redo、失败原子性与同代快照。
6. 更新本 task 的整合验证结果；执行相关分域审查，并由未参与实现的 Sol/medium subagent 对最终完整 diff 做只读评审。

## 预计改动

整合涉及 `caret-engine` Session、`caret-structure` 顺序/范围、`caret-layout` 几何、`caret-app` 导航与快照、Qt/FFI 消费及跨层行为测试。具体代码边界以子任务为准；不因缩短文件或移动代码而新建 crate。

## 验收标准

- [x] 081、082、083 均完成且各自验收通过。
- [x] Paragraph、Section/Custom、Equation/Align 与 InlineEnvironment/InlineMath 使用单一共享交互协议，适配器只提供领域局部位置、事务与几何（子任务 071/081/082/083 的实现与评审记录）。
- [x] 连续 Block 删除落点、Section 邻接 Backspace、Inline/Block 双侧扩选、替换与空 Inline 槽位符合权威行为文档；复制/粘贴部分受 091 阻塞，见下方未完成项。
- [x] 位置、选区、事务与历史使用同一宿主身份/顺序；完整快照、增量刷新与 Qt 绘制使用同代投影（082/083 验收及本次定向回归）。
- [x] 无重复权威状态、任意 Paragraph 隐藏锚点、逐类型补丁、U+FFFC 泄漏、静默对象丢失或失败后非法焦点（083 分域评审发现项已修复）。
- [x] Paragraph、MathEdit、Section、Align 行/cell 的真实局部坐标和行为未被统一底座抹平；新 Block 只需 adapter 接入（子任务测试与 #151 跨宿主范围收敛）。
- [x] Section、Equation、Align 的块外侧垂直导航和 Shift 选区共享视觉行候选/块边界仲裁；InlineMath 外部逐项扩选通过共享端点、范围消费与同代几何协议（#151 与 2026-11-21 定向回归）。
- [ ] 相关行为测试、覆盖率门槛、分域检查及独立最终评审在整链（含剪贴板）上通过；未执行的桌面平台验收明确记录。当前仅子任务分别通过，整链待 091 后执行。
- [ ] Caret 内复制粘贴保留 MathTree/Align 和 InlineMath 对象；外部应用读取可读 TeX 回退；显式 TeX 数学片段粘贴由 091 接入 092 的共享转换器。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-18 | `cargo fmt --all -- --check`、`cargo test --locked -p caret-app vertical_navigation_leaves_math_block_boundary_for_adjacent_visual_paragraph`、`cargo check --locked -p caret-engine` | 块级公式外停靠的 Up/Down 先走视觉段落候选，Shift 扩展不误进公式 | 通过；Equation/Align 停靠到相邻 Paragraph 的定向场景 |
| 2026-11-21 | 审计 081/082/083 文档与索引状态 | 子任务均完成并各自验收 | 081/082/083 均 `done`；071、074 `done`，077 `implemented`；091 `planned`（依赖 092 `ready`） |
| 2026-11-21 | `cargo test --locked -p caret-engine editing::session::structure` / `editing::session::navigation` / `editing::session::math_edit::tests::selection_caret` / `editing::session::document_position` | 跨宿主删除/导航/选区/文档位置协议一致 | 通过：73 / 40 / 86 / 24 项 |
| 2026-11-21 | `cargo test --locked -p caret-app vertical_navigation_leaves_math_block_boundary_for_adjacent_visual_paragraph` / `structural_selection_geometry_covers_equation_hosts_across_host_boundaries` / `backspace` / `inline_math` 过滤 | 块边界、跨宿主几何与空 inline 槽位回归 | 通过：1 / 1 / 14 / 36 项 |
| 2026-11-21 | `cargo test --locked -p caret-engine empty_inline_math`、`cargo test --locked -p caret-app empty_inline_math` | 087 收尾：空 Inline 槽位/命中/删除/Undo/Redo/IME 回归 | 通过：9 / 5 项 |
| — | 091 富剪贴板接入后的整链回归 | 复制/粘贴 MathTree、TeX 回退与同代投影 | 未执行（阻塞条件；092 转换器已就绪） |
| — | 覆盖率门槛与整链 Terra/Sol 评审 | 通过 | 未执行（阻塞条件） |

## 风险与回退

整合变更影响普通文本、Section 标题语义、连续 Block 导航和数学坐标映射。子任务逐步合入；若任何边界无法无损映射，保持文档和有效状态并原子拒绝，不回退到任意 Paragraph 或重复状态机。

## 决策与工作记录

- 2026-11-09：维护者确认 Block/Inline 两层模型和 Paragraph 基线。
- 2026-11-09：Issue #126 审查完成：H1/H2 拆到 081/082；M1/M3/M4 拆到 082；M2 及跨宿主选区/删除落点拆到 083；M5 文档矛盾已修正；M6 未复现报告转入未完成的 069 复核；未经验证的模型 ID/复杂度方案不作为结论。
- 2026-11-09：将原 L 级混合实现 task 拆为 081（P0）、082 和 083；080 保留为跨子任务整合验收，不重复维护子任务设计细节。
- 2026-11-09：本 PR 仅更新/整理 Markdown 文档，不包含代码改动或平台验证。
- 2026-11-16：审计发现原有纯文本剪贴板会将公式降格为 U+FFFC，与结构复制契约冲突。将富剪贴板实现拆为 091；080 保留跨宿主最终集成验收。
- 2026-11-18：桌面测试发现 Equation 外侧 Up/Down 走入公式边界重入路径，Shift 垂直扩展在视觉候选计算前只覆盖整公式；InlineMath 外部选区也只表达 U+FFFC。按维护者要求在 080/083 中补齐 Section 主导的垂直候选复用与 MathTree 内部跨宿主选区。
- 2026-11-21：跟进审计：081/082/083 已完成，083 的 InlineMath 跨宿主范围由 #151 合入；071/074 `done`、077 `implemented`。定向回归（engine structure/navigation/math selection/document_position；app 块边界、跨宿主几何、Backspace、inline math 及 087 空 inline 槽位）全部通过。剩余验收依赖 091 富剪贴板（依赖 083、092 均已 done，091 已转 `ready`）：在 091 完成前，080 的复制/粘贴整链与覆盖率/最终评审无法执行。任务标记为 `blocked`，下一步从 091 开始。 勾选项当前依据 081/082/083 各自验收与 2026-11-21 定向回归，080 整链复核待 091 后再执行；087 收尾新增的 Qt 端到端空槽渲染用例不改变 080 剩余范围。

## 完成摘要

未完成，状态 `blocked`。081、082、083 已完成并合入，071/074 `done`、077 `implemented`；跨宿主位置/顺序/焦点/选区/事务/几何与空 Inline 槽位的定向回归在 2026-11-21 通过。剩余范围是 091 富剪贴板的 MathTree/Align/InlineMath 保留、TeX 回退（复用已完成的 092 `MathFragment`），以及其后的整链覆盖率、分域与独立 final review；091 已完成依赖转 `ready`，构成具体阻塞。
