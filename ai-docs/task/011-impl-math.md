# 011 — M3：数学公式

- 状态：planned
- 依赖：010、042、043、044
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-29

## 目标与背景

实现可直接编辑的数学公式：行内数学、单行显示公式和多行对齐公式。编辑器以 LaTeX 数学语义命名与解析，不把数学源码当成普通正文。表格另见 012。

## 必读

- [公式与表格](../architecture/editor/math-and-table.md)、[命令输入](../architecture/editor/command-input.md)
- [快捷键](../architecture/interface/keyboard-shortcuts.md)、[结构导航](../architecture/editor/structure-navigation.md)
- [`latex-rust` 文档](https://docs.rs/latex-rust/latest/latex_rust/)，集成时核对实际锁定版本、API、许可证和安全公告

## 结构与 LaTeX 命名

文档保留 Caret 自己版本化的语义树；不得把第三方 crate 的 AST 直接作为 `.cdoc` 持久化格式。节点和编辑概念按 LaTeX 数学模式命名：

| Caret 结构 | LaTeX 对应 | 文档位置 |
| --- | --- | --- |
| `InlineMath` | inline math：`$...$`、`\(...\)` | 行内对象；占一个原子位置，内部用 `MathPos` |
| `Equation` | `equation*` / `\[...\]` / `$$...$$` | 独占一行的无编号显示公式块 |
| `Align` | `align*` | 多行无编号显示公式；每行有对齐单元格，`&` 表示对齐点，`\\` 表示换行 |

单行公式仍叫 `Equation`，不叫“单行文本”：公式可能宽于可用行宽，必须由公式布局明确处理，不能拆成普通文本行。LaTeX `\\` 只在 `align*` 内表示公式行分隔；正文 Shift+Enter 的 `SoftBreak` 与公式行分隔是不同节点。

首版只实现无编号显示公式，不实现自动编号、`\label` 或 `\ref`。编号版 LaTeX `equation` / `align` 环境必须明确报不支持并保留内容，不能映射成无编号节点后静默丢失语义。

解析和 TeX 数学盒布局优先评估并复用 [`latex-rust`](https://docs.rs/latex-rust/latest/latex_rust/)。它提供 typed AST、TeX 数学布局、单行/多行环境，并内嵌 STIX Two Math；本任务仍要验证其公开盒树能否映射到 Caret 布局快照、字形/规则绘制、命中测试及内部编辑路径。PNG 不可作为公式存储或缩放绘制结果；优先从布局盒树适配 Caret 的矢量/字形绘制管线。若公开 API 不足，先验证最小适配层或向上游补 API，不复制整套解析器/TeX 布局，也不直接把 `egui`/SVG/PNG 渲染器接到 Qt。

`.cdoc` 的数学树、节点 ID、占位符和 `MathPos` 属于 Caret 稳定模型；crate AST 是解析/布局边界类型，通过适配器转换。保留用户输入的 LaTeX 片段供错误定位和可选导出；解析/归一化后的文本回写是否有损由夹具记录，不假设源码往返等价。遇到不支持的语法要显示错误并保留原输入，不能丢弃内容或伪造渲染。

## 字体

LaTeX 没有唯一的“LaTeX 字体”。公式字体家族包括 Computer Modern / Latin Modern Math、STIX Two Math、Cambria Math、XITS Math、TeX Gyre Math 等；它们的风格、覆盖范围和数学度量不同。

M3 首版统一使用 `latex-rust` 内嵌的 **STIX Two Math**。保留 `defaultMathFontFamily` 作为稳定扩展入口；新文档未设置时默认为 `STIX Two Math`，M3 只支持该字体且不提供多字体切换 UI。其他字体请求保留在文档中但当前回退到 STIX Two Math，并产生可诊断提示；不得静默改写用户设置。以后确需扩展字体列表时另立任务，需同时支持 OpenType `MATH` 度量、glyph fallback、缓存代际、文档重现性和跨平台验收。

数学字体不止一种；M3 暂不提供切换。公式中的 `\text{...}` 应由 Caret 正文 shaping 与语言/字体回退处理，而不是强制用数学字体。适配原型必须验证文本节点的度量和位置能否回映到 Caret；不能支持时保留源文本并提供明确错误。

## 范围与非目标

在现有画布内完成上述三种公式结构。实现 `insert.inline_math`、`insert.equation`、`insert.align`、`equation.move`、`equation.exit`，以及 `$` / `\(`、`$$` / `\[`、`\frac`、`\sqrt`、下标和上标。公式内部的 `\\` 不打开正文命令模式；`Align` 内才解释为公式行分隔；输入形式使用 LaTeX 无编号 `align*`。

不包含：表格自身的插入和行列编辑、公式图片导出。单元格内公式由 044 按[公式与表格的集成规则](../architecture/editor/math-and-table.md#m3-集成验收)统一集成；011 汇总验收必须确认该证据已完成。

## 前置条件与待决策

010 当前仍是 planned；公式模型与交互须等其结构导航能力完成。042 可以先独立验证 `latex-rust`，043 依赖 010 和 042，044 依赖 043、012 与 027。

## 拆分与汇总

M3 按可独立验收的边界拆成以下任务；子任务遵守本文和[公式与表格架构](../architecture/editor/math-and-table.md)，各自维护实现与验证记录。011 保留为端到端验收任务。

| 子任务 | 范围 | 完成后提供的证据 |
| --- | --- | --- |
| [042：latex-rust 后端验证](042-latex-rust-math-backend.md) | 依赖/API/许可证/安全核验、MathBox 适配与文本节点原型 | 锁定版本；确认公开布局数据足以进入 Caret 快照，列明限制和替代方案 |
| [043：公式模型与编辑](043-math-model-and-editing.md) | `.cdoc` 数学节点、命令、公式内部位置、编辑和导航 | 三类公式可创建、编辑、保存往返；未知/无效输入失败原子 |
| [044：公式布局与画布渲染](044-math-layout-and-rendering.md) | MathBox 到 Caret 整数布局/绘制快照，命中、光标几何及表格集成 | Rust/QML/Qt 显示共用一份几何结果；表格内公式完成集成验收 |

汇总步骤：先确认 010 结构导航和 012 表格依赖满足；完成 042–044；按本文所有验收项做端到端核对、独立评审并复验。子任务完成不自动代表 M3 通过。

## 预计改动

整体涉及 `caret-doc`、`caret-engine`、`caret-layout`、`caret-structure`、`latex-rust` 适配器和画布公式绘制，具体分配见 042–044。FFI 只传公式快照，不把 Rust 树或 crate 类型交给 QML。

## 验收标准

- [ ] `InlineMath`、`Equation` 和 `Align` 的节点名、文档位置和 LaTeX 映射一致；`$` / `\(` 插入行内公式，`$$` / `\[` 插入显示公式，LaTeX `align*` 的 `&`/`\\` 对齐和换行可用。
- [ ] 数学语法由 `latex-rust` 解析与排版，或记录其公开 API 阻碍及已验证的最小上游/API 适配；没有重复实现已有 parser/layout 能力。
- [ ] `.cdoc` 持久化格式不暴露依赖 crate AST 版本；首版使用内嵌 STIX Two Math；`defaultMathFontFamily` 保留为稳定扩展入口，未设置时默认为 STIX Two Math，当前只支持该字体；其他请求保留且回退时报告诊断。
- [ ] 过深/不支持的公式输入不会使进程崩溃或丢失源文本；错误位置可定位，布局失败原子处理。
- [ ] 方向键在公式边界进出。Esc 离开公式，插入点在公式之后。
- [ ] 正文命令模式不会在公式内打开。
- [ ] 044 已完成并验证单元格公式的插入、编辑、导航、绘制、命中、撤销和保存往返。
- [ ] 往返夹具有明确的相等或已知差异。测试、fmt、clippy 通过。评审发现已处理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `latex-rust` crate/API/安全与 MathBox 适配原型 | 锁定兼容版本并证明布局可映射；确认深度上限 | 未执行 |
| — | 三种公式结构的解析、编辑、序列化往返与导航测试 | 节点/位置/错误行为符合本任务映射 | 未执行 |
| — | 三平台数学字形和显示公式手工验证 | 使用同一内嵌数学字体且基线/尺寸一致 | 未执行 |
| — | 044 单元格公式集成证据 | 插入、编辑、导航、绘制、命中和保存往返通过 | 未执行 |

## 风险与回退

MathBox 到 Caret 字形快照的映射若需要上游支持，先补最小适配能力。不能以 SVG/PNG-only 管线作为可编辑公式的回退实现。依赖不可维护或 API 不足时保留 Caret 自有文档模型，只替换不合适的依赖边界。

## 决策与工作记录

- 2026-10-25：创建任务。
- 2026-10-29：公式模型使用 `InlineMath`、`Equation`、`Align` 类型名，并以 LaTeX 小写环境名映射；先评估并复用 `latex-rust` 的 parser/MathBox，不将第三方 AST 写入 `.cdoc`。
- 2026-10-29：M3 首版统一使用内嵌 STIX Two Math，保留 `defaultMathFontFamily` 作为未来扩展入口，首版默认并仅支持 STIX Two Math；其他数学字体切换留待单独需求。

## 完成摘要

未完成。库适配、MathBox 到 Caret 快照的可行性与三种公式交互仍待实现和验证。
