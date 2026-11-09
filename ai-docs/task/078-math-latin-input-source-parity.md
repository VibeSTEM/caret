# 078 — 数学输入源的拉丁字符等效输出

- 状态：ready
- 依赖：076
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-09

## 目标与背景

在 MathTree 数学编辑中，用户可能保留中文等输入源，但希望按键产生与对应拉丁输入源相同的字符。macOS 实测已确认字母输入保持 Latin，但反斜线键在中文输入源下仍可能产生中文顿号 `、`。这是输入源的标点映射差异，不能通过将物理键位硬映射为美式键盘字符来解决，否则会破坏 UK、德语等布局。

当前 MathTree/Source 的输入法上下文是 `enabled=false`、`preferLatin=false`，C++ 画布因此不报告 Latin hint。`ImhPreferLatin` 目前用于允许组合输入的 MathText 状态。Qt 6.12 另定义 `Qt::ImhLatinOnly`，语义为仅允许拉丁类输入；但 hints 是发给平台输入法的提示，平台实现可以忽略。任务首先要确认禁用 IME 的 MathTree 是否能收到并遵循该 hint，不能单凭 hint 查询证明目标行为。参见 [Qt 6.12 `Qt::InputMethodHint`](https://doc.qt.io/qt-6.12/qt.html#InputMethodHint-enum) 和 [QWidget 输入法提示说明](https://doc.qt.io/qt-6/qwidget.html#inputMethodHints-prop)。

本任务定义并实现数学结构输入的 Latin 输出策略：字符应匹配同一键盘布局下相应拉丁输入源的结果，而不是固定映射到 US 键盘；`\text` 等普通文本区域保留其输入法组合输入行为。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)、[分层规范](../standards/layering.md)、[076](076-per-key-state-arbitration.md)
- [快捷键管理架构](../architecture/interface/keyboard-shortcut-architecture.md)及相关 IME 状态文档
- Qt 6.12 [`Qt::InputMethodHint`](https://doc.qt.io/qt-6.12/qt.html#InputMethodHint-enum)，查阅日期：2026-11-09

## 范围与非目标

- 评估 MathTree 状态下使用 `ImhLatinOnly`（可与 `ImhPreferLatin` 组合）对字母、数字、标点和反斜线的实际影响；明确 Qt hint 的平台遵循差异。
- 先确认 IME disabled 时 hint 是否可达并生效；若 hints 无法满足标点等效输出，设计平台适配，使键值来自用户对应的拉丁键盘布局；不得将物理扫描码写死映射到 US 布局，也不得仅依据最终 Unicode 值把中文标点替换为反斜线。
- 在 macOS、Windows、Linux 验证中文输入源与相应拉丁输入源下字母、数字、常见标点及 `\` 的对应输出；MathTree Unicode 符号通过粘贴或数学命令等显式路径验证，并检查字体缺字恢复。
- 保持 `\text` 普通文本区域的输入法组合输入，不改变其字符策略。
- 不在本任务中改变输入源切换、系统输入法偏好或全局键盘布局。

## 前置条件与待决策

- 明确各平台“对应拉丁输入源”采用同一键盘布局的比对方式；至少包含 US 与一个非 US 布局，避免把布局差异误判为 IME 差异。
- 确认 MathTree 的 IME disabled 契约能否与 Latin-only hint 共存；不得未经验证打开组合输入，因为 MathTree 不接受 IME composition。
- 优先验证 Qt `ImhLatinOnly`，只有实测不能满足契约时才引入平台专用路径，并说明 API、维护成本与回退行为。
- 如系统不能可靠识别用户对应的拉丁布局，须提供清晰的限制或可配置布局来源；不可假定英文必然等于 US QWERTY。

## 实施步骤

1. 检查 MathTree 的 `ImHints`、按键事件和输入法 commit 路径，区分输入源转换、键盘布局转换与 Qt 文本事件。
2. 验证 IME disabled 时的 hint 查询与平台实际行为；为可行的 Latin-only 路径和标点输出增加可观察测试，覆盖 `\`、字母、修饰键和 `\text` composition。
3. 在目标平台手测中文输入源与同布局拉丁输入源；若通用 hint 不足，再实现范围最小的平台适配并增加平台回归。
4. 更新键盘/输入法权威架构文档，记录保证范围、例外和平台矩阵。
5. 按验证标准完成分域审查与独立最终审查，并将实测结果写入本文。

## 预计改动

`native/canvas/document_canvas/input.cpp`、其输入法上下文状态源、对应 Rust/native 测试及相关键盘/输入法架构文档。平台专用实现范围由前置调查决定；不预先引入 US 键位表。

## 验收标准

- [ ] MathTree 在中文输入源下的字母、数字、标点和 `\` 输出，与同键盘布局的拉丁输入源逐项一致；记录平台、输入源和布局。
- [ ] 至少在 macOS、Windows、Linux 各完成一组实机验证；Qt hint 单元测试不替代实机验证。
- [ ] US 与至少一个非 US 布局均未被错误映射为 US 键位。
- [ ] MathTree 的 Unicode 字符通过粘贴或数学命令等显式路径输入，缺字显示与恢复有验证证据；`\text` 仍支持 IME composition。
- [ ] 不切换系统输入源；不按收到的 Unicode 标点值进行硬替换。平台若需按当前键盘布局映射，必须基于布局/输入源语义并记录限制。
- [ ] 失效实现、重复映射和过期说明已清理；独立评审完成并复验。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Qt/native 自动测试 | 验证 hints、事件分派及 `\text` 输入法状态 | 未执行 |
| — | macOS、Windows、Linux 手工验证 | 比对各自相同键盘布局的 Latin 与非 Latin 输入源输出 | 未执行 |

## 风险与回退

输入法 hint 的实际语义受操作系统和输入法实现影响；MathTree 当前禁用 IME，hint 是否能到达平台尚待确认。平台适配若不能在不假定 US 布局的前提下可靠获取对应拉丁字符，应明确报告无法完全保证的场景，并保留可解释的系统输入行为；不得默默吞掉输入或仅根据收到的 Unicode 值硬替换字符。

## 决策与工作记录

- 2026-11-09：从 076 拆出跨平台验收与数学标点 Latin 等效输出。用户确认中文输入源下数学内容应输出相应拉丁输入源字符；此前 macOS 字母输入通过，但 `\` 仍输出 `、`。
- 2026-11-09：初步核对 Qt 6.12 API：`ImhPreferLatin` 是偏好，`ImhLatinOnly` 表示仅允许拉丁类输入；实际输入法可以忽略提示。禁止直接硬编码 US 物理键映射，先验证通用 hint 和系统同布局 Latin 源行为。

## 完成摘要

未完成。
