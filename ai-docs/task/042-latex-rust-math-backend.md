# 042 — M3：latex-rust 数学后端验证

- 状态：done
- 依赖：无
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-10-31

## 目标与背景

在公式数据模型落地前，以小型可运行原型确认 `latex-rust` 是否能承担 LaTeX 数学解析和 TeX 数学盒布局，并为 Caret 画布提供足够的几何与字形信息。避免重复实现 parser/layout，也避免过早把不适合的 renderer 接入 Qt。总体验收由 [011 M3 公式](011-impl-math.md)管理。

## 必读

- [公式与表格](../architecture/editor/math-and-table.md)、[文档模型](../architecture/editor/document-model.md)、[研究报告 010](../research/010-latex-rust.md)
- [`latex-rust` API](https://docs.rs/latex-rust/latest/latex_rust/)、[crate 源码](https://github.com/jscarr64/LaTeX-Rust)
- [测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

检查当前可用版本、MSRV、许可证、依赖体积、公开 AST/MathBox API、字体嵌入方式、深度限制、已知安全公告和维护状态。建立不进入生产 workspace 的适配原型，覆盖行内、单行显示、多行对齐公式及 `\text{...}` 的正文 shaping 需求；选用上游已支持的 `\[...\]` 与 `\[\begin{aligned}...\end{aligned}\]` 输入。

本任务不实现完整编辑器、持久化格式或 Qt renderer，不以 SVG/PNG-only 结果证明可编辑性，也不为得到结论而复制已有 TeX 排版实现。

## 前置条件与待决策

无。原型必须在任务记录写明依赖版本与锁文件证据；安全公告以实际选定版本复核。若公开 API 不够，结论应指出缺少的最小能力及其影响。

## 实施步骤

1. 核对 crate 元数据、许可证、依赖树、MSRV、深层嵌套修复版本与可配置深度限制。
2. 编译原型，将 `MathBox` 的 glyph、rule、基线、尺寸和层级映射到简化的 Caret 快照；确认命中测试和光标位置所需的节点映射是否可获得。
3. 验证 `\text{...}` 能否注入 Caret shaping 的字形/度量，并支持语言标签和字体 fallback；记录不支持的文本范围。
4. 对照 011 确认 `InlineMath`、`Equation`、`Align` 输入边界和无编号规则；记录版本决策、已知限制和最小补丁建议。
5. 删除原型临时文件或将其隔离到明确的非生产目录；独立评审结论。

## 预计改动

依赖审计、临时原型和适配边界；本任务确定后续输入与接口决策，不把公式 parser/layout 移入 Caret 自行实现。043 创建 `caret-math` parser crate，044 扩展布局接口和 Cargo patch。

## 验收标准

- [x] 记录锁定版本、许可证、MSRV、依赖体积、安全公告与深度上限依据。
- [x] 原型给出公开 MathBox 到 Caret glyph/rule/几何快照的可行性证据；不能映射的字段有明确列表。
- [x] `\text{...}` 文字 shaping 与语言/字体 fallback 路径已验证或明确限定支持范围，源文本保留策略已确定。
- [x] 输出明确的采用/不采用结论和后续实现约束；没有重复实现 crate 已有能力。
- [x] 原型不进入生产依赖，临时工程与构建产物已删除；最终独立评审通过。

## 决策

采用 `latex-rust 3.0.1` 处理 LaTeX 数学解析和 TeX 布局。Caret 使用已由上游支持的 `\[...\]` 和 `\[\begin{aligned}...\end{aligned}\]` 输入；公式产品行为见[架构规范](../architecture/editor/math-and-table.md)。持久化 schema 由 `caret-doc` 拥有，后端适配由低层 `caret-math` 承担；043 创建 parser 边界，044 扩展布局 API。详细 crate 元数据、锁定依赖、原型证据和适配要求统一见[研究报告 010](../research/010-latex-rust.md)。

## 验证结果

| 日期 | 命令 | 实际结果 |
| --- | --- | --- |
| 2026-10-31 | `cargo +1.99.0 check --locked --workspace --all-targets` | 全部 workspace crates 和 targets 成功编译；workspace MSRV 为 1.99 |

隔离原型已按维护者要求连同构建产物删除；其余 crate 与原型验证细节记录在研究报告 010。原型未进入生产依赖。

## 风险与回退

后续解析、布局、shaping、节点映射和深度边界按[研究报告 010](../research/010-latex-rust.md)及 043/044 验收；不另写 TeX parser/layout，也不以 SVG/PNG 代替可编辑几何快照。

## 决策与工作记录

- 2026-10-29：从 011 拆出后端/API 验证任务，约束 043/044 的模型与适配边界。
- 2026-10-31：维护者确定 workspace MSRV 为 1.99；继续采用 `latex-rust 3.0.1`，由 `caret-math` 隔离版本和兼容扩展。
- 2026-10-31：Terra/medium 分领域检查与独立 Sol/medium 最终复审均通过；修正单一证据权威、深度预算、parser 源码范围归属、043/044 顺序及 011 验证记录。原型按维护者要求删除，未在最终复审中重跑；workspace 1.99 编译记录见上表。

## 完成摘要

后端调查与隔离原型完成。结论是保留 `latex-rust`，后续由 `caret-math` 承载适配和上游 patch；043/044 落实 MSRV、shaping、节点映射与依赖分层后接入生产。
