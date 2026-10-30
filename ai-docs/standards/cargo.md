# Cargo workspace 与原生调度

更新日期：2026-10-30。语言规则见 [Rust](rust.md)。native 图见 [CMake](cmake.md)。

## 项目规则

- workspace 使用 edition 2024 和显式 `resolver = "3"`。成员、共享依赖和 lint 集中在根 `Cargo.toml`。
- 提交 `Cargo.lock`。CI 使用 `--locked`。不提交指向尚不存在目录的 member，也不为了目录整齐创建没有用途的 crate。
- 默认成员只留桌面 launcher。裸 `cargo run` 启动桌面文档窗口。`caret-cli` 尚未创建，不要使用 `cargo run -p caret-cli`；CLI 计划见[任务 014](../task/014-impl-cli.md)。完整检查必须写 `--workspace`。不带 `--workspace` 的 `cargo test` 只测 launcher。
- Cargo 是用户入口，CMake 拥有 native 构建图。launcher 的 `build.rs` 按 profile 调用 CMake，不读取 Qt 或 CXX 头，也不再启动 Cargo。它的构建依赖是 `caret-build` 和 `caret-dsl-core`；为了让 Cargo 注入 staticlib 头路径，`caret-ffi` 同时是普通依赖。CMake 消费已经生成的 Rust staticlib，不从 CMake 回调 Cargo。
- build script 只把生成物写到 `OUT_DIR`。native 源文件、QML、资源和影响配置的环境变量都要进入重建追踪。
- 交叉编译没有验证时给出不支持的诊断，不用主机探测结果冒充目标平台配置。

根目录 `.cargo/config.toml` 中的质量与开发别名由 `caret-tests` 提供，避免再用 shell 或 Node 做一套入口。命令清单与执行语义集中维护在[测试规范](testing.md#命令)。

`ub-check` 不命名为 `miri`，以免别名挡住真正的 `cargo-miri`。裸 `cargo test` 在默认成员只有 launcher 时不等于完整聚合测试。native 聚合使用 `--no-tests=error`，避免零个测试被看成全部通过。

`cargo package` 保留给 Rust crate。桌面安装入口是 `caret-package`，不附带开发用静态库、头文件或 CMake 导出。
