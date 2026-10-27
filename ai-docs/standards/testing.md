# 测试目录与入口

更新日期：2026-10-28。断言风格见 [GTest](gtest.md)、[Rust](rust.md) 和 [QML](qml.md)。CI 范围见[持续集成](ci.md)。

`caret-tests` 已接入。下面是测试目录约定，不因目录树而提前建立空文件。现有 native 聚合入口为 `tests/integration/native.rs`，`cargo test --workspace --locked` 会同时执行 Rust 测试、qmllint 和 CTest；Qt 相关用例使用 Qt Test。GoogleTest 的生产与消费见[依赖获取](dependency-acquisition.md#googletest-生产与消费)。

## 目录

```text
tests/
├── Cargo.toml
├── src/                         # 只放格式、lint 和范围调度
├── integration/native.rs        # 触发 CTest 和 qmllint
├── rust/<domain>.rs             # 公共 API 黑盒测试
├── cpp/<module>/*_test.cpp
├── qml/*_test.cpp
├── support/
├── fixtures/
└── performance/
    ├── rust/cpu/
    └── qt/{cpu,gpu}/
```

- 需要私有项的单元测试留在被测模块的 `#[cfg(test)]` 子模块；较大的测试集在 `module.rs` 中声明 `#[cfg(test)] mod tests;`，内容放到同级 `module/tests.rs`。这样保持私有访问与模块归属，也使覆盖率工具将测试源码排除在生产函数门槛之外。模块文件组织遵循[仓库文件结构](repository-hygiene.md#模块文件结构)。
- 公共行为放 `tests/rust/`，由所属 crate 的 `[[test]]` 显式注册。
- `integration/native.rs` 只检查构建树、qmllint 和 CTest 的退出码，不写领域断言。
- C++ 和 QML 功能测试在 `BUILD_TESTING=ON` 时注册。源文件列表写在 CMake 里，不用宽泛的 `GLOB`。
- 性能基准按 `rust|qt` 与 `cpu|gpu` 分目录；不进入默认 CTest、默认 workspace 测试或 PR/push CI，只由 nightly 手动 suite 执行。Rust 性能范围限 CPU；Qt CPU 与 Qt GPU 分别位于对应目录。源码仍参加格式检查；实际注册的手动基准目标也由 nightly 编译和运行。
- 夹具放 `tests/fixtures/<module>/`。日志和覆盖率放构建树或 `artifacts/`。

## 命令

工作目录为仓库根；前置为 git、rustup 和[平台宿主能力](dependency-acquisition.md#宿主前置)。首次使用会按锁定版本供给工具，需联网。完整检查与 panta 共用相同调度方式，只裁掉不属于 Caret 的领域目标。`format`、`lint` 在 CI 中带 `--check`，省略时可能修复源码。`cargo quality` 聚合只读格式、完整 lint、依赖审计和完整 workspace 测试。

```text
cargo caret-package [新的输出目录]
cargo test --locked --workspace
cargo test-plan
cargo test-scope l1
cargo test-scope l2
cargo test-scope all
cargo verify-changed
cargo format
cargo lint
cargo audit
cargo coverage
cargo sanitize
cargo ub-check
cargo hyperfine <command> [options]
cargo performance
cargo quality
```

`cargo lint` 包含 Clippy、未使用依赖、CMake 格式与 lint、Rust/CMake 分层、qmllint、clang-tidy、include-cleaner 和 cppcheck。Objective-C++ 仅在 macOS 检查。`cargo test --locked -p caret-tests --test native` 是单独的 native 聚合；CI 可通过 `CARET_TEST_CTEST_LABELS` 选择 CTest 标签，空匹配会失败。`CARET_TEST_QML_LINT=false` 仅供已有独立 qmllint job 的 CI 跳过重复扫描，本地默认仍检查。

`l1` 是直接受影响的 Rust package，`l2` 是它们的消费者以及相关 native / QML 测试。这两个名字不是[分层](layering.md)里的层号。未知路径和公共构建边界扩大到完整范围。

`cargo hyperfine <command> [options]` 将参数转交给锁定版本的 hyperfine；没有被测命令时不调用。

日常改动用 `cargo verify-changed`。完整聚合用于范围无法缩小的验收和夜间任务。选择器默认比较工作树和 `origin/main` 的 merge-base。

Rust 覆盖率的夜间门槛是函数和行各 85%。性能数字写明输入、机器、配置和测量方式；没有数据时标成待验证。编辑器的帧时间和排版预算写在架构文档里，不设进这条 CI 门槛。

迁移测试时同时改 CMake、Cargo manifest 和文档，并清掉旧路径。同一用例不留两份。
