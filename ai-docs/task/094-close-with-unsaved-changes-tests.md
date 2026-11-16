# 094 — 关闭已编辑文档事务的行为测试

- 状态：ready
- 依赖：093
- 优先级：P0
- 创建 / 更新：2026-11-17 / 2026-11-17

## 目标与背景

`qml/App.qml` 的关闭流程约 601 行：`onClosing` 触发保存/AutoSave flush/保存冲突/恢复写失败/关闭确认重开，但现有测试只对 `App.qml` 源文本做字符串断言（[tests/cpp/shell/shell_load_test.cpp](../../tests/cpp/shell/shell_load_test.cpp) 的 `unsavedChangesDialogUsesStyledActionsInRequestedOrder`、`closeFailureDoesNotReopenPendingConfirmation`）。这是当前唯一被评估为 Critical 的数据丢失暴露点：关闭已编辑文档时若 flush/冲突/失败分支回归，用户可能在未提示的情况下丢失内容。

完成后：存在真正的行为测试，模拟窗口关闭与各失败分支，验证最终会话/持久化状态和用户确认结果，而不是源文本。

## 必读

[测试规范](../standards/testing.md)（Qt Test/QSignalSpy、事件等待）、[验证与评审](../standards/validation-and-review.md)、[QML 断言风格](../standards/qml.md)、任务 019（AutoSave）。

## 范围与非目标

范围内：tests/cpp/shell 新增关闭流程测试 slot 与必要的 fixture；QML 关闭状态机若不可测，加最小可测 seam（不改用户可见行为）。非目标：重写 App.qml 关闭状态机（登记在 098）。

## 前置条件与待决策

需要能驱动窗口关闭的测试入口（`QWindow::close()`/`onClosing` 事件）与可注入的保存失败/冲突条件；确认现有 `LayoutViewModel` 是否有测试专用注入点，没有则补最小 fixture 而非生产 test-only 方法。

## 实施步骤

1. 枚举关闭状态机事件：未修改直接关、保存成功、AutoSave flush、保存冲突、恢复写失败、关闭失败后重开确认、取消关闭。
2. 为每条写行为测试，断言最终文档 generation、保存标记、恢复文件与信号序列。
3. 对不可直接注入的失败路径，优先复用它处已有的配置入口，避免生产层 test-only API。
4. 独立 subagent 只读评审并复验。

## 预计改动

`tests/cpp/shell/shell_load_test.hpp` + 新/现有测试 `.cpp`；可能新增 fixture 头。不改生产代码；如必须加 seam，限定在现有公开属性/方法。

## 验收标准

- [ ] 7 条关闭事件路径均有行为测试且断言用户可观察结果与持久化状态。
- [ ] 删除或替换现有源文本断言；测试不再依赖 App.qml 字面量。
- [ ] `cargo test --workspace --locked`（含 CTest）通过，且新用例在失败分支上确实失败（人为破坏验证一次）。
- [ ] 独立评审通过，范围与限制记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --locked -p caret-tests --test native` | 11/11 通过 | 未执行 |
| — | 人为破坏一个失败分支 | 对应用例失败 | 未执行 |

## 风险与回退

关闭流程涉及 AutoSave 定时器与窗口生命周期，测试可能不稳定；使用 `QSignalSpy`/条件等待而非固定 sleep。若必须改生产 seam，单独评审并记录理由。

## 决策与工作记录

- 2026-11-17：Brooks sweep 将本项列为 Critical 残余并建任务。

## 完成摘要

未完成。
