# 103 — Rust 1.100 工具链升级与 Nightly sanitizer 恢复

- 状态：done
- 依赖：001、006、070
- 优先级：P0
- 创建 / 更新：2026-11-20 / 2026-11-20

## 目标与背景

[Issue #152](https://github.com/VibeSTEM/caret/issues/152) 要求把锁定 Rust 从 1.99.0 升到 1.100.0，并恢复 Nightly `sanitizer matrix`。三个彼此独立的触发点：

- Rust 1.99.1 修复了 x86_64 FFI `bool` 返回值误编译；#149 新增的 `command_line_wire_valid`（`crates/caret-ffi/src/lib.rs`）属于受影响形态，本机未复现但属潜在 UB。
- Cargo 1.100 改用新的 build-dir 布局：`target/[<triple>/]<profile>/build/<package>/<hash>/out`，不再有固定层数的 `build/<pkg>-<hash>/out`，也不再固定生成 `deps`。仓库三处按 `OUT_DIR` 层数推算 target 根的代码失效。
- Nightly sanitizer 有两类失败：Linux TSan 下 #142 新增的 9 个 `Qml.Close*` 测试全部报 Qt 内部 data race（冲突帧在未插桩的 libQt6Core）；macOS ASan/LSan 偶发报告 QML FileDialog/QFileSystemWatcher 的 FSEvents 系统泄漏。

完成后：工具链与 MSRV 为 1.100，构建脚本不依赖 build-dir 层数，1.100 的 clippy/Cargo lint 门禁通过；sanitizer 排除与抑制按失败证据更新（最终效果以合并后下一轮 Nightly 为准）；`MathTree` 解码按维护者决定不再做向前兼容。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)、[提交规范](../standards/commits.md)
- [技术基线](../standards/baseline.md)、[Cargo](../standards/cargo.md)、[Rust](../standards/rust.md)
- [文档存储](../architecture/editor/document-model/document-storage.md)、[数学公式](../architecture/editor/math.md)
- Rust 1.99.1 / 1.100.0 发布说明与 [Cargo CHANGELOG](https://doc.rust-lang.org/cargo/CHANGELOG.html)（2026-11-20 查阅）

## 范围与非目标

已交付：

- `rust-toolchain.toml`、`Cargo.toml` 的版本与 MSRV，以及基线表和任务 001 的版本记录。
- `caret-build` 的 target 根/产物目录定位：沿祖先 `CACHEDIR.TAG` 找 target 根，不再用 `OUT_DIR` 层数；新增 `artifact_dir`，launcher 的 caret-ffi staticlib 与 `caret-tests` 的 target 目录改用它。
- Rust 1.100 clippy 新增的 `needless_bool`、`needless_range_loop`；Cargo 1.100 新增的 `unused_dependencies` 与 `caret-tests` 的 `caret-app` 依赖表归属。
- Nightly sanitizer：Linux TSan 排除表加入 `Qml.Close*` 并更新证据；macOS LSan 抑制表只补充 QML FileDialog/QFileSystemWatcher/FSEvents 帧。
- `MathTree` 解码明确不做向前兼容：`schemaVersion` 与当前版本不匹配（含更高版本）按格式错误拒绝，不再作为 Opaque 保留；只有未知节点类型/结构变体进入 Opaque。同步测试、架构文档和 task 102 的决策记录。
- 可选优化中实际采纳的 8 处测试临时目录 `Drop` 守卫改 `core::mem::DropGuard`。

未采用 / 非目标：

- `str::trim_prefix`（5 处）：实现后确认仓库固定 Miri nightly（`tests/src/miri.rs` 的 `nightly-2026-09-15`）尚未稳定该 API，`cargo ub-check` 会因 E0658 编译失败；已全部回退。Miri nightly 升级与回填由 [104](104-rust-1-100-followups.md) 跟进。
- `.cargo/config.toml` 的 `global-min-publish-age`（依赖冷却期）：维护者确认内测阶段不需要，未采用；发布阶段重新评估由 [104](104-rust-1-100-followups.md) 跟进。
- Issue #152「顺带清理（Low）」中 #149 的 snapshot 必填字段与 #145 的数学符号旧常量，由 [104](104-rust-1-100-followups.md) 跟进。
- 不引入 Qt 侧 TSan 插桩，也不承诺消除 Qt 内部报告的根因；排除项的收敛由 [104](104-rust-1-100-followups.md) 跟进。

## 前置条件与待决策

- 本机 rustup 可安装 1.100.0；CI 的 `caret-toolchain` action 读取 `rust-toolchain.toml`，workflow 无需改。
- Cargo 的临时开关 `__CARGO_TEMPORARY_BUILD_DIR_NEW_LAYOUT_OPT_OUT` 不采用（官方说明后续移除），只按新布局实现。
- 「未知节点类型」的判定：文档结构中未知 `type`、未知数学 `kind`、未知行内环境 `kind` 或未知环境 scope 视为无法解释的节点结构，保留为 Opaque；不匹配（含更高）`schemaVersion` 按格式错误拒绝。已知类型上的未知字段同样应报错，但字段级严格校验属于任务 085 的实现范围，本任务只验证数学版本与节点类型边界；检查只沿当前已知结构路径进行，Opaque 载荷内部不递归解释。

## 实施步骤

1. 升级工具链与 MSRV，用 1.100 下的实际失败逐项对照 issue 清单。
2. 用 `CACHEDIR.TAG` 定位 target 根并新增 `artifact_dir`；改 launcher 与 `caret-tests` build script，补单元测试（含新布局、显式 `--target`、triple 命名根与错误路径）。
3. 清理 1.100 新增 lint 与依赖表：clippy 两处；`caret-tests` 的 `caret-app` 移到 dev-dependencies；launcher 的 `caret-ffi` 误报在 workspace 层关闭 Cargo `unused_dependencies`（未使用依赖仍由 `cargo lint` 的 cargo-machete 步骤负责）。
4. Nightly sanitizer：先取失败 run 日志确认测试名和泄漏栈，再更新 TSan 排除正则（含 `Qml\.Close.*`）与 macOS LSan 抑制表，并更新 `tests/src/sanitize/tests.rs` 断言与注释证据。
5. `MathTree` 解码去掉「未来 schemaVersion → Opaque」分支与 align `source` 特判，更新 `storage_format` 测试、架构文档与 task 102 决策记录。
6. 可选优化逐项确认：完成 8 处 `DropGuard` 转换；`trim_prefix` 与依赖冷却期确认不适用后不保留改动。
7. 全量门禁、分领域评审与只读 final review，修复后复验，提交 PR 并合并。

## 预计改动

- 构建/工具链：`rust-toolchain.toml`、根 `Cargo.toml`、`crates/caret-build/src/{environment,lib}.rs`、`crates/launcher/{build.rs,Cargo.toml}`、`tests/build.rs`、`tests/Cargo.toml`。
- lint/clippy：`crates/caret-engine/src/layout/math_navigation.rs`、`crates/caret-math/src/tests.rs`。
- sanitizer：`tests/src/sanitize.rs`、`tests/src/sanitize/tests.rs`、`tests/fixtures/sanitizers/macos-system-frameworks-lsan.supp`。
- 解码：`crates/caret-doc/src/model/{codec,tests}.rs`、`crates/caret-doc/src/storage_format.rs`、`crates/caret-doc/src/storage_format/{math_capabilities,tests}.rs`。
- 测试夹具：caret-app 的 5 个测试文件、`crates/caret-dslc/src/tests.rs`、`crates/caret-ffi/src/tests.rs`、`crates/caret-build/src/atomic_directory.rs`。
- 文档：`ai-docs/architecture/editor/{math.md,document-model/document-storage.md}`、`ai-docs/standards/{baseline,cargo}.md`、`ai-docs/task/001-*.md`、`ai-docs/task/102-*.md`、`ai-docs/task-index.md`、本任务。

无新增产品依赖；`Cargo.lock` 无改动。

## 验收标准

- [x] `rustc --version` 为 1.100.0；`Cargo.toml` 的 `rust-version` 与基线表一致；launcher 与 native 构建在 1.100 下成功，不依赖 `OUT_DIR` 层数（验证表第 2–5 行）。
- [x] `cargo fmt`、`cargo format --check`、`cargo clippy -D warnings`、`cargo test --workspace`、`cargo lint --check` 通过；PR #153 CI 全绿。
- [x] target 根/profile 目录有单元测试覆盖新布局、显式 `--target`、triple 命名根与错误路径；干净 target 与显式 `--target` 的 launcher 构建通过。
- [x] Linux TSan 排除表覆盖 9 个 `Qml.Close*` 测试且有失败 run 证据；macOS LSan 抑制覆盖 FileDialog/FSEvents 泄漏帧，并用真实失败日志核对、在本机 sanitizer 运行中实际命中（Nightly 复验见「完成摘要」）。
- [x] 更高 `MathTree.schemaVersion` 打开时返回格式错误，不再进入 Opaque/安全预览；未知节点类型仍保留为 Opaque，行为有测试固定；架构文档与 task 102 记录同步。
- [x] 可选优化：8 处 `DropGuard` 完成且行为有测试；`trim_prefix`、依赖冷却期未采用的原因已记录。
- [x] 三个分领域独立评审与未参与实现的只读 final review 完成；发现项（显式 `--target` 的 staticlib 位置、Opaque 测试断言、文档同步、`trim_prefix` 与 Miri nightly 冲突）均已处理并复验。

## 验证计划与结果

| 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- |
| macOS 27.2.1 arm64，`rustup toolchain install 1.100.0`；`cargo metadata --locked` | 安装锁定版本且锁文件兼容 | 通过，rustc 1.100.0 (0ef1bc386 2026-11-09) |
| `cargo build --locked -p caret-launcher` | 新布局下 launcher/native 构建成功 | 通过（含 build script 与 native 增量构建） |
| 干净 target：`cargo build --locked -p caret-launcher --target-dir <临时空目录>` | 无 uplift 产物也能完成构建 | 通过（1m11s）；staticlib 只存在于 `debug/build/caret-ffi/<hash>/out/`，由 build-dir 扫描命中 |
| 干净 target + 显式 triple：`cargo build --locked -p caret-launcher --target aarch64-apple-darwin --target-dir <临时空目录>` | 显式 target 也能完成构建 | 通过（1m23s）；命中 host 构建依赖单元 `<target>/<profile>/build/caret-ffi` |
| `cargo fmt --all -- --check`；`cargo format --check` | 无格式差异 | 通过（评审修复后最终复跑） |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | 无警告 | 通过（final review 修复后最终复跑） |
| `cargo test --workspace --locked` | 全 workspace 通过 | 通过：1914 项 Rust 测试、36 个 test binary；native CTest 20/20（QML 13 项） |
| `cargo lint --check` | 通过 | 通过（clippy、cargo-machete、CMake/layering、qmllint、clang-tidy、include-cleaner、cppcheck） |
| 定向测试：`caret-doc` / `caret-app` / `caret-build --lib` / `caret-dslc` / `caret-ffi` / `caret-math` / `caret-launcher` / `caret-tests sanitize` | 新边界与可选优化不改变行为 | 全部通过（90 / 352 / 26 / 14+4 / 17 / 146 / 9 / 7 项） |
| `CARGO_TARGET_DIR=<临时目录> cargo +nightly-2026-09-15 check --locked --offline -p caret-app --lib --tests` | Miri 固定 nightly 可编译 | 通过（`--lib` 11.15s，`--tests` 亦通过）；未运行完整 `cargo ub-check`/Miri 解释执行 |
| 本机 `cargo sanitize`（只完成 ASan+UBSan profile；TSan profile 未执行） | sanitizer 构建与抑制文件可用 | 构建与 CTest 均执行；新增 `implementation_callback_rpc` 抑制命中 2 次 / 5224 字节（issue 证据：run 44426253052 job 135231750091、44646139582 job 135989089454、44665955367 job 136055176276）。本机另有 3 个纯系统帧泄漏（libobjc 类初始化、CoreGraphics `CGCMSConverterConvertColorComponents`/字形渲染）使 3 个测试失败；栈中无 caret 帧，判断为 macOS 版本差异下的既有系统泄漏，未扩大抑制范围 |
| PR [#153](https://github.com/VibeSTEM/caret/pull/153) / Actions run [44735158384](https://github.com/VibeSTEM/caret/actions/runs/44735158384) | PR CI 通过 | 全部通过：check/build、format、clippy、cmake、layering、machete、qmllint、clang-tidy/includes/cppcheck、audit、math-backend；selector self-test 按路径规则跳过 |
| 合并后下一轮 Nightly `sanitizer matrix`（Linux TSan、macOS ASan/LSan） | 恢复绿色 | 待观察；本机结果不能代替 CI |

## 风险与回退

- 回退：恢复 `rust-toolchain.toml`/`Cargo.toml` 版本并还原 build script 的 target 根解析；1.99 旧布局已被 1.100 取代，长期不保留双布局支持。
- Nightly sanitizer 的 TSan 排除会隐藏这些测试在 Linux TSan 下的执行；只跳过已知 Qt 内部误报，Qt 更新或插桩后复查。
- macOS LSan 抑制若过宽会隐藏真实泄漏；只放 Qt/系统帧，文件头注释与本节都保留复查入口。
- 向前兼容清理会改变既有 `.cdoc` 边界语义：带更高 `MathTree.schemaVersion` 的文件从「只读预览」变为打开失败，测试和文档已同步，避免误判为回归。
- `CACHEDIR.TAG` 搜索假设 build-dir 位于 target 根内；若启用 Cargo 的 `build.build-dir` 重定位需同步调整 `caret-build`（helper 注释已记录）。`CARGO_TARGET_DIR` 恰以 host triple 命名且父目录也是带 tag 的 target 根时可能误判根目录，仓库与 CI 不使用该配置。
- 固定 nightly `nightly-2026-09-15` 未升级：`trim_prefix` 等 1.100 新 API 不能用于 Miri 覆盖的 crate；升级 Miri 时需重跑 `cargo ub-check` 并重新评估这些未采用项。

## 决策与工作记录

- 2026-11-20：创建任务与分支 `yuki/task-152-rust-1-100`；先升级工具链并用实际失败对照 issue 清单。
- 2026-11-20：维护者确认 #150 把更高 `MathTree.schemaVersion` 保留为 Opaque 违反「不做向前兼容、只有未知节点类型才进 Opaque」，本次一并清理。
- 2026-11-20：维护者要求 issue 的「可选优化」全部执行；「顺带清理（Low）」的 #149/#145 项留作独立后续。
- 2026-11-20：实测 Cargo 1.100 布局与 uplift 行为：主 package 构建时 staticlib uplift 到 profile 根，但 `cargo build -p caret-launcher` 的干净 target 没有 uplift，只有 `debug/build/caret-ffi/<hash>/out/`；`ffi_artifacts` 同时扫描两处（issue 里「直接使用 `<target>/debug/libcaret_ffi.a`」只覆盖前者）。
- 2026-11-20：显式 `--target` 的干净 launcher 构建暴露 host 构建依赖单元位置 `<target>/<profile>/build/caret-ffi`（与 `<target>/<triple>/<profile>` 不同）；同时扫描两个单元目录后通过。
- 2026-11-20：`str::trim_prefix` 在稳定 1.100 可用，但固定 Miri nightly `nightly-2026-09-15` 上仍未稳定（E0658）；为保持 Nightly `miri` job 绿色，5 处替换全部回退，并记录为「确认后未采用」。
- 2026-11-20：依赖冷却期 `.cargo/config.toml` 的 `global-min-publish-age` 经维护者确认内测阶段不需要，未采用；`cargo.md` 只保留 Cargo lint 与 cargo-machete 的分工说明。
- 2026-11-20：Cargo 1.100 的 `cargo::unused_dependencies` 只按目标引用判断，无法识别 launcher 经 `DEP_CARET_FFI_*` 和构建顺序消费的 `caret-ffi` 边；在 workspace 层 `allow`（Cargo 不允许包级覆盖 workspace lints），未使用依赖仍由 cargo-machete 兜底。`caret-tests` 的 `caret-app` 仅集成测试使用，移到 `[dev-dependencies]`。
- 2026-11-20：三个分领域独立评审 + 未参与实现的只读 final review；发现项已修复复验。PR #153 通过 CI（run 44735158384）后合并为 `65700c72`，任务关闭。

## 完成摘要

工具链、build-dir 适配、sanitizer 配置、`MathTree` 向前兼容清理和可选优化已实现并合并（PR #153，squash commit `65700c72`）。仓库内验证：`cargo fmt`、`cargo format --check`、`cargo clippy -D warnings`、`cargo test --workspace`（1914 项 + native CTest 20/20）、`cargo lint --check`、`cargo test -p caret-tests sanitize`、干净 target/显式 `--target` launcher 构建、Miri 固定 nightly 编译检查均通过；PR CI 全绿。分领域评审与只读 final review 的发现项均已处理复验。

未采用项与外部待补：

- `str::trim_prefix`、依赖冷却期、#149/#145 清理与 Qt TSan 排除收敛：按确认结论未保留改动，后续由 [104](104-rust-1-100-followups.md) 跟进。
- 合并后下一轮 Nightly `sanitizer matrix` 尚未运行；Linux TSan 排除与 macOS LSan 抑制需由该轮确认，若仍失败按本任务注释中的复查入口继续收敛。
- 本机 macOS 27.2.1 的 ASan+UBSan 仍有 3 个纯系统帧泄漏（libobjc/CoreGraphics），未扩大抑制范围；若 CI runner 出现同类栈需新的证据再决策。
- `cargo ub-check`（Miri 解释执行）未完整运行，只验证固定 nightly 下 `caret-app` 可编译。
- #149/#145 的「顺带清理（Low）」不在本任务范围。
