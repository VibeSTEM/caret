# 092 — 共享数学片段转换接口

- 状态：ready
- 依赖：043
- 优先级：P1
- 创建 / 更新：2026-11-16 / 2026-11-16

## 目标与背景

`caret-math` 已有受限 TeX 数学解析器和 MathTree/Align serializer。Markdown 047、TeX 文档 049、公式源码输入及富剪贴板都需要复用同一数学子语言转换边界，但当前序列化接口返回裸字符串，不能报告未知语义或扩展字段无法表达造成的损失。本任务为这一共享领域能力定义可检查的转换 API，不接管任一外层格式，也不创建第二套 AST。

权威边界见[文档格式导入与导出](../architecture/editor/document-interchange.md)和[数学公式](../architecture/editor/math.md)。

## 必读

- [数学公式](../architecture/editor/math.md)
- [文档格式导入与导出](../architecture/editor/document-interchange.md)
- [047 Markdown 适配器](047-markdown-interchange.md)
- [049 TeX 文档适配器](049-tex-interchange.md)
- [验证与评审规范](../standards/validation-and-review.md)

## 范围与非目标

- 以 `MathInputKind` 为显式宿主种类，把受支持 TeX 数学片段解析为 `MathTree` / `Align`，并从规范模型产生规范 TeX 数学片段。
- 解析和序列化共用一套 Rust 领域 API，结构化错误能区分语法错误、超限、类型不匹配及语义/extra 不可表达，并尽可能带 MathTree 路径。
- checked serializer 验证树形和所有已知节点、Run/环境属性及 extra 数据；任何不能无损表达的已知语义或不透明扩展都返回明确损失，不得静默跳过。
- Markdown、完整 TeX 文档、公式输入和剪贴板负责各自的外层语法、分隔符、源位置和诊断；它们只调用共享数学转换 API，不相互依赖。
- 不实现 TeX 文档解析、文档级 IR、剪贴板 MIME/选区、Markdown 扩展、TeX 宏执行或其他外层分隔符扫描。

## 前置条件与待决策

- 以 `caret-math` 现有 parser、MathTree/Align serializer 及验证逻辑为基线，确认公有/内部边界和当前静默丢失字段清单。
- 对每类 MathNode、MathText Run、InlineEnvironment、Align 行/cell、extra 和未知节点记录可序列化、拒绝或可忽略的理由；不得按“当前 serializer 输出了字符串”推断无损。
- 确定结构化诊断中源字节位置与 MathTree 路径的可用边界；位置无法提供时明确报告缺失，不伪造。

## 实施步骤

1. 审计 parser、serializer、模型校验与 extra 字段，对照数学架构支持表列出转换缺口。
2. 定义单一 parse/checked-serialize API 和结构化错误类型，保留并迁移现有调用方；047/049/公式源码输入和剪贴板在各自任务中接入该共享边界。
3. 为 Inline、Equation、Align 添加有效输入、边界、超限和不可表达字段行为测试；覆盖 parse → serialize → parse 的语义往返。
4. 更新数学与文档交换架构、任务依赖，并完成分域审查、修复及独立最终评审。

## 预计改动

`crates/caret-math` parser/serializer API 和测试，必要时数学模型文档及引用该 API 的适配器调用点。不新建 crate，不把外层格式、剪贴板策略或文档 AST 引入数学 crate。

## 验收标准

- [ ] Inline、Equation、Align 均通过同一受类型约束的片段转换 API；错误明确区分语法、限制、宿主类型和无法表达的内容。
- [ ] serializer 对不能安全还原的语义或 extra 返回结构化损失；不会静默删除已知属性、未知字段或数学节点。
- [ ] 支持子集的 parse → serialize → parse 保留规定的 MathTree/Align 语义；错误源位置与路径准确或明确标为不可用。
- [ ] 统一 API 与当前 caret-math parser/serializer 一致，未引入第二实现、第二 AST 或外层格式依赖；047、049、公式源码输入和 091 的实际接入由各自任务验收，不作为本任务的前置验收条件。
- [ ] 失败转换不修改输入模型或编辑会话；格式、lint、覆盖率门槛、行为测试、分域审查及独立最终评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 待实现后填写 | 数学语义往返、错误诊断、格式/lint/覆盖率 | 未执行 |

## 风险与回退

旧 serializer 可能接受了无法完整往返的树。遇到没有安全输出的内容时宁可返回可见损失错误；需要完整外部保真的 Caret 内部复制由 091 的结构 MIME 保留 MathTree，不以 TeX 文本回退冒充无损。

## 决策与工作记录

- 2026-11-16：确认 `caret-math` 是 047/049 共用的数学片段转换器；通用文档适配器和剪贴板只负责各自的外层结构。
- 2026-11-16：从富剪贴板任务拆出 checked serializer/API，避免 047 因剪贴板实现而产生无关依赖。

## 完成摘要

未完成。
