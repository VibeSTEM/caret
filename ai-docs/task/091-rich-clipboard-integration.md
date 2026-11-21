# 091 — 富剪贴板与数学片段接入

- 状态：implemented
- 依赖：083、092
- 优先级：P1
- 创建 / 更新：2026-11-16 / 2026-11-21

## 目标与背景

当前系统剪贴板仍是纯文本，结构范围中的公式可能以 U+FFFC 暴露并丢失 MathTree。本任务实现剪贴板架构定义的片段传输与事务，并复用任务 092 定义的数学转换 API；不建立通用交换 AST，也不实现完整 Markdown 或 TeX 文档适配器。

行为契约唯一见[剪贴板架构](../architecture/editor/clipboard.md)；数学转换边界见[数学公式](../architecture/editor/math.md)。

## 必读

- [文档格式导入与导出](../architecture/editor/document-interchange.md)
- [数学公式](../architecture/editor/math.md)
- [块宿主交互协议](../architecture/editor/document-model/block-interaction.md)
- [跨宿主选区与事务](083-cross-host-selection-and-transactions.md)
- [验证与评审规范](../standards/validation-and-review.md)
- [提交规范](../standards/commits.md)

## 范围与非目标

- 实现剪贴板架构定义的 Caret 结构载荷与 text/plain 双格式复制粘贴，并验证已选范围和公式结构可往返。
- Qt 只负责系统剪贴板多格式读写；选区切片、模型校验、TeX 转换、身份重建和一次性编辑事务由 Rust 领域层负责。
- 不实现全量 Markdown/TeX 文件导入导出、TeX 宏执行、HTML 富文本、任意外部格式探测或新的通用转换 AST。

## 完成情况

按[剪贴板架构](../architecture/editor/clipboard.md)实现 v1 双格式 MIME、严格片段编解码、text/plain 回退与单事务粘贴：

- **caret-doc**：新增 `ClipboardFragment`（shape Inline/Blocks、首尾裁剪标志、`Vec<Block>`）与严格 JSON 信封 `caret.fragment` v1；解码拒绝未知信封字段、任意层级 `extra`/未知节点字段、Opaque、未来/旧 MathTree schema、非法模型与重复 ID；提供递归 NodeId 重写与 ID 查询。片段模型校验使用 `validate_for_safe_preview`（跳过源文档主题模板解析，其余 ID/深度/数学/环境规则全检），以便携带格式节点的片段可跨主题传输。
- **caret-math**：新增 `wrap_tex`（`\(…\)`/`\[…\]`）与外部纯文本扫描 `scan_plain_text_math`：整段 `\[…\]`/`$$…$$`（内容恰为 aligned → Align，否则 Equation）、整段 `$…$`/`\(…\)`、混合文本逐对 `\(…\)`；未闭合/非法/超限/歧义整段回退 Plain，不部分插入；`$` 只在整段规则识别。
- **caret-engine**：
  - `Session::copy_clipboard_fragment`：普通单段选区 → Inline、跨段/结构选区 → Blocks；按文档顺序递归裁切（首尾段落用 TextMap/`split_inline_nodes`，环境完整覆盖保留、部分覆盖展开，Equation/Align/InlineMath/MathTree/InlineEnvironment 原样保留）；plain_text 全部经 092 checked `MathFragment::to_tex` + `wrap_tex`，无 U+FFFC，checked 损失返回 `clipboard.math_loss`；空选区返回 `None`；公式内部端点复制返回 `clipboard.host_mismatch`（v1 非目标，显式拒绝）。
  - `Session::paste_clipboard_fragment`：Inline 仅接受正文 Paragraph 文本位置并合并 TextFlow；Blocks 支持正文文本/结构选区、嵌套环境、首尾 clipped 与目标前后缀合并、块级对象落点，先 `regenerate_clipboard_ids`、候选文档 `Document::validate()` 通过后才写入；Inline 用单个 `Paragraphs` 补丁、Blocks 用新增 `DocumentPatch::Body`、显示公式用 `BlockRange`，均一次 commit、generation +1、一条 Undo；失败在任何写入前返回，文档/焦点/选区/generation/history 全不变。
  - `Session::paste_external_clipboard_text`：Plain 回退既有粘贴语义；WholeInline/MixedInline 构造 InlineMath 含量替换选区；WholeDisplay 插入现成 Equation/Align 块；非正文目标安全回退纯文本；全部单事务。
  - 新命令 `clipboard.paste_fragment`（`PasteClipboardFragment`）与 `clipboard.paste_external`（`PasteExternalClipboardText`）；新错误 `clipboard.host_mismatch`、`clipboard.math_loss: …`。
- **caret-app/FFI**：`copy_clipboard_json` 一次返回 `{fragment|null,text}`（原子取同一选区）；`paste_clipboard_json(text,fragment)` 严格解码 Caret 载荷，失败 `clipboard.invalid_fragment` 且**不降级**纯文本；FFI 新增 `clipboard_copy_json`/`clipboard_paste_json`，沿用 `error:` 协议与 generation/revision 刷新。
- **native/QML**：`layout_view_model_clipboard_commands.cpp` 改为 `QMimeData` 双格式（`application/vnd.caret.fragment+json` + text），复制一次 FFI 成功后才写剪贴板（空选区不覆盖），粘贴只读一次 `mimeData()`、Caret 格式原样转发给 Rust、明确拒绝损坏载荷不降级；`documentOperationMessage` 新增 `clipboard.*` 用户文案（四 locale 同步）；QML 仍只触发调用、不推导选区。
- **测试**：caret-doc 15 项、caret-math 15 项、caret-engine 29 项、caret-app/FFI 新增用例、Qml.ShellLoad 内 4 个真实 QClipboard MIME/外部文本/损坏载荷用例及既有剪贴板断言更新。

## v1 非目标与后续

Math 形状（公式内部连续 MathFlow/MathText 局部选区裁切与粘贴到数学宿主）、跨文档主题模板迁移、QML 用户可见错误对话框、真实跨进程/跨平台剪贴板验证、混合在普通文本中的显示公式识别、含选区外 Opaque 节点的文档复制剪枝由任务 105 跟进；v1 对这些路径显式拒绝/安全回退而非静默降级。

## 前置条件与待决策

- `caret-math` 已有 `MathInputKind`、数学 parser 与 MathTree/Align serializer；实现前盘点并复用现有入口，只有其公共边界不足时才调整 API。
- 依据规范模型验证被复制节点和 ID 重映射；extra 与格式字段不得经白名单静默丢失。
- MIME 与 schema 版本遵循剪贴板架构页；实现不得复制该页的 wire-format 规则。
- 验收覆盖 Equation、Align、InlineMath、跨段落/跨块混合选区、环境嵌套、无效 JSON、未知版本、解析错误、撤销/重做和跨应用纯文本回退。

## 实施步骤

1. 核对 Rust `Session::selected_text`、结构范围遍历、`caret-math` parser/serializer、FFI JSON 与 Qt `QClipboard` 当前调用链，确认剪贴板权威状态不复制到 UI。
2. 设计并实现带版本的规范节点片段编码、选区裁切、粘贴验证和 ID 重映射；把所有插入作为单一 Rust 编辑事务。
3. 复用 092 的 checked 数学片段 API，增加剪贴板外层定界符识别/格式化适配，并保持文档级 047/049 适配器边界独立。
4. 接入 Qt 多格式剪贴板读写与错误传播；旧系统仅提供纯文本时仍走安全的外部片段导入路径。
5. 增加行为测试与失败原子性验证；同步架构和 task 记录，完成分域审查、修复和独立最终评审。

## 预计改动

`caret-engine` 选区提取/粘贴事务与测试、必要时 `caret-math` 片段 API 与测试、`caret-app`/FFI 结构载荷编码、native Qt 剪贴板命令与集成测试，以及相关架构文档。默认留在现有 crate；仅在现有模块边界无法承载共享数学片段职责时评估模块内拆分，不为此新建 crate。

## 验收标准

- [x] Caret MIME 往返保留选区中完整对象的 MathTree/Align、InlineMath、文本与格式；Inline 片段仅在 Paragraph 位置接受，Blocks 遵循宿主边界；粘贴后节点 ID 与目标文档无冲突。v1 未提供独立 Math 形状（公式内部局部选区），由任务 105 跟进并以 `clipboard.host_mismatch` 显式拒绝。
- [x] 纯文本回退无 U+FFFC 泄漏，公式以可读规范 TeX 输出；外部应用只读取 text/plain 仍能看到内容。共享 checked serializer 对不可表达语义或扩展字段报告损失，复制返回错误且不覆盖系统剪贴板。
- [x] 外部纯文本中的受支持闭合公式片段进入对应 InlineMath/Equation/Align；裸 `$`、未闭合、非法或超限输入保持原文（安全回退），不破坏选区与文档。
- [x] 粘贴为一次可撤销事务；Redo 重现相同结构与结果，失败不改变文档、焦点、选区、generation 或历史（engine 测试逐项断言）。
- [x] 损坏/未知 Caret 载荷不静默降格到纯文本；未知字段、未知版本、Opaque 与 unsupported 节点按严格策略拒绝并保留源载荷（C++ MIME 载荷测试）。
- [x] Rust 与 Qt 边界职责符合分层；格式/lint/直接消费者测试/分域评审通过（最终评审待完成）；覆盖率门槛由 Nightly 验证；窗口/跨平台与真实跨进程剪贴板未执行，已如实记录并转任务 105。
- [x] 047/049 的数学转换边界未变（复用 caret-math `MathFragment`），[数学公式](../architecture/editor/math.md)、[命令](../architecture/editor/commands.md)与[剪贴板](../architecture/editor/clipboard.md)实现状态已同步，无第二数学 AST 或适配器间调用。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-21 | macOS 27.2.1 arm64 / rustc 1.100.0；`cargo test --locked -p caret-doc` / `-p caret-math` / `-p caret-engine` / `-p caret-app` / `-p caret-ffi` | 编解码、TeX 扫描、复制/粘贴事务、外部文本、失败原子性、ID 重映射 | 通过：105 / 182 / 919 / 352 / 18 项 |
| 2026-11-21 | 全量门禁：`cargo fmt --all -- --check`、`cargo format --check`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo test --workspace --locked`、`cargo lint --check` | 无回归 | 通过：workspace 1995 项 + native CTest 20/20；fmt/format/clippy/lint 通过 |
| 2026-11-21 | `cargo build --locked -p caret-launcher` + `ctest --test-dir target/native/debug` + 定向 `Qml.ShellLoad` | native MIME 构建与真实 QClipboard 行为 | 通过：20/20；新增 4 个 MIME/外部文本/损坏载荷用例与既有剪贴板用例 PASS；copy text 无 U+FFFC、公式为 TeX 定界符，坏版本载荷拒绝且文档不变 |
| 2026-11-21 | 两轮分域评审（engine 剪贴板域 + 跨层协议域） | 发现项闭环 | 完成：M1 块粘贴无段落落点、M2 start-clipped caret、P1 非 UTF-8 MIME 载荷、P2 混合文本换行等已修复并补回归；其余低风险项记录在任务 105 |
| 2026-11-21 | 未参与实现的只读最终评审（两轮 delta 复核） | 可交付、发现项闭环 | 完成：M1/M2/P1/P2 闭环；最终评审发现并修复 end-clipped caret 回归与 `first_paragraph_hint` 文档顺序，补 seam/redo/单 Undo 回归；结论可交付 |
| — | 覆盖率门槛、真实跨进程/跨平台剪贴板（X11/Wayland/Windows） | 通过/如实记录 | 未执行：覆盖率由 Nightly 验证；跨进程/平台转任务 105 |

## 风险与回退

片段裁切、节点身份重映射或新旧版本解释不完整会导致结构损失或目标文档 ID 冲突。实现遇到不能完整验证的节点时在写文档前拒绝；纯文本回退供非 Caret 应用读取，不能用于掩盖 Caret 专用载荷的失败。

已记录的 v1 边界：公式内部局部选区暂不生成 Math 片段（显式 `clipboard.host_mismatch`）；携带源主题模板引用的片段粘贴到不同主题文档会被整文档校验拒绝（不静默丢字段）；`validate_for_safe_preview` 跳过主题模板解析是片段跨主题传输的必要取舍；空定界符（如 `\(\)`）会被识别为空公式片段；`ExplicitBreak` 在 Blocks 纯文本投影中仅作为块分隔，不生成页断占位。Blocks 全量 body 补丁使撤销校验依赖 before/after 快照全等，若未来编辑路径改写 body 形态需同步维护。

## 决策与工作记录

- 2026-11-16：经维护者确认，MathTree 片段应通过富剪贴板在 Caret 间保留结构，并提供外部可读 TeX；转换边界由 047/049 共用。
- 2026-11-16：盘点发现 047/049 原设计只定义完整文档适配器，未定义共用数学片段入口；架构已明确复用 `caret-math`，本 task 承接剪贴板结构与接线实现。
- 2026-11-21：实现决策：v1 采用 Inline/Blocks 两种片段形状，完整 MathTree/Align/InlineMath 作为对象保留在片段内；独立 Math 形状与公式内部局部选区列入任务 105。片段信封/严格校验/ID 重写放 caret-doc，选区裁切与单事务粘贴放 caret-engine，Qt 只原样读写 MIME；Blocks 粘贴使用新增 `DocumentPatch::Body` 保证单补丁/单 Undo。外部纯文本扫描与定界符格式化放 caret-math（复用 092 checked 边界）。公式内部复制显式失败而非降级纯文本。

## 完成摘要

任务 091 v1 已实现并验证：Caret MIME 双格式（严格版本化片段 + text/plain 回退）、无 U+FFFC 的 TeX 回退、外部纯文本闭合公式识别、ID 重映射、失败原子性与一次粘贴=一条 Undo 的片段事务；Qt 仅读写 MIME、Rust 负责切片/校验。全量门禁与 C++/QML 集成测试通过；v1 非目标（Math 形状与公式内部选区、跨文档主题模板迁移、QML 错误对话框、真实跨进程/跨平台剪贴板）已记录并由任务 105 跟进，未把未执行检查写成通过。
