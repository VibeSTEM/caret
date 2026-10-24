# Rust 编码、crate 与工具链

更新日期：2026-10-25。版本见[技术基线](baseline.md)。DSL 见 [DSL](dsl.md)。测试位置见[测试规范](testing.md)。

## 项目规则

- edition 2024。编译器 channel、rustfmt 和 clippy 写在根 `rust-toolchain.toml`。MSRV 写在 workspace 的 `rust-version`。升级编译器和升级依赖分开验证。
- crate 名使用 `caret-` 前缀。模块和函数用 `snake_case`，类型用 `PascalCase`。
- 领域代码用结构化 `Result`。正常输入、文件和解析路径不用 `unwrap` 或 `expect` 代替错误处理。Clippy 对这两项保持 deny，测试模块可以单独放行。
- ID 使用 newtype。持久化 DTO 和内存对象分开。路径使用 `Path` / `PathBuf`，不假设操作系统路径都能无损变成 UTF-8。
- workspace 默认 `unsafe_code = "deny"`。登记过的例外只有 CXX 生成胶水，以及 `caret-foundation` 里的崩溃信号模块。手写 unsafe 放在专用模块，逐块写 `// SAFETY:`。
- panic 不是业务错误。`panic = abort` 时不能靠 `catch_unwind` 恢复。

格式和 Clippy 的 Rust 入口是 `cargo fmt --all -- --check` 与 `cargo clippy --locked --workspace --all-targets -- -D warnings`。日常跨语言检查走根 `cargo format`、`cargo lint` 和 `cargo verify-changed`。

## 要移植的 crate

来源 crate 在移植时改名。没有消费者的空壳不提前创建。

| 来源 | 规划名 | 带走的内容 |
| --- | --- | --- |
| `panta-build` | `caret-build` | 工具资产、安装事务、LLVM / CMake / Ninja 定位、`.toolchain/` 安装根、`exe_name`、编译数据库、Python 工具供给 |
| `panta-foundation` | `caret-foundation` | 进程早期的崩溃信号。不放文档或界面模型 |
| `panta-dsl-core` | `caret-dsl-core` | `.pa` 的语法、诊断、校验、格式化、求值、Artifact 聚合、TS 输出和 FSM 生成 |
| `panta-dslc` | `caret-dslc` | 单文件检查、格式化和生成的命令行。不链接 Qt |
| `panta-core` 的一部分 | 并入应用服务 crate | `path`、`runtime`、`settings`、`language`、`theme` |
| `panta-ffi` 的骨架 | `caret-ffi` | DTO、错误和句柄转换。不复制来源里的网格和工程接口 |
| launcher | `crates/launcher` | 调度 CMake 并启动桌面程序 |
| `panta-cli` 的进程入口 | `caret-cli` | 把参数适配成 `caret-app` 的命令。不链接 Qt 或 FFI |
| `panta-tests` | `caret-tests` | 格式、lint、测试范围和 native 聚合 |

`panta-mesh`、`panta-import`，以及来源应用 crate 里的网格、材料和工艺模型不移植。来源 CLI 里的 STL 子命令不保留。应用命令行的依赖见[分层](layering.md)。

拆 crate 时看它是否依赖重库、是否有独立生命周期、消费者是不是只需要其中一小块。依赖 Qt 或重型 C++ 库的能力留在 C++ 适配器。领域 crate 不依赖 FFI。不为了猜测复用而建立 `caret-common`。

## 路径

构建 crate 把下载的工具装进 `.toolchain/`，不放进 `target/`。Windows 可执行名补 `.exe`，并在有限深度内查找 `bin/` 下的 CMake 和 LLVM 工具。

应用内逻辑路径移植来源的六类根：

| scheme | 含义 |
| --- | --- |
| `project` | 当前文档或工程根，由服务注入，不依赖进程 cwd |
| `user-config` | 用户配置 |
| `app-data` | 应用数据 |
| `cache` | 可重建缓存 |
| `session` | 临时会话 |
| `qrc` | 只读内置资源，拒绝解析成本机路径 |

引用形如 `scheme:/relative`。三种解析：

- `resolve` 只校验并拼接绝对路径，不访问文件系统。
- `resolve_existing` 要求目标存在，规范化后仍在对应根内。
- `resolve_write_target` 允许目标尚不存在，但最深的现存祖先仍在根内。

拒绝空引用、NUL、绝对路径、越出根的 `..`、Windows 保留设备名、以点或空格结尾的组件，以及逻辑引用里的反斜杠。符号链接和 junction 的越界检查降低误用风险，不是对抗并发篡改文件系统的安全边界。

Qt 适配器用标准目录发现 user-config、app-data、cache 和 session，创建目录并用临时文件验证可写。`QString` 与 UTF-8 必须能往返。file URL 只经 `QUrl::toLocalFile` 解码一次。逻辑引用不是 QML URL，不能直接交给操作系统打开。

安装锁使用 fs4 的显式 `try_lock`，区分 `WouldBlock`，释放时先解锁再关句柄。这是协作锁。文档写入如果以后需要互斥，复用同一套库，不另选一套文件锁。

## 运行时、设置和主题

运行时上下文由 Rust 持有：会话代际、路径服务和已提交的变量快照。后台任务捕获提交当时的快照；上下文关闭后拒绝旧结果。CXX 不暴露上下文句柄。QML 不把 `QObject` 放进 Rust 任务。

设置的 schema、默认值和校验在 Rust。平台存取由 C++ 适配器完成。主题的默认值和覆盖用 `.pa` 描述，校验后发布快照；`Theme.qml` 只绑定这份快照。
