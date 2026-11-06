# 065 — TeX 源码输入与结构化公式转换

- 状态：planned
- 依赖：043、044、037
- 优先级：P2
- 创建 / 更新：2026-11-04 / 2026-11-04

## 目标与背景

支持用户直接输入 TeX 源码，并将其转换为 Caret 结构化 `MathTree`。当前 `MathSourceDraft`、FFI 状态投影、Qt AutoSave deadline 和关闭提示已经存在；正文/块级公式插入命令 `insert.inline_math`、`insert.equation`、`insert.align` 仍是 `available: false` 的占位候选，`InsertInlineMath(source)` 目前由 engine 测试直接调用。Math TextFlow 中的 `math.split_inline`（别名 `\math`）是独立的树拆分命令。044 负责公式插入命令接入和可交互布局；本 task 负责 TeX 源码输入、解析转换和草稿生命周期，不替代 044 的插入命令。

TeX 输入是明确的公式构造方式，解析成功后只提交结构化 MathTree，不在 `.cdoc` 中保留第二份源码。当前解析尚不完整时可保留草稿继续编辑；其他语法错误会拒绝该次提交并保留已提交树，是否也保留错误候选文本由本任务决策。语法有效但节点 ID、Run 属性或未知字段无法安全归属的候选同样不得提交，需定义保留与修正路径。转换完成后，TextFlow 与结构化公式命令按树事务即时更新 MathTree，不经 TeX 往返。源码草稿只表示无法提交为有效树的 TeX 输入，不等同于一般数学焦点，也不阻止结构化编辑进入 AutoSave。

## 必读

- [数学公式架构](../architecture/editor/math.md)
- [公式模型与编辑](043-math-model-and-editing.md)
- [公式布局与画布](044-math-layout-and-rendering.md)
- [AutoSave 实现](037-autosave-implementation.md)
- [文档生命周期与历史](018-document-lifecycle-history.md)
- [AutoSave 策略设计](019-autosave-design.md)
- [验证与评审规范](../standards/validation-and-review.md)

## 范围与非目标

- 设计 TeX 源码输入和转换入口；明确它与 044 的公式插入命令、数学命令查询及 TextFlow 编辑之间的关系。
- 评估 `MathSourceDraft`、MathEdit 与 app document lifecycle 的接口和文件边界，确认哪些状态归 engine、app、FFI、Qt 和 QML 所有。
- 定义有效源码提交、不完整源码、解析错误、取消、失焦、切换目标、撤销/重做、手动保存、AutoSave、recovery、异步打开及窗口关闭时的状态迁移。
- 明确草稿的 dirty/revision 身份、持久化边界、过期目标处理和 timer generation 规则；无效源码不得伪装成已保存 `.cdoc` 状态。
- 用设计结论更新数学架构权威文档，再按设计拆出实施范围和黑盒测试。

本 task 开始前不新增用户可见的源码编辑入口。它不改变结构化 MathTree、Math TextFlow 即时编辑或 `.cdoc` 单一权威模型，也不把数学排版和 IME 几何实现从 044 移入 UI 层。

## 前置条件与待决策

- 043 完成数学模型、树事务与源码映射基础；044 提供公式画布中的可交互位置与焦点路径；037 完成 app AutoSave 生命周期实现。
- 决定源码入口是显式模式/面板、命令行上下文，还是只作为内部导入/恢复能力；不得默认把 TeX 源码键入当作普通数学输入。
- 决定源码草稿是否只存在于当前会话，还是另有独立恢复载体；`.cdoc` 仍只保存有效结构化树。
- 决定保存与关闭遇到不完整草稿时采用何种明确用户操作，以及 AutoSave 与 recovery 的差异。

## 实施步骤

1. 盘点现有 `MathSourceDraft`、MathEdit、FFI、Qt timer 和关闭对话框的 API、所有权及实际调用入口。
2. 绘制源码编辑状态机和 revision/generation 生命周期，明确结构化公式输入不进入草稿暂停态。
3. 比较现有模块扩展与独立 source editor 模块的依赖、API、测试和维护成本，确定单一权威状态位置。
4. 更新数学架构文档并记录决策；设计评审后再拆分实现任务与验证用例。

## 预计改动

- 设计阶段：`ai-docs/architecture/editor/math.md` 与本 task。
- 后续实现目录由设计评审确定；候选范围包括 `crates/caret-engine/src/editing/session/math_edit/`、`crates/caret-app/src/document_lifecycle.rs`、`crates/caret-ffi/src/document/`、`native/viewmodels/document/`、`qml/` 和 Qt Quick 测试。

## 验收标准

- [ ] 文档说明 TeX 源码输入和转换的入口、提交/取消与错误恢复行为；`\math`、`\equation`、`\align` 命令接入归 044。
- [ ] 结构化公式输入、Math TextFlow 编辑和源码草稿三种状态的边界明确；普通公式焦点不触发 AutoSave 暂停。
- [ ] 独立接口/文件边界经依赖方向、状态所有权、跨层调用数和测试成本评估后决定，并记录取舍。
- [ ] 保存、AutoSave、recovery、异步打开、失焦、取消、关闭及过期 revision 的状态迁移有状态图与失败行为。
- [ ] `.cdoc` 只接受有效 MathTree；不完整草稿不会被误报保存成功或覆盖新 revision。
- [ ] 设计通过独立 Terra/medium 评审后，实施和黑盒验证范围可单独排期。

## 验证计划与结果

设计阶段检查架构文档、调用链和状态图；实施由后续 task 覆盖 Rust 状态迁移、Qt timer 与真实窗口交互，不以当前 engine 单元测试代替用户路径验证。

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 待设计 | 记录源码入口、状态迁移和跨层验证方案 | 未执行 |

## 风险与回退

若产品决定不提供源码编辑入口，应删除不可达的源码草稿 UI 生命周期与跨层接口，并保留结构化公式编辑路径；不得让隐藏源码草稿能力永久增加 AutoSave 状态复杂度。

## 决策与工作记录

- 2026-11-04：从 066 重新编号为 065。数学文本字体环境不再独立排期：数学文本岛复用正文 TextFlow 与 Font 环境，字体角色由 064 管理，公式 shaping 由 044 验收。TeX 源码输入转 MathTree 单独设计；044 负责 `\math`、`\equation`、`\align` 命令接入，065 负责其源码输入/转换与草稿生命周期。

## 完成摘要

未完成。完成时填写设计决策、依赖评审和后续实施任务链接。
