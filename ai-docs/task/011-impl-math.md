# 011 — M3：数学公式

- 状态：planned
- 依赖：010、042、043、044、045、046
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-11-01

## 目标与背景

实现可直接编辑的数学公式：行内数学、单行显示公式和多行对齐公式。编辑器以 LaTeX 数学语义命名与解析，不把数学源码当成普通正文。表格另见 012。

## 必读

- [公式与表格](../architecture/editor/math-and-table.md)、[命令输入](../architecture/editor/command-input.md)
- [快捷键](../architecture/interface/keyboard-shortcuts.md)、[结构导航](../architecture/editor/structure-navigation.md)
- [研究 010：Typst 数学排版移植](../research/010-typst-math-layout.md)、[源码署名](../standards/source-attribution.md)

## 结构与 LaTeX 命名

文档保留 Caret 自己版本化的语义树，由 `caret-math` 解析器直接产出。节点和编辑概念按 LaTeX 数学模式命名：

| Caret 结构 | LaTeX 对应 | 文档位置 |
| --- | --- | --- |
| `InlineMath` | inline math：`$...$`、`\(...\)` | 行内对象；占一个原子位置，内部由 `MathEdit` 源码字节游标定位 |
| `Equation` | `equation*` / `\[...\]` / `$$...$$` | 独占一行的无编号显示公式块 |
| `Align` | `align*` | 多行无编号显示公式；每行有对齐单元格，`&` 表示对齐点，`\\` 表示换行 |

单行公式仍叫 `Equation`，不叫“单行文本”：公式可能宽于可用行宽，必须由公式布局明确处理，不能拆成普通文本行。LaTeX `\\` 只在 `align*` 内表示公式行分隔；正文 Shift+Enter 的 `SoftBreak` 与公式行分隔是不同节点。

首版只实现无编号显示公式，不实现自动编号、`\label` 或 `\ref`。编号版 LaTeX `equation` / `align` 环境必须明确报不支持并保留内容，不能映射成无编号节点后静默丢失语义。

解析与排版由工作区内的 `caret-math` 完成（[042](042-math-backend.md)）：自写 LaTeX 数学子集解析器与序列化器，排版移植自 Typst v0.15.1，度量来自 OpenType MATH 表。排版输出字形与矩形，直接进入 Caret 页面字形快照，PNG/SVG 不作为公式存储或交互路径。

`.cdoc` 的数学树、节点 ID 和占位符属于 Caret 稳定模型；当前公式内部编辑用 `MathEdit` 的源码字节游标定位。编辑中的 `source` 是解析输入权威，成功解析后与 `root` 一起提交；不支持的语法显示错误并保留原输入，不能丢弃内容或伪造渲染。

## 字体

LaTeX 没有唯一的“LaTeX 字体”。公式字体家族包括 Computer Modern / Latin Modern Math、STIX Two Math、Cambria Math、XITS Math、TeX Gyre Math 等；它们的风格、覆盖范围和数学度量不同。

M3 首版统一使用内置的 **STIX Two Math**。保留 `defaultMathFontFamily` 作为稳定扩展入口；新文档未设置时默认为 `STIX Two Math`，M3 只支持该字体且不提供多字体切换 UI。其他字体请求原样保留在文档中，当前仍用 STIX Two Math 排版；不得静默改写用户设置。以后确需扩展字体列表时另立任务，需同时支持 OpenType `MATH` 度量、glyph fallback、缓存代际、文档重现性和跨平台验收。

数学字体不止一种；M3 暂不提供切换。公式中的 `\text{...}` 目标是由 Caret 正文 shaping 与语言/字体回退处理；当前用数学字体正体字形排版，由 044 继续实现。

## 范围与非目标

在现有画布内完成上述三种公式结构。实现 `insert.inline_math`、`insert.equation`、`insert.align`、`equation.move`、`equation.exit`，以及公式内的 `\frac`、`\sqrt`、下标和上标；`$...$`、`\(...\)`、`\[...\]` 仅作为公式源码解析/导入语法。公式内部的 `\\` 不打开正文命令模式；`Align` 内才解释为公式行分隔；输入形式使用 LaTeX 无编号 `align*`。

命令面板别名固定为 `\math`（行内）、`\equation`（单行显示）、`\align`（多行对齐）。这些是结构插入命令；`\(`、`\[` 仅保留为公式源码定界写法，不作为命令候选。

不包含：表格自身的插入和行列编辑、公式图片导出。单元格内公式由 045 接入模型、编辑和导航，由 046 接入布局、画布、命中、光标和高亮；011 汇总验收必须确认两部分证据齐全。

## 前置条件与待决策

010 结构导航与 042 后端已完成；043、044 进行中，表格模型编辑与公式布局集成分别由 045/046 承接。

## 拆分与汇总

M3 按可独立验收的边界拆成以下任务；子任务遵守本文和[公式与表格架构](../architecture/editor/math-and-table.md)，各自维护实现与验证记录。011 保留为端到端验收任务。

| 子任务 | 范围 | 完成后提供的证据 |
| --- | --- | --- |
| [042：数学后端选型与验证](042-math-backend.md) | `caret-math` 解析、序列化与 Typst 移植排版 | 后端测试与 `cargo math-backend` CI 任务通过；移植来源记录完整 |
| [043：公式模型与编辑](043-math-model-and-editing.md) | `.cdoc` 数学节点、命令、公式内部位置、编辑和导航 | 三类公式可创建、编辑、保存往返；未知/无效输入失败原子 |
| [044：公式布局与画布渲染](044-math-layout-and-rendering.md) | 正文公式排版接入段落布局与页面字形快照，完成环境高亮、命中与光标几何 | 正文数学路径共用一份 Rust 几何结果 |

汇总步骤：先确认 010 结构导航完成；完成 042–044，再由 045/046 完成单元格公式集成；按本文验收项做端到端核对、独立评审并复验。子任务完成不自动代表 M3 通过。

## 预计改动

整体涉及 `caret-doc`、`caret-math`、`caret-engine`、`caret-layout`、`caret-structure` 和画布公式绘制，正文数学由 042–044 交付，单元格集成由 045/046 承接。FFI 只传公式快照，不把 Rust 树或 crate 类型交给 QML。

## 验收标准

- [ ] `InlineMath`、`Equation` 和 `Align` 的节点名、文档位置和 LaTeX 映射一致；`\math`、`\equation`、`\align` 分别插入行内、单行和多行结构，LaTeX `align*` 的 `&`/`\\` 对齐和换行可用。
- [x] 数学语法由 `caret-math` 解析与排版；排版移植自 Typst，来源与修改记录在文件头和 `NOTICE`。
- [x] `.cdoc` 持久化格式是 Caret 自有 schema；首版使用内置 STIX Two Math；`defaultMathFontFamily` 保留为稳定扩展入口，其他值原样保留。
- [ ] 过深/不支持的公式输入不会使进程崩溃或丢失源文本；错误位置可定位，布局失败原子处理。
- [ ] 方向键在公式边界进出。Esc 离开公式，插入点在公式之后。
- [ ] 正文命令模式不会在公式内打开。
- [ ] 045 已完成单元格公式模型、插入编辑、导航、撤销和保存往返。
- [ ] 046 已完成单元格公式布局、绘制、命中、光标和高亮。
- [ ] 往返夹具有明确的相等或已知差异。测试、fmt、clippy 通过。评审发现已处理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-01 | Linux：`cargo math-backend` | 解析、往返、排版与缓存测试通过；深度上限生效 | 通过 |
| — | 三种公式结构的解析、编辑、序列化往返与导航测试 | 节点/位置/错误行为符合本任务映射 | 未执行 |
| — | 三平台数学字形和显示公式手工验证 | 使用同一内嵌数学字体且基线/尺寸一致 | 未执行 |
| — | 045/046 单元格公式集成证据 | 模型编辑、导航、绘制、命中、光标、高亮和保存往返通过 | 未执行 |

## 风险与回退

移植的排版代码需人工对照 Typst 后续版本同步修正。不能以 SVG/PNG-only 管线作为可编辑公式的回退实现。

## 决策与工作记录

- 2026-10-25：创建任务。
- 2026-10-29：公式模型使用 `InlineMath`、`Equation`、`Align` 类型名，并以 LaTeX 小写环境名映射；数学树为 Caret 自有 schema。
- 2026-11-01：后端为自写解析器加 Typst 移植排版，见 042 与研究 010。
- 2026-10-29：M3 首版统一使用内置 STIX Two Math，保留 `defaultMathFontFamily` 作为未来扩展入口，首版默认并仅支持 STIX Two Math；其他数学字体切换留待单独需求。

## 完成摘要

未完成。后端、模型、三种公式的命令插入、公式内编辑与正文显示已完成；`\text` 正文 shaping、统一环境高亮与 045/046 表格集成待做。
