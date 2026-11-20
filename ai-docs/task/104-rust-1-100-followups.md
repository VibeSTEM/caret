# 104 — Rust 1.100 与质量清理的后续项

- 状态：implemented
- 依赖：103
- 优先级：P2
- 创建 / 更新：2026-11-20 / 2026-11-21

## 目标与背景

[103](103-rust-1-100-upgrade.md) 合并后遗留若干因工具链约束、产品阶段或独立 issue 范围而未在本次执行的事项。本任务统一跟进，避免散落在 issue 与任务记录中：

1. **Miri 固定 nightly 升级与 `str::trim_prefix` 回填**：`tests/src/miri.rs` 原固定 `nightly-2026-09-15`，该 nightly 尚未稳定 `trim_prefix_suffix`；1.100 的 `str::trim_prefix` 不能用于 Miri 覆盖的 crate，103 回退了 5 处替换。
2. **依赖冷却期评估**：`.cargo/config.toml` 的 `[registry] global-min-publish-age` 在内测阶段不需要（103 未采用）。结束内测、面向外部发布前重新评估是否启用。
3. **#149 snapshot 必填字段**：`caret-app` 的 snapshot 输出 `mode`/`prefix`/`cursorUtf16`，但 C++ 解码仍容许缺省并保留「旧快照省略 prefix 字段时使用的兼容缺省值」注释。按「不做兼容」改为必填并删除兼容注释。
4. **#145 数学符号旧常量**：`caret-math/src/symbols.rs` 的旧 accent 常量只被测试使用；`caret-doc/src/model/math.rs` 的「旧或未验证节点…兼容默认值」注释与 `caret-math/src/serialize.rs` 的空样式兜底需要删除或改写为当前 schema 的正式语义。
5. **Qt TSan 排除收敛**：Linux TSan 排除根因是未插桩 libQt6Core 的 QQmlThread 分配冲突。Qt 提供 TSan 插桩或上游给出结论后复查并移除排除；此前把同类 QML 测试统一打 label、按 label 排除，避免逐项补表。

## 必读

- [103](103-rust-1-100-upgrade.md)、issue #152、#149、#145
- [验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)、[依赖获取](../standards/dependency-acquisition.md)
- [文档存储](../architecture/editor/document-model/document-storage.md)、[数学公式](../architecture/editor/math.md)

## 完成情况

1. **Miri nightly 升级与 `trim_prefix` 回填（已完成）**：`MIRI_NIGHTLY` 升到 `nightly-2026-09-22`（1.100 开发周期内、已稳定 `str::trim_prefix`；rustc 1.100.0-nightly `1303417c4`），注释说明选型依据。重新采用 5 处 `trim_prefix`（`caret-math` 的 parser×2、dimension、layout/frame，以及 `launcher/src/ffi_names.rs`）。`cargo ub-check` 在新 nightly 上通过；升级记录由本任务与 `tests/src/miri.rs` 注释共同指向（未改动 001/006 的历史记录）。
2. **依赖冷却期（本阶段结论：保持不启用；发布前待决）**：维护者 2026-11-20 决定内测阶段不需要 `global-min-publish-age`，`.cargo/config.toml` 保持无该配置。发布前必须完成启用/不启用的取值决策（触发条件：结束内测、面向外部发布），并确认与 `cargo deny`/`cargo audit` 的分工；该决定尚未发生。
3. **#149 snapshot 必填字段（已完成）**：`parse_command_line` 把 `mode`、`prefix`、`cursorUtf16` 改为必填；删除解码路径的默认 mode/prefix 兜底与 `cursorUtf16` 缺失时重算分支；`cursorUtf16` 始终与 prefix + query 前缀的 UTF-16 长度比较。`commandLine` 整体缺省仍回落到共享协议默认结果；`cursor`/`preedit`/`preeditCursorUtf16`/`codeFontFamily` 的既有可选边界未扩大。`crates/caret-engine/build.rs` 的生成注释由「旧快照省略 prefix 字段时使用的兼容缺省值」改为「返回该模式的共享协议默认前缀」。C++ 测试补了三个字段逐个缺失/类型错误、`cursorUtf16` 不一致与 `commandLine` 整体缺省的行为用例。
4. **#145 数学符号旧常量（已完成）**：删除只被测试或旧兼容语义使用的 `ACCENTS`、`LEGACY_ACCENT_*`、`legacy_accent_values`、`accent_char`、`is_ring_accent`、`legacy_accent_count`，以及 `MathAccent.legacy_character` 字段、`default_legacy_accent_character` 和三参数 `accent!` 形式；保留仍被生产代码使用的 `accent_attribute`。`DEFAULT_MATH_MATRIX_STYLE` 的注释改写为构造期语义：未经持久化校验的构造期/中间 `Matrix` 节点未设置 `style` 时按规范 `matrix` 处理，有效 `.cdoc` 仍必须显式带 `style`；`layout/convert.rs`/`serialize.rs` 的兜底处补了非持久化兼容路径的短注释。常量名、`MATH_MATRIX_STYLES` 内容和 schema 必填性未改。
5. **Qt TSan 排除收敛（已按 label 收敛，上游条件待满足）**：会初始化 QML 引擎或创建窗口的 11 个 `caret_shell_load_test` 用例（`Qml.ShellLoad`、`Qml.MathTextDrag`、9 个 `Qml.Close*`）统一增加 `qt-qml-runtime` label；`caret_add_qml_test` 对 label 去重；`run_ctest_in` 新增 `CtestExclusion { names, labels }`（`-E`/`-LE`），Linux TSan 改为排除 `^qt-qml-runtime$`，`App.HeadlessWindowLoads` 仍按名字排除。`Qml.PreferencesWithoutRuntime` 只构造 ViewModel、不创建引擎，保持运行；`Qml.FormatCheck` 同样保持运行。复查条件（Qt 提供 TSan 插桩或上游结论后复跑 Linux TSan、冲突确认消失即移除排除）已写入注释。

## 范围与非目标

范围内：上述 5 项及其测试、注释、任务记录；按决策成熟度逐项实施、验证与提交。

非目标：不引入新的编辑器功能；不为单项提前抽象；不把 #149/#145 的清理扩大为格式迁移；不引入 Qt 侧 TSan 插桩，也不承诺消除 Qt 内部报告的根因。

## 前置条件与待决策

- Miri nightly 升级：已确认 `nightly-2026-09-22` 的 `miri` 组件可用且 `str::trim_prefix` 已稳定；升级决策记录在本任务与 `tests/src/miri.rs` 注释中。
- 依赖冷却期：当前不启用；仅在结束内测/对外发布前重新评估。
- Qt TSan：取决于 Qt 版本与工具链结论；未确认前不承诺移除 label 排除。

## 实施步骤（已执行）

1. 升级 Miri 固定 nightly 并回填 `trim_prefix`，跑 `cargo ub-check`。
2. 逐项确认并实施 #149 快照必填字段与 #145 数学符号清理，每项补行为测试并同步注释/文档。
3. 把 Linux TSan 的 QML 排除从名字正则收敛为 label，并扩展 CTest runner。
4. 全量门禁、分领域评审与只读 final review；修复后复验并提交 PR。

## 预计改动（实际）

- 工具链/测试：`tests/src/miri.rs`、`crates/caret-math/src/{parser.rs,dimension.rs,layout/frame.rs}`、`crates/launcher/src/ffi_names.rs`。
- 快照协议：`native/viewmodels/document/layout_snapshot_codec.cpp`、`tests/cpp/shell/shell_layout_snapshot_validation_test.cpp`、`crates/caret-engine/build.rs`。
- sanitizer/CTest：`qml/CMakeLists.txt`、`native/cmake/build-policy.cmake`、`tests/src/build.rs`、`tests/src/sanitize.rs`、`tests/src/sanitize/tests.rs`。
- 数学符号：`crates/caret-math/src/symbols.rs`、`crates/caret-doc/src/model/math.rs`、`crates/caret-math/src/{layout/convert.rs,serialize.rs}`。
- 记录：`ai-docs/task/104-rust-1-100-followups.md`、`ai-docs/task-index.md`。

无新增产品依赖；`.cargo/config.toml` 与 `Cargo.lock` 无改动。

## 验收标准

- [x] 每项有明确落地或书面结论（采用 / 不采用及原因）：1、3、4、5 已落地；2 记录「当前不启用 + 发布前重新评估」。
- [x] 代码类改动补用户可观察的行为测试，跑受影响门禁并同步文档；纯文档事项更新对应任务记录。
- [x] Miri nightly 升级后 `cargo ub-check` 通过；5 处 `str::trim_prefix` 回填有独立验证。
- [x] 依赖冷却期有明确决定：内测阶段不启用，外部发布前复核（触发条件已记录）。
- [x] Qt TSan 排除项在证据支持下收敛为 label 方案，并记录继续等待上游的具体条件。
- [x] 分领域评审与未参与实现的只读 final review 完成，发现项已处理复验。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-20 | macOS 27.2.1 arm64，`nightly-2026-09-22` + miri 组件；`cargo +nightly-2026-09-22 check --locked --offline -p caret-app --lib --tests` | 新 nightly 可编译 | 通过（16.96s） |
| 2026-11-20 | `cargo ub-check` | Miri 在升级后的固定 nightly 上通过 | 通过（exit 0）：caret-app `--lib` 83 passed + 1 ignored（268 filtered），dsl-core 各过滤用例 1/2/1/1 passed；共 88 passed |
| 2026-11-20 | `cargo test --locked -p caret-doc` / `-p caret-math` / `-p caret-engine` | #145 清理不改变可观察行为；Matrix 缺 style/columns 的负例被拒绝 | 通过：90 / 146 / 890 项（`incomplete_or_malformed_math_nodes_are_rejected` 增加缺 `style`/缺 `columns` 两个用例） |
| 2026-11-20 | `cargo clippy --locked -p caret-doc -p caret-math --all-targets -- -D warnings`；`RUSTDOCFLAGS=-D warnings cargo doc --no-deps -p caret-doc -p caret-math` | 无警告、注释不引用已删除项 | 通过 |
| 2026-11-20 | `cargo build --locked -p caret-launcher`；`cmake --build target/native/debug --target caret_shell_load_test` | #149/#5 的 C++/CMake 改动可构建 | 通过 |
| 2026-11-20 | C++ 快照测试：`caret_shell_load_test commandLineProtocolDescriptionMatchesCodecBehavior invalidLayoutSnapshotsAreRejectedWithoutReplacingPublishedPages`；`ctest -R '^Qml.ShellLoad$'` | 必填字段与拒绝路径覆盖，错误码固定 | 4 passed / 0 failed；Qml.ShellLoad 1/1 passed（13.80s，带 `qt-qml-runtime` label） |
| 2026-11-20 | `ctest --test-dir target/native/debug -N --show-only=json-v1`；`-LE '^qt-qml-runtime$' -N`；`-E '^App\.HeadlessWindowLoads$' -LE '^qt-qml-runtime$' -N` | 11 个引擎/窗口测试带 label，FormatCheck 与 PreferencesWithoutRuntime 不带；排除数量正确 | 20 tests；11 个带 `qt-qml-runtime`，`Qml.FormatCheck` 仅 `qml`；`-LE` 后 9 个、组合后 8 个 |
| 2026-11-20 | `cargo test --locked -p caret-tests sanitize` | 排除策略断言 | 通过（7 项） |
| 2026-11-20 | 锁定 `cmake-format --check` / `cmake-lint`（两个 CMake 文件）、clang-format（两个 C++ 文件） | 格式与 lint | 通过 |
| 2026-11-21 | `cargo fmt --all -- --check`；`cargo format --check`；`cargo clippy --locked --workspace --all-targets -- -D warnings`；`cargo test --workspace --locked`；`cargo lint --check` | 全量门禁 | 通过：fmt/format ok；clippy 0；workspace 测试 1913 passed + 1 ignored，native CTest 20/20；lint 9 步通过 |
| 2026-11-21 | 分领域评审 2 份 + 未参与实现的只读 final review | 发现项处理并复验 | 完成；必须修复项（Miri 记录指向、label 数量 12→11、验收勾选时序）与建议项（C++ 错误码断言前清空 error、miri 注释、codec 哨兵注释、Matrix 负例等）均已修复复验 |
| — | PR CI 与合并后下一轮 Nightly `sanitizer matrix` | 外部验收 | 待提交 PR 后补记 |

## 风险与回退

- Miri nightly 升级可能引入新的 UB 报告或工具行为变化；若后续出现新失败，回退到原固定日期并撤回依赖新 API 的回填。`trim_prefix` 回填已在稳定 1.100 与固定 nightly 两端验证。
- 快照协议把三个字段改为必填：有效快照由 Rust 侧始终输出，不存在持久化兼容问题；若发现旧 native 消费方构造了缺字段 JSON，会得到明确 `layout.invalid_command_line` 而不是静默默认。
- `MathAccent` 删除了一个公共字段、`caret_math::symbols` 删除了旧常量：`caret-doc`/`caret-math` 均 `publish = false`，工作区内消费点已清理，不面向外部 crate 消费者；这些项原为「只被测试/旧兼容使用」。
- Linux TSan 的 label 排除按「引擎/窗口测试」口径收敛；`Qml.PreferencesWithoutRuntime` 不创建引擎，保持运行，避免损失其 TSan 覆盖。
- 矩阵样式兜底仍存在于构造期路径，本任务只把注释改为正式语义；若要求直接布局输入缺少 `style` 时显式报错，属于另一次行为改动。

## 决策与工作记录

- 2026-11-20：创建任务，汇总 [103](103-rust-1-100-upgrade.md) 的未采用/非目标项与 issue #152 的「顺带清理（Low）」；本任务不改变 103 的完成结论。
- 2026-11-20：开始实施。Miri 固定 nightly 从 `nightly-2026-09-15` 升到 `nightly-2026-09-22`（1.100 周期内、已稳定 `str::trim_prefix`），回填 5 处替换；`cargo ub-check` 通过。依赖冷却期按维护者结论保持不启用，记录发布前复核条件。
- 2026-11-20：#149 快照 `mode`/`prefix`/`cursorUtf16` 改为必填，删除兼容缺省逻辑与生成代码注释，C++ 契约测试覆盖缺失/类型错误/不一致/整体缺省。
- 2026-11-20：#145 删除旧 accent 常量与 `legacy_character` 字段，保留生产使用的 `accent_attribute`；矩阵样式默认值注释改写为构造期语义。
- 2026-11-20：Linux TSan 排除从名字正则改为 `qt-qml-runtime` label（11 个引擎/窗口测试），保留 `App.HeadlessWindowLoads` 名字排除；`Qml.PreferencesWithoutRuntime`（不创建引擎）与 `Qml.FormatCheck` 保持运行。复查条件写入注释。
- 2026-11-20：并行实现后由 lead 集成；全量门禁（fmt/format/clippy/workspace/lint）与 `cargo ub-check` 通过。两个分领域评审（native/sanitizer 与 math/document）及未参与实现的只读 final review 完成，发现项（记录指向、label 覆盖修正、测试错误码断言、注释措辞）已修复并复验。

## 完成摘要

五项后续项已实现或形成明确结论：Miri nightly 升级并回填 `trim_prefix`（`cargo ub-check` 通过）、#149 快照三项必填、#145 旧数学符号清理、Qt TSan 排除按 label 收敛；依赖冷却期保持不启用并记录发布前复核条件。全量门禁（fmt/format/clippy/workspace 测试 1913 passed + 1 ignored、native CTest 20/20、lint）通过，`cargo ub-check` 在新 nightly 上通过；两个分领域评审与未参与实现的只读 final review 已完成，发现项修复后复验。PR CI 与合并后下一轮 Nightly `sanitizer matrix` 结果待补记。
