# 022 — 文本编辑、选择与 IME

- 状态：planned
- 依赖：021
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-25

## 目标与背景

实现基础输入、删除、光标和选区命令，统一 UTF-8 存储位置、Unicode 字素移动和 IME composition。字素到 glyph cluster 的视觉映射由 024/027 接通。

## 必读

- [文档模型](../architecture/editor/document-model.md)、[命令](../architecture/editor/commands.md)、[文字塑形](../architecture/editor/text-shaping.md)
- [分层](../standards/layering.md)、[FFI](../standards/ffi.md)、[测试](../standards/testing.md)

## 范围与非目标

实现 text.insert、text.delete、caret.move、选区、段落拆分/合并、软换行和 IME 提交/取消。按 020 的标题规则，在块环境标题内 Enter 时将后半段转为正文首段。修改走 Session::run，位置规则遵循 020。

不包含格式 UI、反斜线命令、协作编辑或正式 Undo/Redo；历史由 018 跟进。

## 前置条件与待决策

021 文档模型和 .cdoc 可用。022 不维护第二份正文或另定义位置单位。

## 实施步骤

1. 明确 UTF-8 offset、用户可见字素边界与 IME preedit 的桥接契约；定义 Qt 适配器到编辑命令的输入 DTO。
2. 实现命令和事务失败语义。
3. 验证输入 DTO、命令事务、IME 提交/取消和重复通知；视觉位置映射与真实画布验收留给 027/008。
4. 独立评审并复验。

## 预计改动

caret-engine、C++ 输入/画布适配器、FFI DTO 和编辑命令测试。

## 验收标准

- [ ] 同一 Unicode 版本的扩展字素簇规则作用于 Paragraph 逻辑文本，跨 Run 和 Bold/Italic/Font 节点分段；格式边界不能制造额外光标停靠点。测试含 `Run(U+0065) + Bold(Run(U+0301))` 和跨 Run/环境拆开的 emoji ZWJ/肤色序列；中文、Indic 组合和阿拉伯双向文本不会被截断或拆开。此任务不依赖 shaping 的 glyph cluster 输出。
- [ ] 鼠标与命令选区端点保持在完整字素边界；格式操作把交互选区吸附到完整字素簇，外部非法 Pos 明确拒绝，不静默拆开字素。
- [ ] 输入与编辑不隐式规范化 Unicode 码点序列；搜索/比较的规范化结果不能替代正文。
- [ ] IME 更新不写入文档；提交仅插入一次，取消无副作用。
- [ ] IME composition 未提交时 Return 由输入法优先消费，不误触发标题/正文 Enter；composition 映射到文档时保留原始码点序列。
- [ ] Unicode/IME 用例在 Windows、macOS、Linux 共用同一预期结果；Qt UTF-16 与 Rust UTF-8 的映射不改变逻辑位置。
- [ ] 正文 Paragraph 中 Enter 创建段落，Shift+Enter 是软换行；环境标题按 020 的独立标题规则处理。
- [ ] 块环境标题内 Enter 保留标题前缀，新建正文首段放入后缀并插到现有子项前；首项为表格/嵌套环境时仍保留原顺序，后缀为空时新段为空。
- [ ] 标题开头/末尾 Enter、标题内选区 Enter、跨标题/正文选区和未提交 IME composition 均有明确事务结果；映射或拒绝不得丢失标题/正文内容。
- [ ] 失败命令保持文档、选区和待输入状态不变。
- [ ] 独立评审和定向测试通过。

## 验证计划与结果

| 日期 | 环境 / 检查 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Unicode/IME 命令测试 | 插入、删除、提交和取消符合契约 | 未执行 |
| — | Qt 输入 DTO/命令测试 | IME 通知不重复提交，位置边界符合 020 契约 | 未执行 |
| — | 画布视觉位置映射 | glyph cluster 与命中结果一致，留给 027/008 | 未执行 |

## 风险与回退

字节偏移不能直接当作字符移动步长。所有移动、删除和命中走统一映射；映射不可用时拒绝猜测。

## 决策与工作记录

- 2026-10-25：从原 008 拆出文本输入与 IME。

## 完成摘要

未完成。
