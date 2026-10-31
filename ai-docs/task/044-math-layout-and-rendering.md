# 044 — M3：公式布局、画布渲染与表格集成

- 状态：planned
- 依赖：012、027、043
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-10-31

## 目标与背景

通过独立 `caret-math` crate 适配 `latex-rust` 的数学盒布局，并精确、高效地呈现到 Caret 页面画布。绘制、命中测试、光标几何和选区共用同一 Rust 侧布局快照；Qt 只消费快照并绘制。完成 M3 规定的表格内公式可视集成。总体验收由 [011 M3 公式](011-impl-math.md)管理。

## 必读

- [公式与表格](../architecture/editor/math-and-table.md)、[页面画布与命中测试](../task/027-canvas-and-hit-testing.md)、[表格](../task/012-impl-table.md)
- [分层](../standards/layering.md)、[数值规范](../standards/numeric-values.md)、[测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

在 043 创建的低层 `caret-math` workspace crate 中扩展 `latex-rust` pin/patch、输入适配和 MathBox 快照 API；为 token/AST 增加细粒度源码字节范围及预处理偏移映射。接通块级 `Equation`/`Align` 的交互式创建、MathFocus 编辑和命令入口，补齐 `^`/`_` 上下标插入及已提交数学内容的 Backspace/Delete；确保可见布局可用后才开放候选。`caret-math` 不依赖 Qt、Parley 或 `caret-doc`，向 `caret-layout` 暴露调用方注入的 shaping provider 与稳定节点 provenance。实现 MathBox 到 Caret 布局快照的适配；包含 glyph ID/位置、规则线、基线、盒尺寸、文本片段映射及必要的字体资源身份。度量使用项目统一的整数表示。Qt 使用既有字形 atlas/QSG 路线，不把 QML 变为公式布局权威。

覆盖段落行内公式、独占行 `Equation`、多行 `Align`，公式字体默认 STIX Two Math，并验证文档 `defaultMathFontFamily` 的唯一支持值与诊断回退；公式内 `\text{...}` 使用 Caret 正文 shaping。接通表格单元格内公式的绘制、命中、光标与布局。

不实现编号公式、字体切换 UI、SVG/PNG-only 可编辑渲染、独立 GPU 基准或大范围段落排版优化。

## 前置条件与待决策

依赖 043 创建的 parser crate、整段公式 source range 和稳定公式树，以及 042 的后端边界结论。依赖 012、027 已完成；在 1.99 工具链验证补丁包和依赖树，增加细粒度源码范围映射、正文 shaping provider 与布局盒到 Caret 节点的 provenance。Caret 树到 parser/layout 的深度预算映射须验证；超过上游默认预算的导入或编辑须原子拒绝。输入采用 `\[...\]` 和 `\[\begin{aligned}...\end{aligned}\]`；不为未发布的公式语法增加星号环境 parser patch。如果上游 patch 不能满足契约，暂停集成并复核 patch 范围，不另写 TeX parser/layout，也不用 SVG/PNG 绘制绕开契约。

## 实施步骤

1. 定义缓存键和 generation：公式树、字体、字号、语言、缩放/分辨率中哪些影响布局、绘制及 atlas 缓存。
2. 把数学盒输出映射到 Caret 整数坐标与字形/rule 快照；确认 `\text{...}` 混合正文 shaping 时尺寸与基线一致。
3. 通过适配层 patch 暴露 token/AST 细粒度源码范围和预处理映射；用错误位置用例验证原始输入定位。
4. 接通 `\equation`/`\align` 插入命令和显示公式 MathFocus 编辑，MathFocus 路径覆盖行内与块级结构。
5. 实现 `^`/`_` 上下标插入和已提交数学节点的 Backspace/Delete，定义结构边界、占位槽、撤销及失败原子行为。
6. 接入画布绘制、光标几何、命中测试与选择范围；让正文方向键在相邻的行内公式边界进入既有公式 MathFocus，并由同一布局快照选择入口 MathPos；验证左右两侧边界、连续移动、退出及正文光标投影一致，输入更新不会重建未受影响的公式/段落快照。
7. 将同一公式路径接入表格单元格，覆盖单元格尺寸变化、分页、命中和导航。
8. 补充具有行为断言的布局/渲染/缓存测试，做性能对比；独立架构与代码评审并处理发现。

## 预计改动

`crates/caret-layout` 数学布局适配，`native` 画布 glyph/rule 绘制，必要的 FFI 快照传输与 Qt 节点绘制；限制为公式内容所需的最小边界。

## 验收标准

- [ ] `InlineMath` 与块级 `Equation`/`Align` 可显示；多行对齐、基线、括号/分数等伸展符号使用数学布局度量。
- [ ] `\math`、`\equation`、`\align` 均可创建并进入相应公式焦点；`\frac`、`\sqrt` 以及行内/块级 `^`、`_` 编辑候选可用。候选只在对应公式渲染和焦点投影可用时开放，创建与编辑可撤销。
- [ ] 行内与块级公式均支持 `^`/`_` 上下标插入，以及已提交文字/节点的 Backspace/Delete；结构边界行为明确，撤销与失败原子测试通过。
- [ ] parser token/AST 提供 UTF-8 源码字节范围和预处理偏移映射；错误位置能映射回原始输入，并覆盖转义、规范化和边界位置。
- [ ] 绘制、命中、光标几何引用同一 generation/几何快照，旧异步结果不能覆盖新布局。
- [ ] 正文方向键可从行内公式两侧进入已有公式，入口 `MathPos` 由布局几何确定；公式左右边缘退出后正文 caret 与快照一致，覆盖保存重开、连续公式及相邻文本边界。
- [ ] 英文、中文 `\text{...}` 按正文字体与语言 shaping 显示；缺失字体/字形有定义明确的 fallback/诊断；未知 `defaultMathFontFamily` 保留文档原值，实际回退到 STIX 时可观测并可测试。
- [ ] 同一公式快照在缩放与三平台检查中不依赖临时栅格图；页面内编辑没有显著重复重排，性能结果记录基线和测试输入。
- [ ] 表格内公式的插入、显示、命中、导航和保存重开已与 012 集成验收。
- [ ] 无 SVG/PNG-only 交互路径；测试、格式、lint、独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | caret-layout 数学盒快照行为测试 | 坐标、基线、字形与源位置映射一致 | 未执行 |
| — | parser adapter token/AST source range 测试 | UTF-8 字节跨度及预处理偏移映射能定位原始输入 | 未执行 |
| — | `\equation` / `\align` 命令与块级 MathFocus 测试 | 两种显示公式可创建、编辑、撤销并与布局快照同步 | 未执行 |
| — | 上下标和公式删除命令测试 | 行内/块级结构编辑、光标位置、撤销及边界失败一致 | 未执行 |
| — | Qt 页面画布人工/组件验证 | 呈现、命中和光标位置吻合 | 未执行 |
| — | 表格内公式集成测试 | 表格单元格内可编辑且保存往返稳定 | 未执行 |
| — | 公式布局基准 | 记录输入、构建与缓存条件 | 未执行 |

## 风险与回退

字体缺字、MATH 度量差异、Qt atlas 生命周期或文本 shaping 混排可能造成错位。失败时保留公式源与错误状态，不能退回无法定位和选择的图片对象。性能优化限定在测量证明的热路径。

## 决策与工作记录

- 2026-10-29：从 011 拆出公式布局、画布渲染和与 012 的最终集成验收。

## 完成摘要

未完成。
