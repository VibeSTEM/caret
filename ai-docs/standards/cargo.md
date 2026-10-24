# Cargo workspace 与原生调度

更新日期：2026-10-25。语言规则见 [Rust](rust.md)。native 图见 [CMake](cmake.md)。

## 项目规则

- workspace 使用 edition 2024 和显式 `resolver = "3"`。成员、共享依赖和 lint 集中在根 `Cargo.toml`。
- 提交 `Cargo.lock`。CI 使用 `--locked`。不提交指向尚不存在目录的 member，也不为了目录整齐创建没有用途的 crate。
- 默认成员只留桌面 launcher。裸 `cargo run` 启动 M0 文档窗口，页面仍是占位。`caret-cli` 用 `cargo run -p caret-cli`。完整检查必须写 `--workspace`。不带 `--workspace` 的 `cargo test` 只测 launcher。
- Cargo 是用户入口，CMake 拥有 native 构建图。launcher 的 `build.rs` 按 profile 调用 CMake，不读取 Qt 或 CXX 头，也不再启动 Cargo。它的构建依赖是 `caret-build` 和 `caret-dsl-core`；为了让 Cargo 注入 staticlib 头路径，`caret-ffi` 同时是普通依赖。CMake 消费已经生成的 Rust staticlib，不从 CMake 回调 Cargo。
- build script 只把生成物写到 `OUT_DIR`。native 源文件、QML、资源和影响配置的环境变量都要进入重建追踪。
- 交叉编译没有验证时给出不支持的诊断，不用主机探测结果冒充目标平台配置。

规划中的根别名指向 `caret-tests`，避免再用 shell 或 Node 做一套入口：

```text
caret-package    安装当前 profile 的桌面运行目录
test-plan        显示受影响的测试范围
test-scope       运行 L1、L2 或全部受影响测试
verify-changed   变更范围内的格式、lint 和测试
quality          本地完整质量检查
format / lint    跨语言格式与静态检查
audit            依赖、许可证和 RustSec
coverage         Rust 覆盖率
performance      手动性能基准
sanitize         平台允许的 sanitizer
ub-check         纯 Rust 的补充动态检查
```

`ub-check` 不命名为 `miri`，以免别名挡住真正的 `cargo-miri`。裸 `cargo test` 在默认成员只有 launcher 时不等于完整聚合测试。native 聚合使用 `--no-tests=error`，避免零个测试被看成全部通过。

`cargo package` 保留给 Rust crate。桌面安装入口是 `caret-package`，不附带开发用静态库、头文件或 CMake 导出。
