# 043 — M3：公式文档模型与编辑

- 状态：in-progress
- 依赖：010、042
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-01

## 目标与背景

为可编辑公式建立版本化 Caret 数学树，并实现公式创建、内部编辑、位置和键盘导航。公式树属于 `.cdoc` 稳定文档模型；`latex-rust` AST 只能存在解析/排版适配器边界。总体验收由 [011 M3 公式](011-impl-math.md)管理。

## 必读

- [数学公式](../architecture/editor/math.md)、[文档模型](../architecture/editor/document-model.md)、[结构导航](../architecture/editor/structure-navigation.md)
- [命令输入](../architecture/editor/command-input.md)、[快捷键](../architecture/interface/keyboard-shortcuts.md)
- [存储格式](../architecture/editor/document-model/document-storage.md)、[测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

实现三种公式载体的 Caret 持久化模型，以及公式插入、源码编辑、位置与键盘行为。MathEdit 标识目标公式、活动编辑分支和分支内位置；源码模式使用 UTF-8 字节偏移，文本岛模式复用正文 TextFlow 位置语义。Paragraph 行内树、公式文本分支和两者的编辑适配遵循[数学公式架构](../architecture/editor/math.md)，不嵌套 Paragraph block 或复制正文编辑逻辑。首版无自动编号、无 `\label` / `\ref`；支持的分隔符、行分隔和失败保留规则以 011 为准。保留 `defaultMathFontFamily`，新文档默认 STIX Two Math，首版只支持该字体。

本任务不实现 Qt 绘制和大范围布局优化，不写入第三方 AST，不实现表格行列编辑。

## 前置条件与待决策

010 结构导航与 042 后端验证已完成。Caret 数学模型由 `caret-doc` 持有；`caret-math` 解析器直接产出 Caret 节点，第三方布局 AST 不进入 `.cdoc`。页面布局、绘制、命中与光标几何属于 044；表格单元格模型/编辑与画布集成分别属于 045/046。

## 实施步骤

1. 校验 042 提供的三种公式节点、稳定 ID、源内容与深度/长度限制；确认模型由 Caret schema 持有。
2. 实现 `\math`、`\equation`、`\align` 插入，源码更新、分组/命令停靠点、空槽编辑、Align 行分隔和公式进出；正文格式命令与公式输入分流。
3. 将 `Paragraph.inlines` 统一为 `Vec<ParagraphContent>`；043 接入 Text、Math 和 InlineEnvironment 分支，Code 仅保留后续扩展位置。MathFlow 与 Paragraph 共用 TextFlow 载体，固定数学槽位各自保留 MathFlow。抽取现有段落文本存储/操作边界；InlineEnvironment 子项使用同一混合序列，`\bold`、斜体、字体与 Math 通过通用 split/merge 操作维护范围，underline 仍由 Run 属性适配器处理。普通正文根流中的公式 TextFlow 必须提升为 Paragraph 文本项；经过格式包装时递归转换子项但保留单个包装节点。嵌套分数等固定槽位内的切分不得越过槽边界。文本岛适配器沿树路径复用正文输入法和编辑事务，不构造内容副本。
4. 将每次公式和文本岛编辑作为原子事务接入撤销/重做。解析、范围、兼容性或输入法替换失败时保留原文档与有效焦点状态。
5. 覆盖插入点拆分、粘贴、文本岛输入法替换、稳定 ID/未知字段、错误恢复、边界导航及保存往返。
6. 按[数学公式架构](../architecture/editor/math.md)定稿 ParagraphContent/MathFlow 节点、宿主参数化的 InlineEnvironment、TextFlow 持久化字段与 sourceMap、MathTree schema 版本及 `caret.paragraph.content-flow.v1`、`caret.math.content-flow.v1`、`caret.math.text-flow-source-map.v1` 能力。未发布的临时 inline/TextContent 格式不兼容、不迁移。编辑尽量局限于所属树分支；性能结论须基于代表性公式的节点数、分配、扫描、内存和序列化测量，不因共享实现直接推断性能提升。
7. 更新 011 与架构文档中的职责和数据契约，独立评审后复验。

## 预计改动

公式节点和文档验证在 042 已由 `crates/caret-doc` 提供；本任务集中于 `crates/caret-engine` 的公式命令、编辑状态、事务/历史与位置行为。`caret-app` 只在命令或快照边界确有需要时调整，不在 Qt/QML 重复数学编辑规则。

## 验收标准

- [ ] 三种公式类型使用 Caret 自有 `.cdoc` schema、稳定 ID 与未知字段保留；ParagraphContent 和 MathFlow 使用定稿 schema，并按文档存储规则声明两项必需能力。未发布临时格式不提供迁移路径。
- [ ] 目标 MathFlow 与 042 当前数学树布局不同，需完成模型/序列化/能力 schema 定稿及其往返验证；不提出紧凑表示或性能收益主张。任何性能优化需先记录代表性基线。
- [x] 首版编号环境、自动编号、`\label` 与 `\ref` 明确不作为无编号公式接受；不支持语法由解析器拒绝且不改动现有文档。
- [ ] 持久化 schema、页面绘制/光标投影及块布局接入后，命令可插入三种公式；在这些调用链完成前，桌面候选保持不可执行。数学源码与文本岛是平级编辑模式，文本岛复用正文光标、格式、IME 与撤销事务。
- [ ] 源码解析、无效范围和替换失败保持文档、编辑目标与 generation 不变；插入点拆分、粘贴、格式合并、文本岛切换和公式对象身份有回归测试。
- [x] 公式的绘制、版面命中、正文 shaping 与页面几何归 044；单元格模型/交互归 045，单元格画布集成归 046。
- [ ] caret-engine 直接消费者验证、格式与 lint 通过；分领域审查及独立最终评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-01 | `cargo test --offline --locked -p caret-doc`、`-p caret-app`、`-p caret-engine` | 模型、直接消费者及公式编辑行为通过 | caret-doc 52 项、caret-app 196 项、caret-engine 247 项通过 |
| 2026-11-01 | `cargo test --offline --locked -p caret-tests --test document` | `.cdoc` 往返、未知数据、能力声明与失败行为通过 | 20 项通过；当前格式 fixture 与手写 envelope 已声明 `caret.paragraph.content-flow.v1` |
| 2026-11-01 | `cargo test --offline --locked --workspace` | Workspace Rust 与 native 测试通过 | Rust 单元及集成套件通过；本机 native 聚合有 1 项 ShellLoad 失败：`documentPathAdapterReturnsLocalPathsAndDefaultSaveLocation` 断言 `default_url.isLocalFile()`，ShellLoad 其余 39 项及其他 7 个 CTest 均通过 |
| 2026-11-01 | `cargo test --offline --locked -p caret-engine`、`cargo clippy --offline --locked -p caret-engine --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | 数学编辑状态、元数据校验、源码模式 IME 边界及直接消费者通过 | 256 项 engine 测试通过；clippy、格式和差异检查通过。用例覆盖源码/命令查询禁用 IME、拒绝宿主格式副作用、Font 源码语义校验、结构选区保护、连续公式导航、显式退出、空公式删除及撤销边界 |
| 2026-11-01 | `cargo test --offline --locked -p caret-engine`、`cargo clippy --offline --locked -p caret-engine --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | 公式删除历史、方向回退和 TextFlow 扩展字段位置映射通过 | 259 项 engine 测试通过；clippy、格式和差异检查通过。新增断言覆盖空公式删除前后视觉 affinity 的 undo/redo、Escape 仅在优先方向无段落时回退，以及含未知 TextFlow 字段时仍可映射/移动/公式进出，普通编辑则原子拒绝以免丢字段 |
| 2026-11-01 | `cargo test --offline --locked -p caret-doc` | 正文与数学 TextFlow 的 sourceRange 宿主约束明确 | 53 项 caret-doc 测试通过；Paragraph TextFlow 携带数学 sourceRange 时在模型验证边界拒绝，数学树内的范围仍按 MathTree.source 验证 |
| 2026-11-01 | `cargo test --offline --locked -p caret-math` | 空文本岛保留可编辑、可序列化的树位置 | 37 项 caret-math 测试通过；`\text{}` 及格式包装中的空文本流不再被解析适配器静默省略，序列化往返保持空 TextFlow |
| 2026-11-01 | `cargo test --offline --locked -p caret-math text_flow_source_map_accounts_for_escaped_characters_and_empty_text`、`cargo test --offline --locked -p caret-doc math_text_source_map_declares_its_required_feature` | 公式文本光标具备可验证的显示文本/源码映射 | 定向测试通过；sourceMap 精确覆盖普通字节、转义字符和空 TextFlow 锚点，并声明 `caret.math.text-flow-source-map.v1` |
| 2026-11-01 | `cargo test --offline --locked -p caret-doc -p caret-math -p caret-engine` | TextFlow 映射范围、解析语义与元数据事务通过 | caret-doc 54 项、caret-math 38 项、caret-engine 262 项通过；覆盖 UTF-8 边界、空文本锚点、转义和折叠空白映射、映射缺口、sourceMap 与解析器逐项一致、未知映射字段保留、重复键歧义拒绝，以及删除带未知映射元数据的文本时文档/焦点/generation 原子不变 |
| 2026-11-01 | `cargo test --offline --locked --workspace` | Workspace Rust 与 native 聚合验证 | 全部 Rust 单元/集成测试通过；native 7/8 通过。既有 `Qml.ShellLoad` 在 `documentPathAdapterReturnsLocalPathsAndDefaultSaveLocation` 因 `default_url.isLocalFile()` 失败，未触及本次数学改动；其余 QML、格式、启动和构建检查通过 |
| 2026-11-01 | `cargo clippy --offline --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | lint、格式与差异检查通过 | 全部通过 |
| 2026-11-01 | `cargo test --offline --locked -p caret-engine`、`cargo clippy --offline --locked -p caret-engine --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff HEAD --check` | TextFlow 分支编辑、Align cell 坐标与失败原子性通过 | 281 项 engine 测试通过；clippy、格式和差异检查通过。覆盖空/非空文本岛，InlineMath/Equation/Align 输入，sourceMap 转义与空白，Run 属性保留、未知字段拒绝、跨 Run grapheme 删除、Align 包装/注释/转义/行列定位、TextFlow 删除后源码光标回退、两端边界往返、嵌套空岛穿越和 undo/redo。IME、文本选区及格式命令尚未接入 |
| 2026-11-01 | `cargo test --offline --locked -p caret-engine` | 正文与公式共用字素边界算法 | 282 项 engine 测试通过；TextMap 和数学文本光标复用 `GraphemeMap` 的边界、移动及 affinity 吸附规则。相邻 TextFlow/InlineEnvironment 仍未投影为统一逻辑流 |
| 2026-11-01 | `cargo test --offline --locked -p caret-engine`、`cargo clippy --offline --locked -p caret-engine --all-targets -- -D warnings`、`cargo fmt --all`、`git diff --check` | 混合流只读映射保持正文位置和编辑路径语义 | 284 项 engine 测试通过；通用 FlowMap 支持按需输出物理树路径，Paragraph TextMap 当前只请求逻辑文本、字素边界和旧 EditNode 写路径。覆盖跨 Run/环境组合字素、空 Run、空环境、软换行、公式原子、Opaque 拒绝与两种路径索引；clippy、格式和差异检查通过 |
| 2026-11-01 | Terra 分领域审查及 Sol/medium 最终只读审查 | 边界、行为、模型与存储无阻塞问题 | 最终审查无 P0–P2；修复缺少 runs/旧 text 静默变空及文档 DTO / capability 不一致 |
| 2026-11-01 | 性能与覆盖率门槛 | 记录代表性前后基线；Rust/native C++ coverage gate 通过 | 本轮未运行性能基准与覆盖率门槛；编辑工作副本仍复制行内段落，性能影响待基准验证 |

## 风险与回退

公式 schema 一旦发布需通过能力标识和版本迁移保持兼容；先验证必要字段和未知能力策略，再稳定持久化形式。布局器的库 AST 变化不能要求 `.cdoc` 迁移。

## 决策与工作记录

- 2026-10-29：从 011 拆出文档模型、编辑和导航任务；以 020/011 的产品语义为准。
- 2026-11-01：明确 043 的焦点为 `MathEdit` 源码字节位置；页面几何留给 044，表格模型与集成分别由 045/046 验收。
- 2026-11-01：数学源码和数学命令查询禁用 IME；TextFlow 活动分支接入后再启用正文输入法事务。命令查询只接收命令行白名单，公式源码模式拒绝正文格式命令，避免误改宿主段落。显示公式之间可按方向切换，显式退出跳过连续公式并恢复到可用正文边界；无目标时保留焦点。撤销分组合并在数学焦点迁移时切断。
- 2026-11-01：源码一致性验证比较 parser 可表达的属性，保留树独有的环境属性。数学 TextFlow 的任意 Font 覆盖仍不能单独表示：`fontFamily` 目前同时承载源命令映射与实际字体；在建立无损语义/覆盖映射前，不宣称自定义数学字体支持。正文 TextFlow 未知扩展字段现在不阻断只读位置映射，但编辑适配器仍在提交前拒绝无法归属的字段，以免降平时丢失。
- 2026-11-01：`TextFlow.sourceRange` 仅对数学树有宿主意义；正文段落不持有可供它引用的公式源码，因此模型验证拒绝 Paragraph TextFlow 的 sourceRange，避免把无效数据送到编辑层后才失败。
- 2026-11-01：解析器必须保留零长度文本岛；`\text{}` 现在生成一个空 Run 的 TextFlow，保证模型和序列化能表达文本编辑入口。Session 现有类型化 `MathTextFlowPath` 活动分支，可在 InlineMath、Equation 和 Align cell 中用 sourceMap 定位、编辑、重建路径并恢复历史位置；该实现尚未接入 IME、文本选区及行内格式命令。
- 2026-11-01：TextFlow 新增 `sourceMap`，把显示字节区间逐段映射到公式源码，防止转义字符被按相同字节长度错误换算；不等长映射项只能按边界整体处理。普通段落禁止这类数学源映射，公式树能力由 `caret.math.text-flow-source-map.v1` 明确声明。
- 2026-11-01：`sourceMap` 的模型校验要求显示字节区间连续覆盖文本且边界位于 UTF-8 字符边界；engine 将存储映射逐项与解析器生成映射比较。源码重解析按映射键索引并保留映射项及 sourceRange 的未知字段；若文本删除/改写使字段无法唯一映射，事务拒绝并保留有效状态。解析构建和模型校验使用增量偏移/有序游标，避免密集短 Run 的重复全量扫描。

## 完成摘要

当前分支已将 `Paragraph.inlines` 和 MathTree 改为共享 TextFlow 的强类型混合流，加入 Paragraph/MathFlow 能力声明，并让解析、序列化、数学验证、历史估算和段落位置映射消费新模型。旧的临时 inline/TextContent 格式没有转换器；旧 MathTree schema 只读预览，不进入新编辑树。公式源码模式和命令查询禁用 IME；Math TextFlow 已接入类型化路径、sourceMap 光标定位、直接 Run 编辑、解析验证、元数据迁移和单次历史事务，覆盖 InlineMath、Equation 与 Align cell。正文与单个数学 TextFlow 共用 GraphemeMap；通用 FlowMap 可按需输出物理树路径，正文 TextMap 当前只请求逻辑文本、字素边界和旧 EditNode 写路径。MathFlow 尚未接入统一 FlowMap。跨相邻 TextFlow/InlineEnvironment 的合并字素导航与环境边界删除尚未实现；删除整个 TextFlow 后暂回源码分支，后续输入仍按数学源码语义处理。文本岛仍未复用正文完整的选区、IME、格式适配和事务入口；文本编辑仍复制宿主并完整重解析公式。正文根流提升、包装环境转换及数学文本岛自定义字体覆盖尚未完成。桌面公式绘制/光标投影尚未接入，公式命令候选保持不可执行。性能成本尚无代表性基准，不宣称共享载体已带来性能收益。044 负责公式页面布局，表格集成归 045/046。
