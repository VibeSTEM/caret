# 测试目录与入口

更新日期：2026-10-24。断言风格见 [GTest](gtest.md)、[Rust](rust.md) 和 [QML](qml.md)。CI 范围见[持续集成](ci.md)。

目录和命令是规划。移植 `caret-tests` 之后，它们才成为可运行入口。

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
```

- 需要私有项的单元测试留在被测 `.rs` 的 `#[cfg(test)]` 模块。
- 公共行为放 `tests/rust/`，由所属 crate 的 `[[test]]` 显式注册。
- `integration/native.rs` 只检查构建树、qmllint 和 CTest 的退出码，不写领域断言。
- C++ 和 QML 功能测试在 `BUILD_TESTING=ON` 时注册。源文件列表写在 CMake 里，不用宽泛的 `GLOB`。
- 性能基准不进入默认 CTest，由 `cargo performance` 运行。源码仍参加格式和 lint。
- 夹具放 `tests/fixtures/<module>/`。日志和覆盖率放构建树或 `artifacts/`。

## 命令

```text
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
cargo quality
```

`l1` 是直接受影响的 Rust package，`l2` 是它们的消费者以及相关 native / QML 测试。这两个名字不是[分层](layering.md)里的层号。未知路径和公共构建边界扩大到完整范围。

日常改动用 `cargo verify-changed`。完整聚合用于范围无法缩小的验收和夜间任务。选择器默认比较工作树和 `origin/main` 的 merge-base。

Rust 覆盖率的夜间门槛是函数和行各 85%。性能数字写明输入、机器、配置和测量方式；没有数据时标成待验证。编辑器的帧时间和排版预算写在架构文档里，不设进这条 CI 门槛。

迁移测试时同时改 CMake、Cargo manifest 和文档，并清掉旧路径。同一用例不留两份。
