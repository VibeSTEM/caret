# 005 — launcher、CMake 与 FFI 骨架

- 状态：done
- 依赖：003、004
- 优先级：P0
- 创建 / 更新：2026-10-25 / 2026-10-25

## 目标与背景

基础设施的最后一块是能打开并关掉的窗口。launcher 调度 CMake，CMake 链接 `caret-ffi` 的 staticlib 和已移植的 QML 模块。窗口内容是组件库的空白页，不是编辑器。

## 必读

- [Cargo](../standards/cargo.md)、[CMake](../standards/cmake.md)、[CXX](../standards/cxx.md)、[FFI](../standards/ffi.md)
- [分层](../standards/layering.md)、[Ninja](../standards/ninja.md)

## 范围与非目标

交付：

- `crates/launcher`。`build.rs` 不读取 Qt 或 CXX 头，也不启动 Cargo；按 profile 调用 CMake。构建依赖是 `caret-build` 与 `caret-dsl-core`。
- `caret-ffi` 导出版本字符串和已校验的内置主题快照。不出现文档、命令或页面快照类型。
- native 入口加载 `Caret.Shell`。关闭窗口后进程退出。窗口内容由 007 接上。
- workspace 默认成员改为 launcher。裸 `cargo run` 打开这个窗口。

不包含：画布、ViewModel 上的文档状态、设置对话框的业务保存。那些在 007 和 008。

## 前置条件与待决策

003、004 已 done。004 已通过 `caret-build` 将 Qt 装到 `.toolchain/`，并交付最小 CMake 工程和可独立加载的 QML 模块。

## 实施步骤

1. 移植 launcher，在 004 的 CMake 工程上增加桌面应用和 FFI 链接，复用已有模块与测试登记，改名前缀。记录来源 commit。
2. 写最小 `#[cxx::bridge]`。实现只转发，不放业务。
3. 空白窗口加载 QML 模块。确认没有把来源的工艺页登记进工程。
4. 跑通配置、编译、启动、关闭。Debug 和 Release 都用 `/MD` 的平台按规范检查运行库。
5. 独立评审后复验。

## 预计改动

新建 `crates/launcher/`、`crates/caret-ffi/`、`qml/App.qml` 的空白页，扩展 004 已有的 `native/` CMake 工程。CMake 不回调 Cargo。

## 验收标准

- [x] `cargo run` 打开窗口，关闭后进程退出，不残留子构建进程。
- [x] `caret-ffi` 不依赖 `caret-engine`。launcher 的依赖停在 `caret-ffi`、`caret-build`、`caret-dsl-core`。
- [x] CMake 消费已经生成的 staticlib，不从 `build.rs` 再启动 Cargo。
- [x] 窗口里看不到工艺面板。fmt、clippy 和本次 native 构建通过。评审发现已处理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-25 | macOS arm64，`cargo build --locked -p caret-launcher` 后 `QT_QPA_PLATFORM=offscreen target/debug/caret-launcher --quit-after-load` | 空白窗口加载后退出，退出码 0 | 退出码 0。`--version` 打印 `0.1.0`。未知参数退出码 64。结束后没有 cmake 或 ninja 子进程 |
| 2026-10-25 | macOS arm64，`cargo clippy --locked --workspace --all-targets -- -D warnings` 与 `cargo fmt --all -- --check` | 通过 | 通过 |
| 2026-10-25 | macOS arm64，`cargo test --workspace --locked --exclude caret-launcher` 与 `cargo test --locked -p caret-launcher` | 通过 | 通过。launcher 5 项，含 staticlib 文件名不选中 `caret_ffi_bridge` |
| 2026-10-25 | 分层允许表与 Cargo.toml | ffi 运行时只依赖 `caret-app` 以导出已校验的主题快照；launcher 停在 ffi、build、dsl-core | 与 `tests/src/layering.rs` 一致。Windows `/MD` 已写入 `build-policy.cmake`，本机未执行 |

## 风险与回退

Qt 与 CXX 的运行库不一致时，链接会在启动时失败。失败就停在本任务，不把库路径硬编码到用户机器。回退是恢复默认成员并去掉 native 目标。

## 决策与工作记录

- 2026-10-25：创建任务。编辑器 DTO 不在这层骨架里预留。
- 2026-10-25：与 004 同一分支落地。`caret-ffi` 导出版本字符串和已校验的内置主题快照，运行时依赖 `caret-app`。`build.rs` 用托管 Clang 和 Ninja 调 CMake，并把 `caret-ffi` 的 staticlib 传进去。为让 Cargo 注入 `DEP_CARET_FFI_INCLUDE`，`caret-ffi` 同时是普通依赖。
- 2026-10-25：独立评审没有阻塞项。CMake 分层夹具仍未移植，根别名 `lint` 里的那一项留到后续任务。007 把空白页换成 M0 窗口。

## 完成摘要

`cargo run` 打开桌面窗口，关闭后进程退出。`caret-ffi` 提供链接和内置主题快照。工艺面板不在这个窗口里。007 负责窗口内容。
