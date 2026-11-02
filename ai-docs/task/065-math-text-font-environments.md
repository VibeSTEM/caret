# 065 — 数学文本字体环境与 `\textsf` / `\texttt`

- 状态：planned
- 依赖：043、044、064
- 优先级：P2
- 创建 / 更新：2026-11-02 / 2026-11-02

## 目标与背景

为数学 `\text{...}` 内的字体环境确定并实现可靠的字体语义，重点覆盖 `\textsf{...}` 与 `\texttt{...}`。仓库内置的 STIX Two Math 含数学无衬线和等宽字母数字字形，但这些是供技术符号使用的 Unicode 数学字母表变体，不是完整的文本字体族；当前布局把拉丁字母和数字映射到这些字形，其他字符仍由数学字体排版。仓库目前只带 STIX Two Math 文件；STIX Two 家族另外提供独立文本字体文件，是否引入需与复用 JetBrains Mono、应用包体积及缺字行为一起评估。[LaTeX 的字体选择规范](https://www.latex-project.org/help/documentation/fntguide.pdf)将 `\textsf` / `\texttt` 定义为文本字体族选择，不能默认等同于 `\mathsf` / `\mathtt` 数学字母表变体；[STIX Two 上游文件说明](https://github.com/stipub/stixfonts/blob/master/README.md)区分 Math 与 Text 字体文件。

当前 `caret-math` 对 `\textsf` / `\texttt` 使用 `InlineEnvironment::Font`，但布局没有独立文本字体实例或按 TextFlow 运行塑形的字体 family 选择；不支持的 family 还可能在公开布局入口静默退回 Roman。043 的 engine 源码一致性校验只保护文档接入边界，不能替代本任务的字体设计与后端诊断。

## 必读

- [数学公式架构](../architecture/editor/math.md)、[044 公式布局与画布](044-math-layout-and-rendering.md)、[064 内置字体角色](064-builtin-font-roles.md)
- [字体选择规范](../architecture/editor/font-selection.md)、[数值规范](../standards/numeric-values.md)、[验证与评审](../standards/validation-and-review.md)
- [LaTeX2e 字体选择规范](https://www.latex-project.org/help/documentation/fntguide.pdf)（查阅：2026-11-02）
- [STIX 字体项目](https://stixfonts.org/)（查阅：2026-11-02）

## 范围与非目标

- 决定数学 `\textsf` / `\texttt` 是解析为正文字体 family 语义，并由文本 shaping 使用合适字体；实现字体选择、回退、字形度量和基线适配。
- 评估 `\texttt` 与内置 JetBrains Mono（064 的 Code 角色）、`\textsf` 与可复用的正文/内置字体角色之间的关系；记录内置资源、许可、包体积、字体查询和跨平台一致性成本后再选定映射。
- 明确文本字体如何与 `\textbf` / `\textit` 组合，如何处理标点、空格、数字、拉丁字母、希腊字母、CJK、缺字与混合 fallback；不把 Unicode 数学字母表映射当成完整文本字体替代。
- 序列化仍保留 LaTeX 命令与稳定 `InlineEnvironment` 模型；字体缺失、未知 family 或源树不一致时给出明确错误/诊断，不静默退回错误字体。
- `\mathsf` / `\mathtt` 属于数学字母表样式，本任务不改变其数学语义；不实现字体选择 UI、任意用户字体导入、一般 Font 环境 UI 或公式编辑事务。

## 前置条件与待决策

043 提供稳定的 `MathContent::Environment` / TextFlow 树与源码一致性边界；044 提供公式布局快照、TextFlow shaping 接口、字体缺字诊断；064 提供 `<default>` 的 Math/Code 字体角色及 JetBrains Mono 资源。三项未完成前，本任务保持 planned。实施前通过官方资料和字体数据确认 `\textsf` / `\texttt` 与数学字母表命令的区别，并明确产品期望的字体族与缺字回退规则。

## 实施步骤

1. 盘点 parser、serializer、backend、MathLayout、caret-layout TextFlow shaping、font-role 和资源打包链路；列出当前逐阶段字体、样式、source range、基线和 fallback 数据。
2. 对比 STIX Two Math 的数学字母表、STIX Two Text 资源及 JetBrains Mono；评估按 TextFlow 片段塑形和按 glyph 替换两种方案的视觉语义、缺字覆盖、内存、包体积、缓存与维护成本。
3. 在[数学公式架构](../architecture/editor/math.md)记录唯一权威设计：`\textsf` / `\texttt` 的 font role、字重/斜体组合、CJK 与标点回退、度量单位和失效代际；本 task 只记录实现步骤、验证证据与进度，并链接回该设计。
4. 按分层实现 parser/model/backend 到布局快照的最小接口；字体目录与许可文件、Qt/Rust 资源加载保持一致。拒绝或诊断无法无损表示的请求，避免静默 Roman/系统字体 fallback。
5. 覆盖编辑、序列化往返、字体可用/缺失、Latin/Greek/CJK/标点混排、粗斜体组合、行高基线、缓存失效和跨平台资源加载；记录代表性输入的布局与分配基线。
6. 按验证规范完成 Terra 领域/架构评审、修复、独立 Sol final review，并更新 011 与架构文档中的验收索引。

## 预计改动

现存：`crates/caret-math/src/parser.rs`、`serialize.rs`、`backend.rs`、`layout/`、`crates/caret-layout/`、`resources/fonts/`、Runtime 资源清单及许可文件。需要新增字体时，必须先比较 STIX Two Text 与 JetBrains Mono 的共享/独立职责和包体积，不复制已有字体，也不为短小适配器新增 crate。

## 验收标准

- [ ] `\textsf` / `\texttt` 按已确认的文本 family 语义排版；不与 `\mathsf` / `\mathtt` 数学字母表变体混淆。
- [ ] 字体选择、粗体/斜体组合、标点与 CJK fallback 在 Rust 布局和 Qt glyph 渲染中一致；基线、命中与光标几何使用同一 generation 快照。
- [ ] 内置字体资源与许可随 Rust、Qt 和三平台 Runtime 正确加载；不依赖开发机已安装字体提供基础显示。
- [ ] 缺失字体/字形、未知 family、布局或序列化失败有稳定诊断，文档树、源码、焦点及 generation 保持有效。
- [ ] 测试覆盖公式成功、混排边界、字体组合、缺字/缺字体失败、保存往返、撤销重做及缓存失效；性能结果含可复现的字体、字号、字符集与片段数输入。
- [ ] `\mathsf` / `\mathtt` 的既有数学字母表行为不回归；代码注释、文件结构、资源注册、文档索引和独立评审符合仓库规范。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | caret-math/caret-layout 字体样式行为测试 | 文本 family 与数学字母表语义分离且组合正确 | 未执行 |
| — | Rust 与 Qt 字体资源加载/缺字行为 | 内置资源可加载，fallback 与诊断确定 | 未执行 |
| — | Latin、Greek、CJK、标点混排布局基准 | 基线、行高和热点成本可复现 | 未执行 |
| — | 三平台 Runtime package 检查 | 资源和许可文件完整 | 未执行 |

## 风险与回退

`\textsf` / `\texttt` 是文本 family 语义；以数学 Unicode 字母表替换字符会造成混排、标点与覆盖差异。引入完整字体会增加包体积和字体注册边界。若目标平台或字体库无法可靠提供所选 family，首版应将请求保留并报告缺失，不能静默改变样式；回退方案需在设计决策中明确并可验证。

## 决策与工作记录

- 2026-11-02：从 043/044 中拆出数学 TextFlow 的 `\textsf` / `\texttt` 字体 family 设计与实现；043 继续负责行内格式事务，044 负责一般公式布局/文本塑形快照，本任务决定并接通数学文本 family 语义。

## 完成摘要

未完成。
