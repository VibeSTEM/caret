# 持续集成

更新日期：2026-10-24。CI 配置从 panta 移植，并在移植时把 workflow、action、缓存键和环境变量中的 `panta` 前缀改成 `caret`。改名保持矩阵和行为，不顺便重设计。

这些 workflow 目前不在本仓库。落地前，任务文档里的命令是规划入口。

## 要移植的工作流

| 来源 | 作用 |
| --- | --- |
| `ci.yml` | pull request 和 `main` 按影响范围构建、测试和检查 |
| `nightly.yml` | 定时跑完整 workspace 测试、全量 lint、覆盖率和长时动态检查 |
| `package.yml` | 手动打三平台可搬迁的桌面包，上传 artifact |
| `sdk-googletest.yml` | 手动生产 GoogleTest 静态 SDK |

几何、网格和渲染 SDK 的生产工作流不移植。

复合 action 一并移植：工具链安装、变更路径分类、Linux GL 前置、工具与依赖缓存、质量工具缓存。Rust 组件仍从 `rust-toolchain.toml` 读取 channel。

## CI 的形状

- 纯文档变更不创建运行。`crates/`、`tests/`、`native/`、`qml/`、`resources/` 里的 Markdown 夹具仍要触发。
- `contents` 权限保持只读。同一 ref 的进行中运行可以取消；夜间和打包任务不取消已开始的运行。
- 检查按路径分成依赖审计、Rust lint、native lint、QML、格式和受影响测试。未知路径扩大到完整范围。
- native lint 把 clang-tidy、include-cleaner 和 cppcheck 分成独立检查。macOS 另查手写 Objective-C++。
- 格式检查覆盖 Rust、C++ / CXX、CMake、QML 和 `.pa`。
- 夜间任务在 macOS、Ubuntu 和 Windows 上运行 `cargo test --locked --workspace`。Rust 覆盖率的函数和行门槛各为 85%，只在夜间阻断。native 覆盖率归档报告。性能基准不设失败阈值。
- Linux job 安装 QML 运行所需的系统库。工具、Qt 和 GoogleTest 仍经 Cargo 供给到 `.toolchain/`，不装成系统包，也不写入 `target/`。

## 移植时要改的名字

环境变量和打包文件名使用 `caret`。来源中的 `PANTA_*` 在 Caret 里写成 `CARET_*`，包括系统工具旁路和界面测试用的截图变量。工具缓存指向 `.toolchain/`，不用来源仓库里 `target/` 下的工具目录。选择器比较的基线仍是 `origin/main`，可用参数改成其他 ref。
