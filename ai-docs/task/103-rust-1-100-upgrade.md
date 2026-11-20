# 103 — Rust 1.100 工具链升级与 Nightly sanitizer 恢复

- 状态：done
- 依赖：001、006、070
- 优先级：P0
- 创建 / 更新：2026-11-20 / 2026-11-20

## 目标与背景

[Issue #152](https://github.com/VibeSTEM/caret/issues/152) 要求把锁定 Rust 从 1.99.0 升到 1.100.0，并恢复 Nightly `sanitizer matrix`。三个彼此独立的触发点：

- Rust 1.99.1 修复了 x86_64 FFI `bool` 返回值的误编译；#149 新增的 `command_line_wire_valid`（`crates/caret-ffi/src/lib.rs`）是受影响形态，本机未复现但属潜在 UB。
- Cargo 1.100 改用新的 build-dir 布局（`target/<profile>/build/<package>/<hash>/out`，不再有固定层数的 `build/<pkg>-<hash>/out`，也不再固定生成 `deps`），仓库三处按 `OUT_DIR` 层数推算 target 根的代码会失效。
- Nightly sanitizer 有两类失败：Linux TSan 下 #142 新增的 9 个 `Qml.Close*` 测试全部报 Qt 内部 data race（冲突帧在未插桩的 libQt6Core）；macOS ASan/LSan 偶发报告 QML FileDialog/QFileSystemWatcher 的 FSEvents 系统泄漏。

完成后：锁定工具链与 MSRV 为 1.100，构建脚本不再依赖 build-dir 层数，1.100 的 clippy/Cargo lint 门禁通过，Nightly sanitizer 在 Linux/macOS 恢复绿色；同时按维护者决定清理 `MathTree` 解码中把更高 `schemaVersion` 保留为 Opaque 的向前兼容行为。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)、[提交规范](../standards/commits.md)
- [技术基线](../standards/baseline.md)、[Cargo](../standards/cargo.md)、[Rust](../standards/rust.md)
- [文档存储](../architecture/editor/document-model/document-storage.md)、[数学公式](../architecture/editor/math.md)
- Rust 1.99.1 / 1.100.0 发布说明与 [Cargo CHANGELOG](https://doc.rust-lang.org/cargo/CHANGELOG.html)（2026-11-20 查阅）

## 范围与非目标

范围内：

- `rust-toolchain.toml`、`Cargo.toml` 的版本与 MSRV，以及基线表和任务 001 的版本记录。
- `caret-build` 的 target 根/profile 产物定位：沿 `CACHEDIR.TAG` 找 target 根，不再用 `OUT_DIR` 层数；launcher 的 caret-ffi staticlib 与 `caret-tests` 的 target 目录改用它。
- Rust 1.100 clippy 新增的 `needless_bool`、`needless_range_loop`；Cargo 1.100 新增的 `unused_dependencies` 与 `tests` 包 `caret-app` 的依赖表归属。
- Nightly sanitizer：Linux TSan 排除表加入 `Qml.Close*` 并更新证据；macOS LSan 抑制表补充 QML FileDialog/FSEvents 帧。
- Issue 列出的可选优化：`.cargo/config.toml` 的 `global-min-publish-age`、8 处测试临时目录 `Drop` 守卫改 `core::mem::DropGuard`。5 处 `str::trim_prefix` 在实现后按评审撤回：仓库固定 Miri nightly（`tests/src/miri.rs`）早于该 API 稳定，`cargo ub-check` 会编译失败；不为此在本任务升级 Miri 工具链（属独立工具链决策）。
- `MathTree` 解码明确不做向前兼容：`schemaVersion` 高于当前版本按格式错误拒绝，不再作为 Opaque 保留；只有未知节点类型才进入 Opaque。同步更新测试、架构文档和 task 102 的决策记录。

非目标：

- Issue #152 的「顺带清理（Low）」中 #149 的 snapshot 必填字段与 #145 的数学符号旧常量，属于独立后续任务，本次不改。
- 不引入 Qt 侧 TSan 插桩，也不承诺消除 Qt 内部报告的根因。

## 前置条件与待决策

- 本机 rustup 可安装 1.100.0；CI 的 `caret-toolchain` action 读取 `rust-toolchain.toml`，workflow 无需改。
- Cargo 的临时开关 `__CARGO_TEMPORARY_BUILD_DIR_NEW_LAYOUT_OPT_OUT` 不采用（官方说明后续移除），只按新布局实现。
- 「未知节点类型」的判定：文档结构中未知 `type`、未知数学 `kind`、未知行内环境 `kind` 或未知环境 scope 视为无法解释的节点结构，保留为 Opaque；不匹配（含更高）`schemaVersion` 按格式错误拒绝。已知类型上的未知字段按档案规范同样应报错，但字段级严格校验属于任务 085 的实现范围，本任务只验证数学版本与节点类型边界；检查只沿当前已知结构路径进行，未知 Opaque 载荷内部不递归解释。

## 实施步骤

1. 在专用分支升级工具链与 MSRV，跑 1.100 下的 fmt/clippy/test 收集实际失败，逐项对照 issue 清单。
2. 用 `CACHEDIR.TAG` 定位 target 根，补齐 profile 产物目录；改 launcher 与 `caret-tests` build script，并补单元测试。
3. 清理 1.100 新增 lint：clippy 两处；`caret-tests` 的 `caret-app` 移到 dev-dependencies；launcher 的 `caret-ffi` 误报在 workspace 层关闭 Cargo `unused_dependencies`（未使用依赖仍由 `cargo lint` 的 cargo-machete 步骤负责）。
4. Nightly sanitizer：先取失败 run 的日志确认测试名和泄漏栈，再更新 TSan 排除正则（含 `Qml\.Close.*`）与 macOS LSan 抑制表，并更新 `tests/src/sanitize/tests.rs` 断言与注释证据。
5. `MathTree` 解码去掉未来 schemaVersion 的 Opaque 分支与对齐 `source` 特判，更新 `storage_format` 测试、架构文档与 task 102 决策记录。
6. 可选优化按 issue 清单完成；每项保持行为不变并跑受影响测试。
7. 全量门禁（`cargo format --check`、`cargo lint --check`、`cargo test --workspace --locked`、`cargo test -p caret-tests --test native`）后做独立评审；按分支实际改动委派领域检查和只读 final review，修复后复验。

## 预计改动

`rust-toolchain.toml`、根 `Cargo.toml`、`.cargo/config.toml`、`crates/caret-build/src/environment.rs`、`crates/caret-build/src/lib.rs`、`crates/launcher/build.rs`、`crates/launcher/Cargo.toml`、`tests/build.rs`、`tests/Cargo.toml`、`crates/caret-engine/src/layout/math_navigation.rs`、`crates/caret-math/src/tests.rs`、`tests/src/sanitize.rs`、`tests/src/sanitize/tests.rs`、`tests/fixtures/sanitizers/macos-system-frameworks-lsan.supp`、`crates/caret-doc/src/model/codec.rs`、`crates/caret-doc/src/storage_format*.rs`、若干测试临时目录夹具（`crates/caret-math/{parser.rs,dimension.rs,layout/frame.rs}` 与 `crates/launcher/src/ffi_names.rs` 的 `trim_prefix` 替换已回退、无 diff）、`ai-docs/standards/baseline.md`、`ai-docs/task/001-*.md`、`ai-docs/task/102-*.md`、`ai-docs/task-index.md`。无新增产品依赖。

## 验收标准

- [ ] `rustc --version` 为 1.100.0；`Cargo.toml` 的 `rust-version` 与基线表一致；launcher 与 native 构建在 1.100 下成功，不依赖 `OUT_DIR` 层数。
- [ ] `cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --check`、`cargo test --workspace --locked` 通过。
- [ ] build script 的 target 根/profile 目录有单元测试覆盖新布局、显式 `--target` 和错误路径。
- [ ] Linux TSan 排除表覆盖 9 个 `Qml.Close*` 测试且有失败 run 证据；macOS LSan 抑制覆盖 FileDialog/FSEvents 泄漏帧并有 run 证据。
- [ ] 更高 `MathTree.schemaVersion` 打开时返回格式错误，不再进入 Opaque/安全预览；未知节点类型仍保留为 Opaque，行为有测试固定；架构文档与 task 102 记录同步。
- [ ] 可选优化完成且行为有测试保证；未采用项记录原因。
- [ ] 独立领域检查与只读 final review 完成，发现的问题已处理并复验；范围及限制有记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-20 | macOS 27.2.1 arm64，`rustup toolchain install 1.100.0` | 安装锁定版本 | 已安装 rustc 1.100.0 (0ef1bc386 2026-11-09) |
| 2026-11-20 | `cargo metadata --locked` | 锁文件兼容 | 通过 |
| 2026-11-20 | `cargo build --locked -p caret-launcher` | 新布局下 launcher 与 native 构建成功 | 通过（含重建 build script 与 native 增量构建） |
| 2026-11-20 | `cargo clippy --locked --workspace --all-targets -- -D warnings` | 无警告 | 通过（final review 修复后最终复跑；修复 `needless_bool`、`needless_range_loop`，workspace `unused_dependencies` 不再报告） |
| 2026-11-20 | `cargo build --locked -p caret-launcher --target-dir <临时空目录>`（干净 target） | 无 uplift 产物也能完成 native 构建 | 通过（1m11s），staticlib 只存在于 `debug/build/caret-ffi/<hash>/out/`，由 build-dir 扫描命中 |
| 2026-11-20 | `cargo build --locked -p caret-launcher --target aarch64-apple-darwin --target-dir <临时空目录>`（显式 triple + 干净 target） | 显式 target 也能完成构建 | 通过（1m23s）；build script 运行前产出的是 host 构建依赖单元，扫描 `<target>/<profile>/build/caret-ffi` 后命中 |
| 2026-11-20 | `CARGO_TARGET_DIR=<临时目录> cargo +nightly-2026-09-15 check --locked --offline -p caret-app --lib --tests` | Miri 固定 nightly 可编译（trim_prefix 回退后，含 DropGuard） | 通过（`--lib` 11.15s；最终评审补跑 `--tests` 亦通过）；未运行完整 `cargo ub-check`/Miri 解释执行 |
| 2026-11-20 | `cargo fmt --all -- --check` | 无差异 | 通过（评审修复后最终复跑） |
| 2026-11-20 | `cargo format --check`（跨语言入口） | 通过 | 通过（评审修复后最终复跑） |
| 2026-11-20 | `cargo test --workspace --locked` | 全 workspace 通过 | 通过（final review 修复后最终复跑：1914 项 Rust 测试、36 个 test binary；native CTest 20/20，含 QML 13 项） |
| 2026-11-20 | `cargo test --locked -p caret-doc` / `-p caret-app` | codec 新边界与消费者通过 | 通过（90 / 352 项） |
| 2026-11-20 | `cargo test --locked -p caret-build --lib` | target 根单元测试 | 通过（26 项，含新布局/显式 triple/triple 命名根/错误路径） |
| 2026-11-20 | `cargo test --locked -p caret-app -p caret-dslc -p caret-ffi -p caret-math -p caret-launcher` | 可选优化不改变行为 | 全部通过（352 / 14+4 / 17 / 146 / 9 项） |
| 2026-11-20 | `cargo test --locked -p caret-tests sanitize` | sanitizer 排除表断言 | 通过（7 项） |
| 2026-11-20 | `cargo lint --check` | 通过 | 通过（含 clippy、cargo-machete、CMake/layering、qmllint、clang-tidy、include-cleaner、cppcheck）；独立评审与 final review 修复后再次通过 |
| 2026-11-20 | 本机 `cargo sanitize`（macOS 27.2.1；只完成 ASan+UBSan profile，TSan profile 未执行） | sanitizer 构建与抑制文件可用 | ASan+UBSan 插桩 native 树构建、CTest 均执行；新增 `implementation_callback_rpc` 抑制实际命中 2 次 / 5224 字节（issue 证据 run：44426253052 job 135231750091、44646139582 job 135989089454、44665955367 job 136055176276；macOS 日志的 10 个分配全部被新增 Qt/FSEvents 模式覆盖）。本机另有 3 个纯系统帧泄漏使 `Qml.ShellLoad`、`Qml.CloseRecoveryWriteFailure`、`App.HeadlessWindowLoads` 失败：libobjc 类初始化（libdispatch 线程）与 CoreGraphics `CGCMSConverterConvertColorComponents`/字形渲染；栈中无 caret 帧，且本分支未改 QML/native 渲染代码，判断为本机 macOS 版本差异下的既有系统泄漏，不据此扩大抑制范围 |
| 2026-11-20 | PR [#153](https://github.com/VibeSTEM/caret/pull/153) / Actions run [44735158384](https://github.com/VibeSTEM/caret/actions/runs/44735158384) | PR CI 通过 | 全部通过：cargo check/build、format、clippy、cmake、layering、machete、qmllint、clang-tidy/includes/cppcheck、audit、math-backend；selector self-test 按路径规则跳过 |
| — | Nightly `sanitizer matrix`（Linux TSan、macOS ASan/LSan） | 恢复绿色 | 待合并后下一轮 Nightly 复验（本机结果见上一行，不能替代 CI） |

## 风险与回退

- 回退：恢复 `rust-toolchain.toml`/`Cargo.toml` 版本并还原 build script 的 target 根解析；1.99 旧布局已被 1.100 取代，长期不保留双布局支持。
- Nightly sanitizer 的 TSan 排除会隐藏这些测试在 Linux TSan 下的执行；只跳过已知 Qt 内部误报，Qt 更新或插桩后复查。
- macOS LSan 抑制若过宽会隐藏真实泄漏；只放 Qt/系统帧，并在注释中保留未抑制报告的复查入口。
- 向前兼容清理会改变既有 `.cdoc` 边界语义：带更高 `MathTree.schemaVersion` 的文件从「只读预览」变为打开失败，需要测试和文档同步，避免误判为回归。

## 决策与工作记录

- 2026-11-20：创建任务。分支 `yuki/task-152-rust-1-100`；先升级工具链并用实际失败对照 issue 清单。
- 2026-11-20：维护者确认 #150 把更高 `MathTree.schemaVersion` 保留为 Opaque 违反「不做向前兼容、只有未知节点类型才进 Opaque」，本次一并清理。
- 2026-11-20：维护者要求 issue 的「可选优化」全部执行；「顺带清理（Low）」的 #149/#145 项仍留作独立后续。
- 2026-11-20：实测 Cargo 1.100 布局为 `<target>/[<triple>/]<profile>/build/<package>/<hash>/out`，target 根与 triple 层都写 `CACHEDIR.TAG`；显式 `--target` 时 `target` 根的 triple 子层需跳过。launcher 的 caret-ffi staticlib 在主 package 构建（如 `cargo build -p caret-ffi`）时 uplift 到 profile 根；但 `cargo build -p caret-launcher` 的干净 target 实测没有 uplift，只有 `debug/build/caret-ffi/<hash>/out/libcaret_ffi-<hash>.a`，因此两处都按新布局查找（issue 里「直接使用 `<target>/debug/libcaret_ffi.a`」只覆盖前者）。
- 2026-11-20：显式 `--target` 的干净 launcher 构建暴露第二个 staticlib 位置：build script 的 host 构建依赖单元在 `<target>/<profile>/build/caret-ffi`，与 `<target>/<triple>/<profile>` 不同；`ffi_artifacts` 同时扫描两个单元目录后，`cargo build -p caret-launcher --target <host-triple> --target-dir <干净目录>` 通过。
- 2026-11-20：`str::trim_prefix` 在稳定 1.100 可用，但固定 Miri nightly `nightly-2026-09-15` 上仍未稳定（E0658）；为保持 Nightly `miri` job 绿色，5 处替换全部回退为 `strip_prefix(..).unwrap_or(..)`，并记录为「确认后未采用」；升级 Miri nightly 是独立工具链决策。
- 2026-11-20：Cargo 1.100 的 `cargo::unused_dependencies` 只按目标引用判断，无法识别 launcher 经 `DEP_CARET_FFI_*` 和构建顺序消费的 `caret-ffi` 边；在 workspace 层 `allow`，未使用依赖仍由 `cargo lint` 的 cargo-machete 步骤负责。`tests` 的 `caret-app` 仅集成测试使用，移到 `[dev-dependencies]`。

## 完成摘要

工具链升级、Cargo 1.100 build-dir 适配、Nightly sanitizer 修复、`MathTree` 解码向前兼容清理和可选优化已实现，PR [#153](https://github.com/VibeSTEM/caret/pull/153) 通过 CI；`str::trim_prefix` 在确认固定 Miri nightly 尚未稳定后回退。仓库内验证通过：`cargo fmt --all -- --check`、`cargo format --check`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo test --workspace --locked`（1914 项 + native CTest 20/20）、`cargo lint --check`、`cargo test -p caret-tests sanitize`、干净 target 与显式 `--target` 的 launcher 构建、Miri 固定 nightly 的 `caret-app` `--lib`/`--tests` 编译检查。分领域评审与未参与实现的只读 final review 均完成，发现项（显式 target 的 staticlib 位置、测试断言、文档同步、trim_prefix 与 Miri nightly 冲突）已处理并复验。

未完成 / 外部待补：
- PR #153 CI 已通过并按维护者指示合并；下一轮 Nightly `sanitizer matrix`（Linux TSan、macOS ASan/LSan）尚待运行观察，Nightly「恢复绿色」不能以本机结果代替，失败时按本任务复查入口继续收敛。
- 本机 macOS 27.2.1 的 ASan+UBSan 仍报告 3 个纯系统帧泄漏（libobjc/CoreGraphics），未扩大抑制范围；若 CI runner 出现同类栈需要新的证据再决策。
- `cargo ub-check`（Miri 解释执行）未完整运行；只验证了固定 nightly 下 `caret-app` 可编译。
- #149/#145 的「顺带清理（Low）」不在本任务范围。
