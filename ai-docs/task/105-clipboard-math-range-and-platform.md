# 105 — 剪贴板 Math 形状、错误呈现与跨平台验证

- 状态：planned
- 依赖：091
- 优先级：P2
- 创建 / 更新：2026-11-21 / 2026-11-21

## 目标与背景

[091](091-rich-clipboard-integration.md) 交付了 v1 富剪贴板：Inline/Blocks 片段、text/plain TeX 回退、外部纯文本公式识别、严格片段校验、ID 重映射与单事务粘贴。以下按[剪贴板架构](../architecture/editor/clipboard.md)仍属目标契约但未在 091 v1 实现，集中跟进：

1. **Math 片段形状**：公式内部连续 MathFlow/MathText 局部选区应裁切为有效 `MathTree` 片段（或保留完整 Equation/Align），粘贴到兼容数学宿主；无法构造有效片段时复制失败且不降级。091 v1 对公式内部端点复制返回 `clipboard.host_mismatch`、粘贴到公式内部拒绝，需要补齐。
2. **跨文档主题模板迁移**：片段内 `InlineEnvironment.definitionId` 引用源文档主题模板时，粘贴到不同主题目标文档目前被整文档校验原子拒绝；需要明确迁移/映射策略或引入可见的选择性损失报告，不能静默丢格式。
3. **用户可见剪贴板错误**：desktop 目前只有 `documentOperationMessage` 文案映射，QML 仍 `console.warn`；需要接入现有错误对话框/状态反馈，覆盖复制 TeX 损失、host 不匹配、无效载荷。
4. **真实跨进程/跨平台剪贴板**：CI 只覆盖进程内真实 `QClipboard`（offscreen）；需要验证 X11/Wayland 延迟渲染所有权、Windows/其他平台 MIME 读写，以及真实外部应用互操作（只读取 text/plain 时内容可读）。
5. **Blocks 片段细节**：`ExplicitBreak` 的纯文本投影与页断语义、片段裁切中段落 props/layout/语言等边界字段的取舍需要按实际使用确认；空定界符（`\(\)`）等退化片段是否需要拒绝；含选区外 Opaque 节点的文档复制应做范围剪枝（v1 会整次拒绝）。
6. **外部纯文本识别范围**：v1 只识别“整段 trim 后为单个显示公式”的 `\[…\]`/`$$…$$`；混合在普通文本中的显示公式按原文保留，需要按剪贴板架构补齐或明确写入长期非目标。
7. **性能**：native 粘贴统一走外部文本/片段命令后，纯文本回退与单段 Inline 片段也会全量重排布局（旧 `PasteText` 可增量复用段落布局）；需要在 091 v1 后测量并恢复 plain/Inline 的增量路径，或给出可接受的全量阈值。
8. **测试缺口**：M1 嵌套环境落点（替换成 Equation 时 caret home 补在 body 末尾）、`first_paragraph_hint` 的标题顺序、M2/ExplicitBreak 的 end-clipped caret 用例、失败路径的 selection/history 完整断言可随 Math 形状工作一并补齐；091 已补 seam/redo/单 Undo 三项回归。
9. **代码收敛**：engine 的 `validate_copy_*` 与 caret-doc 片段严格校验重复、`insert_display_math_value` 与 `prepare_display_math_insert` 有块插入重复；错误码在 `InvalidPosition`/`ClipboardHostMismatch` 间分类不一致；`caret-doc` 片段测试可拆到 `fragment/tests.rs`。

## 必读

- [剪贴板](../architecture/editor/clipboard.md)、[数学公式](../architecture/editor/math.md)、[文档格式导入与导出](../architecture/editor/document-interchange.md)
- [083 跨宿主选区与事务](083-cross-host-selection-and-transactions.md)、[091 富剪贴板](091-rich-clipboard-integration.md)、[092 共享数学片段转换](092-checked-math-fragment-converter.md)
- [验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)

## 范围与非目标

- 完成 Math 形状与公式内部选区复制/粘贴；跨文档主题模板策略；QML/状态栏用户可见错误；跨进程/跨平台与实际外部应用验证；必要的行为测试与正确性修复。
- 非目标：重新设计剪贴板 MIME/信封、通用文档交换 AST、Markdown/TeX 完整文档适配器（047/049）；不改变 091 v1 已交付的严格失败策略。

## 前置条件与待决策

- 091 v1 已合入且 `MathFragment` checked API（092）可用。
- Math 片段宿主兼容规则需要与 `MathEditTarget`/结构选区端点（083）统一，避免复制/粘贴两条实现分叉。
- 跨文档主题模板缺失时是“迁移到目标默认主题”“按名字映射”还是“可见拒绝”，需要维护者决策。
- 跨平台验证需要可用的 Linux/Windows 桌面会话或 CI runner；不能执行时如实记录。

## 实施步骤

1. 审计 091 `clipboard_fragment` 中复制/粘贴的数学分支与 083 的数学内部选区投影，确定 Math 形状裁切/插入复用点。
2. 实现 Math 片段复制（含 MathTree/MathText 局部范围、完整 Equation/Align、InlineMath 对象）与兼容宿主粘贴；失败保持 091 的原子性与不降级规则。
3. 建立主题模板缺失/跨主题粘贴的决策与可见诊断；同步剪贴板架构。
4. 接入 QML/状态栏用户可见错误并补 C++/QML 测试。
5. 在可用平台验证真实跨进程剪贴板；补平台说明与自动化边界。
6. 全量门禁、分域评审与未参与实现的 final review，更新 091/105 记录与索引。

## 预计改动

`crates/caret-engine/src/editing/session/clipboard_fragment*`（数学分支）、`crates/caret-doc`（片段主题模板策略/校验入口，如需要）、`crates/caret-app`/`caret-ffi`/native/QML 错误呈现、`tests/cpp/shell/`；不新建 crate，不改变 MIME 版本（除非明确升级并保留 v1 拒绝策略）。

## 验收标准

- [ ] 公式内部 MathTree/MathText 局部选区可复制为有效片段并粘贴到兼容数学宿主；无法构造有效片段时复制失败且不覆盖剪贴板，失败原子性沿用 091。
- [ ] 跨文档主题模板引用有明确策略：可无损迁移或给出可见损失/拒绝，不静默丢格式。
- [ ] 桌面端剪贴板错误经现有用户可见通道呈现，QML 不再只写 console。
- [ ] 跨进程/跨平台剪贴板在至少一个桌面 Linux 与声明支持的平台验证，或明确记录未执行条件与风险。
- [ ] 行为测试覆盖 Math 形状往返、宿主不兼容、Undo/Redo、失败原子性；格式/lint/覆盖率门槛、分域评审与独立最终评审通过。

## 风险与回退

Math 局部裁切与宿主映射容易在跨层端点规范化上产生分叉；主题模板迁移可能改变可见样式。遇到无法无损映射时保持 091 的显式拒绝与原子回滚，不引入降级文本或静默格式丢失。

## 决策与工作记录

- 2026-11-21：从 091 v1 交付范围拆出本后续任务，登记 Math 形状、主题模板、QML 错误呈现与跨平台验证四项；091 v1 已合入 main（`5cf2d64e`），本任务依赖满足，可开始。

## 完成摘要

未开始。
