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
6. 按[数学公式架构](../architecture/editor/math.md)定稿 ParagraphContent/MathFlow 节点、宿主参数化的 InlineEnvironment、TextFlow 持久化字段、MathTree schema 版本及 `caret.paragraph.content-flow.v1`、`caret.math.content-flow.v1` 能力。未发布的临时 inline/TextContent 格式不兼容、不迁移。编辑尽量局限于所属树分支；性能结论须基于代表性公式的节点数、分配、扫描、内存和序列化测量，不因共享实现直接推断性能提升。
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
| 2026-11-01 | `cargo clippy --offline --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | lint、格式与差异检查通过 | 全部通过 |
| 2026-11-01 | Terra 分领域审查及 Sol/medium 最终只读审查 | 边界、行为、模型与存储无阻塞问题 | 最终审查无 P0–P2；修复缺少 runs/旧 text 静默变空及文档 DTO / capability 不一致 |
| 2026-11-01 | 性能与覆盖率门槛 | 记录代表性前后基线；Rust/native C++ coverage gate 通过 | 本轮未运行性能基准与覆盖率门槛；编辑工作副本仍复制行内段落，性能影响待基准验证 |

## 风险与回退

公式 schema 一旦发布需通过能力标识和版本迁移保持兼容；先验证必要字段和未知能力策略，再稳定持久化形式。布局器的库 AST 变化不能要求 `.cdoc` 迁移。

## 决策与工作记录

- 2026-10-29：从 011 拆出文档模型、编辑和导航任务；以 020/011 的产品语义为准。
- 2026-11-01：明确 043 的焦点为 `MathEdit` 源码字节位置；页面几何留给 044，表格模型与集成分别由 045/046 验收。

## 完成摘要

当前分支已将 `Paragraph.inlines` 和 MathTree 改为共享 TextFlow 的强类型混合流，加入 Paragraph/MathFlow 能力声明，并让解析、序列化、数学验证、历史估算和段落位置映射消费新模型。旧的临时 inline/TextContent 格式没有转换器；旧 MathTree schema 只读预览，不进入新编辑树。正文 TextMap 的只读扫描直接遍历新模型，但编辑与格式操作仍经临时 `EditNode` 工作副本复用旧树算法；完整的宿主路径直接编辑、正文根流提升与包装环境转换仍需验证。桌面公式绘制/光标投影尚未接入，公式命令候选保持不可执行。性能成本尚无代表性基准，不宣称共享载体已带来性能收益。044 负责公式页面布局，表格集成归 045/046。
