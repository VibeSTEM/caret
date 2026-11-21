# 080 — Block/Inline 共享交互底座（Paragraph 为基线）

- 状态：done
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

- 071、074、081、082、083、090、091 已完成并合入；077 为 `implemented`（代码与自动验证完成，仅桌面 caret 视觉确认待补，不阻塞 080）。
- 091 富剪贴板 v1 已合入 main（PR #159，squash `5cf2d64e`）；依赖 083、092 均完成。整链验收已执行：复制/粘贴 MathTree/Align/InlineMath 保真、TeX 回退、一次 Undo、失败原子性、覆盖率与分域评审；独立最终评审进行中。
- Math 内部局部选区、跨文档主题模板迁移、QML 剪贴板错误对话框与真实跨进程/跨平台剪贴板不属 080 验收范围（任务 105 跟进）。

## 实施步骤

1. 完成 081 的共享宿主三态焦点仲裁。
2. 完成 082 的文档位置、排序与宿主 locator。
3. 完成 083 的跨宿主选区、编辑事务和几何投影。
4. 完成 091 的结构剪贴板与共享数学片段转换接入（091 v1 已合入）。
5. 对完整链路检查 Paragraph 回归、Block/Inline adapter 一致性、MathTree 剪贴板往返、Undo/Redo、失败原子性与同代快照。
6. 更新本 task 的整合验证结果；执行相关分域审查，并由未参与实现的 Sol/medium subagent 对最终完整 diff 做只读评审。

## 预计改动

整合涉及 `caret-engine` Session、`caret-structure` 顺序/范围、`caret-layout` 几何、`caret-app` 导航与快照、Qt/FFI 消费及跨层行为测试。具体代码边界以子任务为准；不因缩短文件或移动代码而新建 crate。

## 验收标准

- [x] 081、082、083 均完成且各自验收通过。
- [x] Paragraph、Section/Custom、Equation/Align 与 InlineEnvironment/InlineMath 使用单一共享交互协议，适配器只提供领域局部位置、事务与几何（子任务 071/081/082/083 的实现与评审记录）。
- [x] 连续 Block 删除落点、Section 邻接 Backspace、Inline/Block 双侧扩选、替换与空 Inline 槽位符合权威行为文档；复制/粘贴整链复核见第 9 项与验证表。
- [x] 位置、选区、事务与历史使用同一宿主身份/顺序；完整快照、增量刷新与 Qt 绘制使用同代投影（082/083 验收及本次定向回归）。
- [x] 无重复权威状态、任意 Paragraph 隐藏锚点、逐类型补丁、U+FFFC 泄漏、静默对象丢失或失败后非法焦点（083 分域评审发现项已修复）。
- [x] Paragraph、MathEdit、Section、Align 行/cell 的真实局部坐标和行为未被统一底座抹平；新 Block 只需 adapter 接入（子任务测试与 #151 跨宿主范围收敛）。
- [x] Section、Equation、Align 的块外侧垂直导航和 Shift 选区共享视觉行候选/块边界仲裁；InlineMath 外部逐项扩选通过共享端点、范围消费与同代几何协议（#151 与 2026-11-21 定向回归）。
- [x] 相关行为测试、覆盖率门槛、分域检查及独立最终评审在整链（含剪贴板）上通过；未执行的桌面平台验收已在验证表与「已知限制与未执行项」明确记录。
- [x] Caret 内复制粘贴保留 MathTree/Align 和 InlineMath 对象；外部应用读取可读 TeX 回退；显式 TeX 数学片段粘贴由 091 接入 092 的共享转换器。新增 C++ 真实 `QClipboard` Blocks 用例逐项断言片段内 InlineMath `x`、Equation `y`、Align `a` 的 MathTree，粘贴后 InlineMath 占位 1→2、显示公式 2→4、新显示公式 ID 2 个且与源 ID 不冲突，一次 Undo/Redo 恢复整份快照；纯文本回退断言 `\(x\)`、`\[y\]`、`aligned` 且无 U+FFFC。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-18 | `cargo fmt --all -- --check`、`cargo test --locked -p caret-app vertical_navigation_leaves_math_block_boundary_for_adjacent_visual_paragraph`、`cargo check --locked -p caret-engine` | 块级公式外停靠的 Up/Down 先走视觉段落候选，Shift 扩展不误进公式 | 通过；Equation/Align 停靠到相邻 Paragraph 的定向场景 |
| 2026-11-21 | 审计 081/082/083 文档与索引状态 | 子任务均完成并各自验收 | 081/082/083 均 `done`；071、074 `done`，077 `implemented`；091 `planned`（依赖 092 `ready`） |
| 2026-11-21 | `cargo test --locked -p caret-engine editing::session::structure` / `editing::session::navigation` / `editing::session::math_edit::tests::selection_caret` / `editing::session::document_position` | 跨宿主删除/导航/选区/文档位置协议一致 | 通过：73 / 40 / 86 / 24 项 |
| 2026-11-21 | `cargo test --locked -p caret-app vertical_navigation_leaves_math_block_boundary_for_adjacent_visual_paragraph` / `structural_selection_geometry_covers_equation_hosts_across_host_boundaries` / `backspace` / `inline_math` 过滤 | 块边界、跨宿主几何与空 inline 槽位回归 | 通过：1 / 1 / 14 / 36 项 |
| 2026-11-21 | `cargo test --locked -p caret-engine empty_inline_math`、`cargo test --locked -p caret-app empty_inline_math` | 087 收尾：空 Inline 槽位/命中/删除/Undo/Redo/IME 回归 | 通过：9 / 5 项 |
| 2026-11-21 | 整链定向回归：`cargo test --locked -p caret-engine`（structure 73、navigation 40、document_position 24、math_edit::tests::selection_caret 86、clipboard_fragment 29、empty_inline_math 9）；`-p caret-app`（empty_inline_math 5、backspace 14、inline_math 36、块边界 1、跨宿主几何 1） | 跨宿主删除/导航/选区/文档位置/剪贴板协议与几何一致 | 通过；另 `cargo test --locked -p caret-doc -p caret-math` 105/182 通过 |
| 2026-11-21 | C++ 真实 `QClipboard` 整链：`richClipboardBlocksPasteRoundTripsDisplayMathAndUndo`（新增）+ `cargo build --locked -p caret-launcher` + `ctest --test-dir target/native/debug -R '^Qml.ShellLoad$'` | 跨段 Blocks 复制含 InlineMath/Equation/Align；text/plain 可读 TeX 且无 U+FFFC；粘贴后三类数学对象保留、ID 重映射无冲突、一次 Undo/Redo | 通过：Qml.ShellLoad 1/1（13.17s）；同套件其余剪贴板/撤销/跨宿主用例均通过 |
| 2026-11-21 | 全量门禁：`cargo fmt --all -- --check`、`cargo format --check`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo test --workspace --locked`、`cargo lint --check` | 无回归 | 通过：workspace 1996 passed + native CTest 20/20（macOS arm64 本地）；fmt/format/clippy/lint 通过。Ubuntu Nightly 的 full tests 因平台条件用例为 1993 项，同样全绿 |
| 2026-11-21 | Nightly diagnostics [#44865061216](https://github.com/VibeSTEM/caret/actions/runs/44865061216)（main@`2c851b15`，产品代码与当前分支一致；本分支仅新增 C++ 测试与记录） | Rust coverage gate、native C++ 85% coverage gate、三平台 full workspace tests、三平台 sanitizer、quality、Miri 全部通过 | 通过：14/14 job 成功；覆盖率门槛证据来自该 run，非本分支本地 coverage |
| 2026-11-21 | 两轮分域评审（跨宿主整链、剪贴板跨宿主集成） | 无功能阻断；需补强 C++ 断言与 080 记录 | 完成：评审报告确认 081/082/083/090/091 链路行为正确；新 C++ 用例已按建议补 MathTree 内容、InlineMath 占位、ID 重映射与 Undo/Redo 整快照断言；记录同步 |
| 2026-11-21 | 未参与实现的 Sol/medium 只读最终评审 | 对最终完整 diff 可交付 | 通过：确认新 C++ 用例非恒真、夹具改动合理、无产品代码改动；指出记录计数/矛盾/证据缺口，已修正并复核 |
| 2026-11-21 | PR [#165](https://github.com/VibeSTEM/caret/pull/165) CI | 适用作业通过 | 通过：`cargo check and build`、`cargo format`、`cargo lint (clang-tidy/includes/cppcheck)`、`detect changed paths` 均 pass，其余按路径 skipping；`mergeStateStatus=CLEAN` |
| — | 桌面平台验收：真实窗口交互/视觉、真实跨进程/跨平台系统剪贴板、外部应用互操作 | 可观察 | 未执行：077 桌面 caret 视觉确认待补；真实系统剪贴板/跨平台由任务 105 跟进；当前只有离屏 QML + 三平台 CI 无头矩阵 |

## 已知限制与未执行项

- 077 桌面 caret 视觉确认仍待补（077 保持 `implemented`）；不阻塞 080。
- 真实桌面窗口/交互/视觉验收未手工执行；现有证据为 macOS 离屏 QML 测试、C++ 真实进程内 `QClipboard` 与 CI 三平台无头矩阵。
- 真实跨进程/跨平台系统剪贴板（X11/Wayland 延迟渲染、Windows）与真实外部应用只读 text/plain 的互操作未验证；080 以进程内 MIME 与 `text/plain` 投影为代理，真实平台验证由任务 105 跟进。
- Math 内部局部选区与独立 Math 形状、跨文档主题模板迁移、QML 用户可见剪贴板错误、混合文本中的显示公式识别、选区外 Opaque 剪枝、粘贴全量重排性能，均由任务 105 跟进；091 v1 对这些路径显式拒绝或安全回退。
- 覆盖率门槛证据来自 Nightly [#44865061216](https://github.com/VibeSTEM/caret/actions/runs/44865061216)（main@`2c851b15`；本分支无产品代码差异，仅新增 C++ 测试与记录），不是本分支自身运行的 coverage。
- 两轮分域评审为会话内只读 subagent 报告，未在 PR 评论中独立存档；最终评审为未参与实现的只读 diff 评审。

## 风险与回退

整合变更影响普通文本、Section 标题语义、连续 Block 导航和数学坐标映射。子任务逐步合入；若任何边界无法无损映射，保持文档和有效状态并原子拒绝，不回退到任意 Paragraph 或重复状态机。

## 决策与工作记录

- 2026-11-09：维护者确认 Block/Inline 两层模型和 Paragraph 基线。
- 2026-11-09：Issue #126 审查完成：H1/H2 拆到 081/082；M1/M3/M4 拆到 082；M2 及跨宿主选区/删除落点拆到 083；M5 文档矛盾已修正；M6 未复现报告转入未完成的 069 复核；未经验证的模型 ID/复杂度方案不作为结论。
- 2026-11-09：将原 L 级混合实现 task 拆为 081（P0）、082 和 083；080 保留为跨子任务整合验收，不重复维护子任务设计细节。
- 2026-11-09：本 PR 仅更新/整理 Markdown 文档，不包含代码改动或平台验证。
- 2026-11-16：审计发现原有纯文本剪贴板会将公式降格为 U+FFFC，与结构复制契约冲突。将富剪贴板实现拆为 091；080 保留跨宿主最终集成验收。
- 2026-11-18：桌面测试发现 Equation 外侧 Up/Down 走入公式边界重入路径，Shift 垂直扩展在视觉候选计算前只覆盖整公式；InlineMath 外部选区也只表达 U+FFFC。按维护者要求在 080/083 中补齐 Section 主导的垂直候选复用与 MathTree 内部跨宿主选区。
- 2026-11-21：整链验收：新增 C++ `richClipboardBlocksPasteRoundTripsDisplayMathAndUndo` 覆盖跨段 Blocks 复制粘贴（InlineMath/Equation/Align 内容、ID 重映射、Undo/Redo），并补强 MathTree/占位/ID 断言；两轮分域评审确认无功能阻断。
- 2026-11-21：跟进审计：081/082/083 已完成，083 的 InlineMath 跨宿主范围由 #151 合入；071/074 `done`、077 `implemented`。定向回归（engine structure/navigation/math selection/document_position；app 块边界、跨宿主几何、Backspace、inline math 及 087 空 inline 槽位）全部通过。（该条为历史计划，已被 2026-11-21 整链验收条目取代）当时剩余验收依赖 091 富剪贴板：080 可在 091 合入后执行复制/粘贴整链、覆盖率与最终评审；Math 内部局部选区不属 080 验收范围（任务 105 跟进）。087 收尾新增的 Qt 端到端空槽渲染用例不改变 080 剩余范围。

## 独立评审

- **跨宿主整链领域评审**（未参与实现，只读）：逐条核对 080 验收与 081/082/083/090/091 证据，确认 Paragraph、Section/Custom、Equation/Align、InlineEnvironment/InlineMath 使用共享位置/顺序/焦点/选区/事务/几何协议，无重复权威状态；整链功能可转 done，需补记录。
- **剪贴板跨宿主集成领域评审**（未参与实现，只读）：确认 MIME 片段、TeX 回退、外部公式粘贴、单事务/失败原子性行为符合 091/092 契约；要求增强新增 C++ 用例断言（MathTree 内容、InlineMath 占位、ID 重映射、Undo/Redo 整快照），已补齐。
- **独立最终评审**（未参与实现，只读完整 diff）：确认测试非恒真、夹具 `atomClass` 调整合理、无范围外产品代码；指出记录计数 1995→1996、验收项 58 的矛盾引用、"已知限制" 缺失与历史计划表述，均已修正并复核通过。
- 限制：分域评审仅有会话内报告，未附独立 PR 评论；最终评审是只读 diff 评审，不等同于独立平台验收。

## 完成摘要

已完成并转 `done`：081/082/083 共享三态焦点、文档位置/全序、跨宿主选区与事务、同代几何，以及 090 拖选预览、091 v1 富剪贴板全部合入；Paragraph、Section/Custom、Equation/Align、InlineEnvironment/InlineMath 的跨宿主删除、替换、导航、选区、Undo/Redo 与剪贴板往返均有定向回归。新增 C++ 真实 `QClipboard` Blocks 用例验证完整 MathTree/Align/InlineMath 保真、TeX 回退无 U+FFFC、ID 重映射与一次 Undo/Redo；覆盖率门槛由 Nightly #44865061216 的 Rust 与 native 85% gate 提供；两轮分域评审与独立最终评审通过，PR #165 CI 适用作业全绿。未执行的桌面平台/真实跨进程剪贴板验收与任务 105 跟进项已在「已知限制与未执行项」如实记录。
