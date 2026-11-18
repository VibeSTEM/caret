# 099 — ShellLoadTest 单类按域拆分

- 状态：done
- 依赖：093
- 优先级：P2
- 创建 / 更新：2026-11-17 / 2026-11-18

## 目标与背景

`tests/cpp/shell/shell_load_test.hpp` 中单个 `ShellLoadTest` 类声明 77 个用例 slot 与 `initTestCase`，实现跨 12 个编译单元、约 5.8k 行；新增测试必须改动共享 header，冲突面大、失败定位差。任务初稿记录的 68 个用例已因后续任务增加测试而过时；当前 `main` 的可执行清单为 77 个用例，静态 Qt 断言宏调用点为 1734，拆分必须以此为新基线完整保留。Brooks sweep 已评估可行方案（未实施）：

- 公共基类 `ShellTestBase : QObject` 只声明 `initTestCase`，派生类继承后每类 `qExec` 都会执行初始化；共享 fixture 放到 `tests/cpp/shell/shell_test_support.hpp/.cpp`；
- 多类单可执行：自定义 `main.cpp` 缓存 `QCoreApplication::arguments()`，用 `QTest::qExec(&obj, const QStringList&)` 逐类调用（直接传 argc/argv 会被 QTest 改写）；单函数 CTest（`Qml.MathTextDrag`、`Qml.PreferencesWithoutRuntime`）必须筛出声明该 slot 的类，未匹配时回落首类保留 Qt 报错；
- 跨用例全局状态：`moduleImportsAndThemeBindingTracksSnapshot` 注入的主题被后续 settings 用例依赖；这些 settings slot 仍在 `ShellLoadTest` 内，需保留类内顺序；聚合 main 的类顺序需 `ShellLoadTest` 先行。

完成后：12 个类/12 个头文件按域拆分，slot 名、77 个用例、1734 个静态 Qt 断言宏调用点及 CTest 20/20 不变。

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
5. 验证：运行仓库格式、lint 与 native 聚合；确认单函数 CTest、全量运行、QtTest 报告格式及参数筛选行为。

## 预计改动

`tests/cpp/shell/`（新 header/source）、`shell_load_test.hpp/.cpp`、`qml/CMakeLists.txt`。无生产代码改动。

## 验收标准

- [x] 单类拆为按域的多类，单 header 不再承载全部 slot。
- [x] slot 名、77 个用例、1734 个静态 Qt 断言宏调用点、CTest 20/20 与总时长在余量内保持不变。
- [x] 单函数 CTest（`Qml.MathTextDrag` 等）仍可单独运行。
- [x] 独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-18 | `cargo test --locked -p caret-tests --test native`（迁移前基线） | 20/20 | 20/20 通过；CTest 总耗时 21.94s，聚合命令 61.56s（含构建/调度） |
| 2026-11-18 | `caret_shell_load_test -functions` 与静态 Qt 断言宏调用点统计（迁移前基线） | 用例及断言清单可对照 | 77 个用例；1734 个断言宏调用点 |
| 2026-11-18 | `./target/native/debug/qml/caret_shell_load_test <slot>` | 抽查通过 | 单函数 `mathTextDragUsesSharedPreviewAndCommitProtocol` 由 CTest 单独运行通过 |
| 2026-11-18 | 聚合入口输出与筛选回归 | 保留 QtTest 参数兼容 | `-functions` 输出 77 项；多类筛选按固定类顺序运行；双 `-o` logger、legacy 格式优先级、XML/lightxml、CSV、TAP 失败行、`/dev/null` 输出目标及跨类 `-datatags` 均通过定向检查 |
| 2026-11-18 | `cargo format`、`cargo lint`、`cargo test --locked -p caret-tests --test native` | 格式、静态检查与 CTest 20/20 通过 | format 完成；lint 九阶段通过；native 20/20 通过；总耗时 21.94s → 22.20s，`Qml.ShellLoad` 12.95s → 13.02s，均在 60s TIMEOUT 内 |

## 风险与回退

跨用例全局状态依赖（主题快照、settings runtime）是主要顺序风险；聚合 main 需固定类顺序并在拆分前后对比全量输出。当前未验证 QtTest `-repeat` 在多个 qExec 类间的套件级重复顺序；该选项会由每类 qExec 分别处理，因此重复分组可能与原单类运行不同，现有 CTest 不使用此选项。测试发现路径为 `target/native/debug/qml/caret_shell_load_test`（非 debug 根）。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已评估并记录完整方案（含 QTest argv/单函数/顺序风险实测），因时间预算停在评估阶段；`Qml.ShellLoad` 时限已按 21 个 fixture 调整为 60s。
- 2026-11-18：实现前复核当前 `main`，任务初稿中 68 个用例与 CTest 11/11 已过时；当前基线是 77 个可执行用例、1734 个静态 Qt 断言宏调用点、CTest 20/20。以当前实际基线迁移，不删除后续补入的测试。
- 2026-11-18：按当前基线拆为 12 个 domain test 类，保留原有 slot 与断言；自定义入口固定类顺序，逐类分派单函数筛选并合并多个 QtTest 输出 logger。
- 2026-11-18：Terra/medium 分领域 reviewer 检出筛选顺序、旧式输出优先级、报告语法和 `-datatags` 边界；已修复并按定向命令复验。共享 fixture 与类拆分 review 未发现确定性代码问题。
- 2026-11-18：Sol/medium 对最终完整 diff 独立只读评审通过；确认 77 个用例、1734 处断言、15 处 `QFAIL`、1 处 `QSKIP` 保持，CTest 注册未变。最终格式、lint 和 native CTest 20/20 均通过。

## 完成摘要

完成 12 类拆分、共享 fixture 抽取与入口参数兼容修复。格式、lint、native CTest 20/20 和独立最终评审均通过。剩余限制：`-repeat` 在多个 qExec 类间按类处理，套件级重复顺序与单类运行不同；现有 CTest 不使用该选项。
