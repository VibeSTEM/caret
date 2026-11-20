# 091 — 富剪贴板与数学片段接入

- 状态：ready
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

- [ ] Caret MIME 往返保留选区中的 MathTree/Align、InlineMath、文本与格式；Inline 片段仅在 Paragraph 位置接受，Math 片段只在兼容数学宿主接受，Block 序列遵循宿主边界；粘贴后节点 ID 与目标文档无冲突。
- [ ] 纯文本回退无 U+FFFC 泄漏，公式以可读规范 TeX 输出；外部应用只读取 text/plain 仍能看到内容。共享 checked serializer 对不可表达语义或扩展字段报告损失，复制显示错误且不覆盖系统剪贴板。
- [ ] 外部纯文本中的受支持闭合公式片段进入对应 InlineMath/Equation/Align；裸 `$`、未闭合、非法或超限输入保持原文或安全报错，不破坏选区与文档。
- [ ] 粘贴为一次可撤销事务；Redo 重现相同结构与结果，失败不改变文档、焦点、选区、generation 或历史。
- [ ] 损坏/未知 Caret 载荷不静默降格到纯文本；未知字段和不支持节点按明确策略拒绝并保留源载荷。
- [ ] Rust 与 Qt 边界职责符合分层；格式化、lint、覆盖率门槛、直接消费者测试、分域审查及独立最终评审通过，未执行的窗口/平台检查如实记录。
- [ ] 047、049、数学架构和命令/Block 交互文档的共享边界一致，没有第二数学 AST 或适配器间调用。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 待实现后填写 | MIME 往返、数学导入/导出、失败原子性、格式/lint/覆盖率 | 未执行 |

## 风险与回退

片段裁切、节点身份重映射或新旧版本解释不完整会导致结构损失或目标文档 ID 冲突。实现遇到不能完整验证的节点时在写文档前拒绝；纯文本回退供非 Caret 应用读取，不能用于掩盖 Caret 专用载荷的失败。

## 决策与工作记录

- 2026-11-16：经维护者确认，MathTree 片段应通过富剪贴板在 Caret 间保留结构，并提供外部可读 TeX；转换边界由 047/049 共用。
- 2026-11-16：盘点发现 047/049 原设计只定义完整文档适配器，未定义共用数学片段入口；架构已明确复用 `caret-math`，本 task 承接剪贴板结构与接线实现。

## 完成摘要

未完成。依赖 083（done）与 092（done，`MathFragment` checked 片段转换 API 已合入）均已满足，任务可开始；本任务负责 Caret 结构载荷/text 回退、选区裁切、ID 重映射与一次性粘贴事务，并复用 092 的数学片段边界。
