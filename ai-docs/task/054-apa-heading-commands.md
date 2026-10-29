# 054 — APA 标题层级命令与默认样式

- 状态：planned
- 依赖：010、024、025、026、027
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-10-29

## 目标与背景

在 010 提供的 Section 块环境和 020 的段落大纲语义之上，补齐基础标题命令及其可见默认样式。默认外观以本任务记录的 APA 风格约定为起点，后续允许由文档主题覆盖；本任务不实现主题编辑 UI。

## 必读

- [文档模型](../architecture/editor/document-model.md)、[结构导航](../architecture/editor/structure-navigation.md)
- [命令](../architecture/editor/commands.md)、[文本格式命令](025-text-formatting.md)、[段落排版](026-paragraph-layout.md)
- [测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

包括 `\section`、`\subsection`、`\subsubsection` 的层级语义和默认 APA 样式；标题文本保留用户输入的原始 Unicode，不自动改写为 Title Case。三个命令都创建带标题 Paragraph 的嵌套 Section；Section 树同时承载标题、正文范围和层级编号，不能让子标题成为与 Section 计数树脱节的普通 Paragraph。独立大纲标题仍可由段落属性表达，但不冒充这些结构命令。

不包括 `\chapter` 命令、任意自定义标题层级、主题编辑 UI、编号格式自定义 UI 和文档格式导入导出。Chapter 与 Section 的层级关系尚无统一文档类别/编号规则；需要先单独定稿，再决定是否把 Chapter 纳入章节树，不能在本任务中把它与 `\section` 都映射到同一层级。

## 前置条件与待决策

依赖结构命令、文本格式环境、段落排版与画布布局能力。Section 标题级别由 Section 树的结构层级表达，标题段落仍可有自己的局部格式；普通 Paragraph 的 `outlineLevel` 只表达独立大纲语义，不替代 Section。APA 默认外观是主题中的全局样式，单个标题才可有局部覆盖。标题样式值必须进入主题/环境样式解析层，不得由 QML 或每次命令写入临时直接格式值来模拟。

## 实施步骤

1. 固定三个命令创建 Section 的父级选择规则：光标所在 Section、相邻同级 Section、缺少父级时的结果都要明确定义；层级编号、结构索引和命令候选必须一致。
2. 为各命令接入 Rust 注册表和 Session，并实现 APA 默认对齐、粗体/斜体及层级样式；标题样式通过 Section 默认模板解析。
3. 覆盖命令创建、撤销/重做、标题级别和样式继承的可观察行为。
4. 按评审规范检查跨层职责、缓存影响和性能，再由独立评审检查完整 diff。

## 预计改动

`caret-engine` 命令/会话、`caret-doc` 主题样式及 `caret-layout` / 画布样式解析。具体文件以实现时的职责边界为准。

## 验收标准

- [ ] `\section`、`\subsection`、`\subsubsection` 创建预期嵌套 Section；子标题参与同一章节树的层级编号，不生成脱离计数模型的普通 Heading Paragraph。
- [ ] 命令在缺少父级、Section 标题内、Section 正文内及相邻兄弟之后执行时，遵循明确的父级选择规则。
- [ ] APA 默认样式符合本任务表格，主题覆盖可逐属性工作。
- [ ] 输入文字不因命令自动转写大小写；撤销/重做恢复结构、样式与位置。
- [ ] 命令候选可用状态与实际 Session 分发一致。
- [ ] 行为、失败边界、格式、lint、直接消费者测试和独立评审通过；未执行的人工验收明确记录。

## APA 默认样式约定

| 层级 | 默认外观 |
| --- | --- |
| Section | 居中、粗体、Title Case 作为输入约定 |
| Subsection | 左对齐、粗体、Title Case 作为输入约定 |
| Subsubsection | 左对齐、粗体、斜体、Title Case 作为输入约定 |

“Title Case 作为输入约定”不代表软件自动转换标题文本；转换会改变原文，应由用户输入或后续明确设计的文本命令负责。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 待实现后填写 | 命令、样式与撤销行为正确 | 未执行 |

## 风险与回退

标题默认样式若被写成节点直接格式，会妨碍主题覆盖。实现应通过主题样式解析回退；若现有解析能力不足，先补齐该能力再接命令，不复制一套样式优先级。

## 决策与工作记录

- 2026-10-29：从 010 拆出 `\subsection`、`\subsubsection` 与 APA 默认样式。它们创建嵌套 Section，以免标题树、正文拥有关系和编号层级出现两套互不一致的模型。
- 2026-10-29：暂不纳入 `\chapter`。需要先确定 Chapter 与 Section 的结构和编号关系，不能将其隐式折叠到相同层级。

## 完成摘要

未完成。
