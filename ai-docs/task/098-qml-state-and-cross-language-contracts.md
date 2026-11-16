# 098 — QML 状态机与跨语言契约收敛

- 状态：planned
- 依赖：093、083
- 优先级：P2
- 创建 / 更新：2026-11-17 / 2026-11-17

## 目标与背景

QML 侧存在与领域规则重复或状态机散落的问题：
- `qml/App.qml` 关闭流程由 9 个布尔标志拼成（allowClose、waitForCloseAfterFileOperation、closeAfterSaveRequested/Succeeded/AutoSaveFlush、pendingCloseConfirmation 等），无状态枚举；
- `qml/Canvas/DocumentCanvasPointerHandler.qml` 自行实现文档位置全序比较（kind→paragraphIndex→`compareDecimal` 字符串长度+字典序模拟 `orderUtf8`）与拖选 stop/segment 协议，Rust 只有数学文本停靠点类型；
- `qml/Canvas/CommandSuggestionPopup.qml` 的 42 条数学候选显示名与 Rust `MATH_CANDIDATES` 各一份；
- `qml/App.qml` 文档操作失败对话框直接显示内部错误码（`documentOperationError.text` 绑定 `operationError`），未映射为用户文案。

完成后：关闭/保存流程以单一状态枚举驱动；位置比较与候选标签由 Rust 提供唯一数据；用户可见错误使用可翻译文案。

## 必读

[分层：native](../standards/layering/native.md)（QML 只表达意图）、[QML 规范](../standards/qml.md)、[架构：画布交互](../architecture/interface/canvas-highlighting.md)、任务 083/090/091。

## 范围与非目标

范围内：QML 状态机重构、Rust→QML 契约补充（新增只读快照字段/类型）、错误文案映射。非目标：改交互行为、改布局协议；不把领域逻辑搬进 QML。

## 前置条件与待决策

关闭流程重构依赖 094 的行为测试先落地（否则无回归网）。错误码→文案映射需产品确认目标语言与文案来源（qsTr 上下文）。

## 实施步骤

1. 以 094 的测试为网，把 App.qml 关闭布尔标志改为枚举/状态机并验证各转移。
2. 位置全序比较与拖选 stop 协议：在 Rust 快照提供共享类型/比较结果（或提供 orderUtf8 数值比较函数），QML 只消费。
3. 数学候选标签由 Rust 导出（或生成 QML 常量），删除 QML 副本；补一致性测试。
4. 错误码→用户文案映射表放 C++/Rust 侧，QML 只显示；补 i18n 条目（`.pa` 全 locale 核对）。
5. 全量门禁（含 qmllint/qmlformat/CTest）+ 独立评审。

## 预计改动

`qml/App.qml`、`qml/Canvas/DocumentCanvasPointerHandler.qml`、`qml/Canvas/CommandSuggestionPopup.qml`、`native/viewmodels/*`、`crates/caret-app/src/document_view/snapshot.rs`、`native/i18n/*`。

## 验收标准

- [ ] 关闭流程无散落布尔标志，状态转移有测试。
- [ ] QML 不再自行实现位置全序比较；候选标签无重复来源。
- [ ] 用户可见错误均为翻译文案；所有 locale 的 `.pa` 通过 lupdate 核对。
- [ ] qmllint/qmlformat/CTest 与全量测试通过；独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --workspace --locked`（含 qml 检查） | 全绿 | 未执行 |

## 风险与回退

QML 状态机与窗口关闭/恢复交互易回归；先测后改，按子项拆 commit。i18n 变更需同步所有支持 locale。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已收敛 QML 三个重复外壳（弹窗、对话框宽度、可见页范围），状态机与跨语言契约留本任务。

## 完成摘要

未完成。
