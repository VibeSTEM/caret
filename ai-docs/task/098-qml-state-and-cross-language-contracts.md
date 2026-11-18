# 098 — QML 状态机与跨语言契约收敛

- 状态：done
- 依赖：093、083
- 优先级：P2
- 创建 / 更新：2026-11-17 / 2026-11-17

## 目标与背景

QML 侧存在与领域规则重复或状态机散落的问题：
- `qml/App.qml` 关闭流程由 9 个布尔标志拼成（allowClose、waitForCloseAfterFileOperation、closeAfterSaveRequested/Succeeded/AutoSaveFlush、pendingCloseConfirmation 等），无状态枚举；
- `qml/Canvas/DocumentCanvasPointerHandler.qml` 自行实现文档位置全序比较（kind→paragraphIndex→`compareDecimal` 字符串长度+字典序模拟 `orderUtf8`）；
- `qml/Canvas/CommandSuggestionPopup.qml` 的 42 条数学候选显示名与 Rust `MATH_CANDIDATES` 各一份；
- `qml/App.qml` 文档操作失败对话框直接显示内部错误码（`documentOperationError.text` 绑定 `operationError`），未映射为用户文案。

完成后：关闭/保存流程以单一状态枚举驱动；位置比较与候选标签由 Rust 提供唯一数据；用户可见错误使用可翻译文案。

## 必读

[分层：native](../standards/layering/native.md)（QML 只表达意图）、[QML 规范](../standards/qml.md)、[架构：画布交互](../architecture/interface/canvas-highlighting.md)、任务 083/090/091。

## 范围与非目标

范围内：QML 状态机重构、Rust→QML 契约补充（新增只读快照字段/类型）、错误文案映射。非目标：改交互行为、改布局协议；不把领域逻辑搬进 QML。

## 前置条件与待决策

关闭流程重构依赖 094 的行为测试先落地（否则无回归网）。错误文案以英文源文案进入现有 `App` context，并同步当前支持的 `en`、`ja`、`zh-Hans`、`zh-Hant` 四个 locale。

## 实施步骤

1. 以 094 的测试为网，把 App.qml 关闭布尔标志改为枚举/状态机并验证各转移。
2. 移除 QML 对正文位置的全序比较。数学文本岛的 Rust 快照提供同域同快照的数值 `orderOrdinal`，QML 只用它筛选局部拖选预览区间；该序号不跨宿主或正文定义全序，松手提交仍由 Rust 处理。
3. 数学候选标签由 Rust 导出；QML 不再按 alias/commandId 映射标签。供 `lupdate` 提取的候选源由 Rust registry 生成，并以测试校验同步。
4. 错误码→用户文案映射表放 C++/Rust 侧，QML 只显示；补 i18n 条目（`.pa` 全 locale 核对）。
5. 全量门禁（含 qmllint/qmlformat/CTest）+ 独立评审。

## 预计改动

`qml/App.qml`、`qml/Canvas/DocumentCanvasPointerHandler.qml`、`qml/Canvas/CommandSuggestionPopup.qml`、`native/viewmodels/*`、`crates/caret-app/src/document_view/snapshot.rs`、`native/i18n/*`。

## 验收标准

- [x] 关闭流程无散落布尔标志，状态转移有测试。
- [x] QML 不再自行实现正文位置全序比较；数学文本岛局部预览消费 Rust 提供的同域序号；候选标签无重复映射来源。
- [x] 用户可见错误均为翻译文案；所有 locale 的 `.pa` 通过 lupdate 核对。
- [x] qmllint/qmlformat/CTest 与全量测试通过；独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-18 | `cargo quality` | Rust→QML 标签、数学偏移协议和全仓门禁通过 | 全部通过；Rust workspace 测试通过，877 个 caret-engine 测试通过，CTest 20/20 通过 |
| 2026-11-18 | 锁定 Qt `lupdate qml native -recursive` 与四份 `.pa` context/source 对照 | 翻译源与每个 locale 完整同步 | 172 条 context/source 全部匹配；四语无缺项或多余项，所有条目 `finished` |
| 2026-11-18 | Terra/medium 领域复审及 Sol/medium 最终 diff 复审 | 关闭状态、数学指针、候选标签和错误翻译路径符合契约 | Terra 对关闭流程、数学指针、标签/i18n 分项复核通过；Sol 复审的 CTest slot 名、AutoSave 失败及普通保存冲突路径均已修复，最终完整 diff 复审通过 |

## 风险与回退

QML 状态机与窗口关闭/恢复交互易回归；先测后改，按子项拆 commit。i18n 变更需同步所有支持 locale。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已收敛 QML 三个重复外壳（弹窗、对话框宽度、可见页范围），状态机与跨语言契约留本任务。
- 2026-11-18：数学拖选序号限定为同一 `domainId` 与布局快照内的局部 UTF-8 顺序，不把它用作跨宿主文档全序；正文拖选和数学岛提交协议保持原样。
- 2026-11-18：候选标签仍以 Rust registry 为唯一运行时来源；由 registry 生成的 QML 静态翻译源仅供 lupdate，并由 Rust 单元测试检查同步。
- 2026-11-18：关闭保存等待期间不把无关文件操作失败当作关闭保存结果；AutoSave 刷新失败继续回到关闭确认，显式关闭保存冲突进入冲突处理。
- 2026-11-18：新增信号级交错回归用例隔离覆盖关闭状态关联；端到端关闭、AutoSave 成功/失败及冲突路径由既有 Qt 集成用例覆盖。

## 完成摘要

QML 关闭状态改为单一枚举；数学文本岛预览使用 Rust 提供的同域 UTF-8 ordinal；候选标签源从 Rust registry 生成并供 lupdate 扫描；文档操作错误映射为四语翻译文案。完整质量门禁及独立最终复审通过。
