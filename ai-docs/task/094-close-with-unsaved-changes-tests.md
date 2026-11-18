# 094 — 关闭已编辑文档事务的行为测试

- 状态：done
- 依赖：093
- 优先级：P0
- 创建 / 更新：2026-11-17 / 2026-11-18

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

## 实际改动

`tests/cpp/shell/shell_document_close_test.cpp` 加载真实 `App.qml`，为 9 个关闭行为用例注册单独 CTest 过滤入口，使每个入口在独立进程中隔离 document/settings runtime；`Qml.ShellLoad` 聚合也会顺序执行这些 slot。移除 `shell_load_test.cpp` 对 QML 源文本的两项断言。测试驱动干净窗口关闭时发现并修复了 `onClosing` 内递归调用 `close()` 导致窗口保持可见的问题：干净会话直接接受当前 close 事件。为可观察失败提示，给操作错误对话框增加稳定的 `objectName`。

## 验收标准

- [x] 7 条关闭事件路径均有行为测试且断言用户可观察结果与持久化状态（另加独立 Discard 用例）。
- [x] 删除或替换现有源文本断言；关闭测试通过真实 QML 对象和窗口事件验证。
- [x] `cargo test --workspace --locked`（含 CTest）通过；人为破坏恢复写失败状态映射后，对应用例按预期失败。
- [x] 独立分领域检查及修复后最终完整 diff 评审通过，范围与限制记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-18 | macOS arm64 / Qt 6.12；`cargo test --workspace --locked` | workspace Rust/native/QML CTest、集成与 doc tests 全部通过 | 20/20 CTest 通过；workspace 测试通过 |
| 2026-11-18 | 暂将 `App.qml` 的恢复写失败投影改为固定 `false`，运行 `Qml.CloseRecoveryWriteFailure`，随后还原 | 用例能捕获恢复写失败分支回归 | 按预期失败于 `recoveryWriteFailed` 断言；源码已还原，恢复状态映射的用例通过 |
| 2026-11-18 | `cargo format --check` | 仓库格式检查通过 | 通过 |
| 2026-11-18 | `cargo lint --check` | Clippy、依赖、CMake、分层、qmllint、clang-tidy、include-cleaner、cppcheck 通过 | 修复全部 review findings 后 9/9 阶段通过 |
| 2026-11-18 | `Qml.CloseSaveEdited`、`Qml.CloseAutoSaveFlush` | 首次隐藏事件确认保存已完成且磁盘含新正文 | 生命周期与首次事件标记修复后 2/2 通过 |

## 风险与回退

关闭流程涉及 AutoSave 定时器与窗口生命周期；用例使用 Qt 信号、可观察状态和条件等待，不使用固定睡眠。测试确认在 macOS arm64 / Qt 6.12 上稳定；其他平台窗口后端仍依赖 CI 覆盖。生产代码只有干净会话关闭事件的接受方式变更，没有增加 test-only API。

## 决策与工作记录

- 2026-11-17：Brooks sweep 将本项列为 Critical 残余并建任务。
- 2026-11-18：真实 App 窗口行为用例覆盖直接关闭、取消、丢弃、手动保存、AutoSave flush、AutoSave 失败后重新确认、进行中保存失败、保存冲突及恢复写失败；冲突测试核验原文件和恢复 envelope 的本地修订 payload。分领域检查提出的保存对话框时序、in-flight 失败、磁盘内容及 generation/信号断言已补充。
- 2026-11-18：Terra/medium 领域检查首轮发现并推动修复对话框时序、进行中失败覆盖和持久化断言缺口；复核最终差异后无阻断项。Sol/medium 复核指出宿主 Documents 副作用与关闭顺序断言问题，已改为临时文件路径并增加隐藏事件边界断言；又发现回调捕获变量生命周期风险，已用局部 QObject 上下文确保先断开，并用独立标记锁定首次 hide 结果。失败提示新增稳定对象名以供行为断言。最终完整 diff review 通过，无剩余发现。按钮由 Qt 元对象触发信号，未覆盖真实指针命中、平台原生文件对话框输入和其他平台窗口后端。

## 完成摘要

task-094 完成。真实窗口关闭行为测试、失败分支注入验证、完整 workspace 测试、格式、lint、分领域检查与最终独立评审均已通过；保留的限制是未验证真实指针命中、原生文件对话框输入和其他平台窗口后端。
