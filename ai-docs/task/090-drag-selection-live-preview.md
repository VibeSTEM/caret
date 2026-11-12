# 090 — 拖选跨公式块/块环境的实时预览

- 状态：implemented
- 依赖：027、031、044、083
- 优先级：P1
- 创建 / 更新：2026-11-12 / 2026-11-12

## 目标与背景

桌面测试发现：鼠标拖动选择 `\align` / `\equation` 时，拖动过程中没有淡蓝色选区背景，松开后才出现。根因是拖选预览与提交使用两套几何来源：

- 拖动中：`qml/PageSurface.qml` 在 `selectionDragActive` 时把 `selectionRects` 置空，只画 `caretPreviewGeometry`；`qml/Canvas/DocumentCanvasPointerHandler.qml` 用 `dragHitMap` 的 segments 计算预览矩形。
- `drag_hit_map_json`（`crates/caret-app/src/document_view/selection.rs`）只为 `page.layout.fragments` 的正文行生成 body segments，公式内 MathText 文本岛另有 math segments，但 Equation/Align 整块与块环境没有 segment。
- 松手后 `set_selection_json` 才经 `selection_rectangles_json` / `structure_selection_rectangles` 生成公式整栏矩形与环境矩形。

因此跨公式块/块环境拖选时没有预览，松手后才出现。

## 必读

- [画布与交互背景](../../architecture/interface/canvas-highlighting.md)、[块宿主交互](../../architecture/editor/document-model/block-interaction.md)
- [验证与评审](../../standards/validation-and-review.md)、[测试规范](../../standards/testing.md)
- [083](083-cross-host-selection-and-transactions.md)、[027](027-canvas-and-hit-testing.md)

## 范围与非目标

- 正文（body domain）拖选：focus stop 变化时直接调用既有 `LayoutViewModel.setSelection(anchor, focus)`，画布在拖动中消费 `LayoutViewModel.selectionRects`；公式块、块环境、跨页、InlineMath、RTL 与松手后同源同值。
- 公式内 MathText 文本岛拖选保持既有片段预览（`previewOnly`），松手再提交。
- 非目标：不新增 app preview-only 接口/FFI；不改变松手提交语义；不处理“拖选起点落在公式对象上无法开始拖选”的既有独立限制。

## 前置条件与待决策

- 既有 `setSelection` 是松手路径，复用使“预览 = 松手”构造上一致；代价是拖动中真实提交会话选区，而非纯预览层。
- 拖动被 layoutChanged/IME 中断时选区停留在最后提交点，不回滚到拖动前；如产品要求取消回滚，需要 preview-only 接口（另开任务）。

## 实施步骤

1. QML `DocumentCanvasPointerHandler` 增加 `previewOnly` 与 `commitDragSelection`；仅 body domain 在 focus 变化时提交，`selectionPreviewRects` 清空。
2. `PageSurface` 拖选中仅在 previewOnly 时隐藏 `selectionRects`。
3. app 测试覆盖公式块整栏矩形、未跨公式不出现、块环境矩形、InlineMath+RTL、跨页；更新 native C++ shell 测试为拖动中 `selectionRects` 非空、跨页矩形来自 `selectionRects`。
4. 独立评审并记录性能/Qt 未验证项。

## 预计改动

`qml/Canvas/DocumentCanvasPointerHandler.qml`、`qml/PageSurface.qml`、`crates/caret-app/src/document_view/tests/layout.rs`、`tests/cpp/shell/shell_document_interaction_test.cpp`。

## 验收标准

- [x] body 拖选跨 Equation/Align 与块环境时，拖动中出现与松手一致的高亮矩形；未跨对象时不出现额外矩形。
- [x] 公式文本岛拖选仍使用片段预览，松手提交；native 测试覆盖跨页预览。
- [x] QML lint、app 测试、native CTest、workspace 测试通过；性能与 Qt 实机手感记录为未验证/待观察。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-12 | `cargo test --locked -p caret-app --lib` | 新增 5 条拖选预览回归通过 | 通过：325 passed / 0 failed |
| 2026-11-12 | native `ctest`（`Qml.ShellLoad`） | 拖动中 selectionRects 非空、预览矩形为空、跨页矩形存在 | 通过：11/11 CTest |
| 2026-11-12 | `cargo lint qmllint --check`、workspace clippy/fmt/测试 | 无警告/回归 | 通过 |
| — | Qt 桌面真实鼠标/触控板、拖动中布局变化/IME、性能 | 手感与开销可接受 | 未验证（本环境无窗口） |

## 风险与回退

- 拖动中真实提交选区：中断/layoutChanged 不回滚，且每次焦点变化产生一次 interaction 序列化；大文档长距离拖动性能需实机确认。
- 拖选起点落在公式对象上仍无法开始拖选（`dragStartDomain` 对 math structure hit 返回空），属既有独立路径，建议另开任务。
- 回退：`previewOnly` 重新置 true 即可恢复旧纯预览行为。

## 决策与工作记录

- 2026-11-12：从桌面测试发现中拆出并实现；选择复用 `setSelection` 而非新增 preview-only 接口，避免扩大 FFI/native 桥；性能与中断语义留待实机/后续任务评估。
