# 075 — Equation / Align 活动块背景高亮

- 状态：planned
- 依赖：044、072
- 优先级：P2
- 创建 / 更新：2026-11-08 / 2026-11-09

## 目标与背景

编辑 Equation 或 Align 时，当前公式块没有奶黄色背景。当前快照只为活动块环境投影 `environment_ranges`，而 QML 画布只消费 Rust/native 发布的矩形；显示公式虽有分页 MathFragment 几何，却未加入活动块高亮投影。

为活动 Equation 与 Align 提供稳定的块级背景几何，复用现有块环境高亮色、圆角与 padding，并确保分页、空公式与 Align cell 聚焦时反馈一致。键盘状态与方向键仲裁仍按[快捷键管理架构](../architecture/interface/keyboard-shortcut-architecture.md)统一由 `Session::run_editor_key` 管理；本任务不新增数学专用光标状态或按键路由。

块级奶黄色背景之外，Equation/Align 内的 `\text{...}` 文本岛在编辑时还需要现有粉色 inline 高亮，提示当前 IME 文本输入位置；两种高亮应能同时正确显示，文本岛高亮只覆盖活动文本范围，不替代公式块高亮。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[数学公式架构](../architecture/editor/math.md)、[当前环境背景高亮](031-environment-highlight.md)、[快捷键管理架构](../architecture/interface/keyboard-shortcut-architecture.md)、[072 空公式宿主占位](072-empty-math-host-placeholder.md)

## 范围与非目标

- 活动 MathEdit target 为块级 Equation 或 Align 时，为其可见页面片段发布高亮矩形；InlineMath 不显示块背景。
- 活动 Equation/Align TextFlow（包括 `\text{...}`）保留粉色 inline 输入高亮；块级奶黄色底色与文本岛高亮按独立几何绘制并可共存。
- Align 高亮粒度需明确选定并覆盖：整个 Align 宿主或当前活动 cell；不根据绘制占位框推断焦点。
- 多页公式只高亮当前活动宿主实际可见片段；无焦点、结构选区、退出停靠点和 stale snapshot 时不留下旧高亮。
- 复用现有主题颜色，不引入新颜色设置；不把 Equation/Align 伪装成文档块环境模型。

## 前置条件与待决策

- 072 的空宿主节点盒与命中契约完成并合入。
- 通过产品行为检查确定 Align 高亮是整个 Align 还是当前 cell；优先保持当前焦点语义一致。
- 明确背景是否需要扩展到块全内容宽度，或仅沿 MathFragment 宽度并由 Canvas padding 绘制。

## 实施步骤

1. 追踪活动 MathEdit target、分页 MathFragment 和现有 `blockEnvironmentHighlightRects` 的快照/codec/canvas 链。
2. 在权威快照层为活动公式块投影专用矩形字段，保持块环境范围与数学模型语义分开。
3. 由 Qt ViewModel 校验并通知该属性，QML Canvas 复用现有块高亮 token；不在 Qt 重建数学文档状态。
4. 覆盖 Equation/Align、空/非空、单页/分页、进入/退出公式及其他块环境回归；更新架构记录并独立评审。

## 预计改动

`crates/caret-app/src/document_view/snapshot.rs`、`native/viewmodels/document/layout_snapshot_codec.cpp` 与 ViewModel、`native/canvas/`、`qml/PageSurface.qml`，以及 Rust/C++/QML 测试。具体接口和模块边界按现有快照传输路径实现。

## 验收标准

- [ ] 活动 Equation 与 Align 显示奶黄色背景，矩形与当前宿主/选择语义一致；InlineMath 和非活动公式不出现块背景。
- [ ] Equation/Align 内活动 `\text{...}` 文本岛显示粉色 inline 高亮，范围跟随真实 TextFlow 光标/IME 预编辑，并与块级奶黄色背景共存。
- [ ] 空宿主仍有高亮几何且不依赖 placeholder Rectangle；Align 空 cell 的选定粒度稳定。
- [ ] 分页裁切、点击切换焦点、键盘离开、undo/redo 和结构选区不会保留旧高亮或误高亮邻接块环境。
- [ ] 快照、Qt 属性、QML 绘制与主题绑定贯通；非法坐标/旧快照拒绝且不替换已发布状态。
- [ ] Rust/native/QML 行为测试、格式/lint 与独立评审通过；未能执行的桌面视觉验证记录平台限制。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Rust app 快照/布局测试 | 活动宿主只发布有效分页范围 | 未执行 |
| — | Qt shell / QML 行为测试 | 主题背景显示、焦点切换后清理 | 未执行 |
| — | 格式、lint 与独立评审 | 修改范围通过仓库门禁 | 未执行 |

## 风险与回退

Equation/Align 的 MathFragment 坐标相对页面版心，且可能跨页；必须用页边距转换成 page-local 内容矩形并裁切到页面。若活动 Align cell 几何无法稳定映射，先高亮整个 Align 宿主并在文档中记录明确的产品行为，不合成虚假 cell 坐标。

## 决策与工作记录

- 2026-11-08：用户报告单行/多行数学块缺少奶黄色背景。初查发现 Rust 只从块环境 ranges 发布背景，QML 负责绘制但不能补出公式宿主范围；在 072 完成后单独实施，避免把公式块误建模为环境节点。
- 2026-11-08：用户补充 Equation/Align 中使用 `\text` 时，活动文本岛也需要粉色 inline 高亮；与奶黄色块级高亮分别建模、绘制并验证共存。

## 完成摘要

未完成。
