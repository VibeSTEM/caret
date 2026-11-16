# 099 — ShellLoadTest 单类按域拆分

- 状态：planned
- 依赖：093
- 优先级：P2
- 创建 / 更新：2026-11-17 / 2026-11-17

## 目标与背景

`tests/cpp/shell/shell_load_test.hpp` 中单个 `ShellLoadTest` 类声明 **69 个 slot**（68 个用例 + initTestCase），实现跨 12 个编译单元、约 5.8k 行；新增测试必须改动共享 header，冲突面大、失败定位差。Brooks sweep 已评估可行方案（未实施）：

- 公共基类 `ShellTestBase : QObject` 只声明 `initTestCase`，派生类继承后每类 `qExec` 都会执行初始化；共享 fixture 放到 `tests/cpp/shell/shell_test_support.hpp/.cpp`；
- 多类单可执行：自定义 `main.cpp` 缓存 `QCoreApplication::arguments()`，用 `QTest::qExec(&obj, const QStringList&)` 逐类调用（直接传 argc/argv 会被 QTest 改写）；单函数 CTest（`Qml.MathTextDrag`、`Qml.PreferencesWithoutRuntime`）必须筛出声明该 slot 的类，未匹配时回落首类保留 Qt 报错；
- 跨用例全局状态：`moduleImportsAndThemeBindingTracksSnapshot` 注入的主题被后续 settings 用例依赖，`ShellSettingsTest` 内部顺序需保持；聚合 main 的类顺序需 `ShellLoadTest` 先行。

完成后：12 个类/12 个头文件按域拆分，slot 名、68 个用例、断言与 CTest 11/11 不变。

## 必读

[测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)、[仓库文件](../standards/repository-hygiene.md)。

## 范围与非目标

范围内：`tests/cpp/shell/` 内测试类拆分、`main.cpp`、`qml/CMakeLists.txt` 显式源文件登记。非目标：改测试用例内容/断言；不改生产代码与 QML。

## 前置条件与待决策

当前 `Qml.ShellLoad` TIMEOUT 已由 45s 调整为 60s（sweep 提交 8fc8443e）；拆分后确认 CTest 名称与超时。注意事项见 `tests/src/test_scope/tests.rs`（`tests/cpp/shell/` 前缀归类已覆盖新增文件，无需改选择器）。

## 实施步骤

1. 新建 `shell_test_support.hpp/.cpp`（`snapshot_from_theme_facade`、`ShellTestBase`、APP 下 recovery/runtime fixture）。
2. 按现有 `.cpp` 自然边界建 12 个类与头文件；机械改 `ShellLoadTest::` → `<Class>::`，换 include；`Q_IMPORT_QML_PLUGIN` 只留在一个 TU。
3. 新建 `main.cpp`：缓存 argv、按 `-functions/-o` 等选项选类、逐类 `qExec`、退出码按位或合并。
4. `qml/CMakeLists.txt` 在 `add_executable(caret_shell_load_test ...)` 内追加新源文件（显式列表）。
5. 验证：`cargo fmt --all`、`cargo test --locked -p caret-tests --test native`（11/11）、增量构建 0 新增告警、抽查单函数 CTest 与全量运行；clang-format 检查新增文件。

## 预计改动

`tests/cpp/shell/`（新 header/source）、`shell_load_test.hpp/.cpp`、`qml/CMakeLists.txt`。无生产代码改动。

## 验收标准

- [ ] 单类拆为按域的多类，单 header 不再承载全部 slot。
- [ ] slot 名、68 个用例、断言数、CTest 11/11 与总时长在余量内保持不变。
- [ ] 单函数 CTest（`Qml.MathTextDrag` 等）仍可单独运行。
- [ ] 独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --locked -p caret-tests --test native` | 11/11 | 未执行 |
| — | `./target/native/debug/qml/caret_shell_load_test <slot>` | 抽查通过 | 未执行 |

## 风险与回退

跨用例全局状态依赖（主题快照、settings runtime）是主要顺序风险；聚合 main 需固定类顺序并在拆分前后对比全量输出。测试发现路径为 `target/native/debug/qml/caret_shell_load_test`（非 debug 根）。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已评估并记录完整方案（含 QTest argv/单函数/顺序风险实测），因时间预算停在评估阶段；`Qml.ShellLoad` 时限已按 21 个 fixture 调整为 60s。

## 完成摘要

未完成。
