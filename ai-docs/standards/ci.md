# 持续集成

更新日期：2026-10-30。CI 配置从 panta `ee84d95` 移植，workflow、action、缓存键和环境变量中的前缀改成 `caret`。保留质量检查机制和矩阵，裁掉不适用的领域目标。

四条 workflow 与配套 action 已在本仓库；本地验证和远程运行证据分别记录在[任务 006](../task/006-infra-ci.md)。配置存在不表示远程运行已通过。

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
- 默认 `contents` 权限只读；只有显式启用 SDK 发布的独立 job 使用 `contents: write`。同一 ref 的 CI 运行可以取消；夜间和打包任务不取消已开始的运行。PR 只恢复缓存，可信 `main` 事件才保存。
- 检查按路径分成依赖审计、Rust lint、native lint、QML、格式和受影响测试。未知路径扩大到完整范围。
- 日常 push/PR 的构建、受影响测试、格式和 lint 只在 Ubuntu 运行，减少每次提交的 runner 成本。native lint 把 clang-tidy、include-cleaner 和 cppcheck 分成独立检查；macOS 的手写 Objective-C++ 检查放在 nightly。
- 格式检查覆盖 Rust、C++ / CXX、CMake、QML 和 `.pa`。
- 夜间任务在 macOS、Ubuntu 和 Windows 上运行 `cargo test --locked --workspace`。Rust 和 native C++ 覆盖率都在夜间按门槛阻断，具体范围和门槛见[测试规范](testing.md)。性能基准不设失败阈值。
- Windows 和 macOS 的常规跨平台构建与测试由 nightly 负责；手动桌面打包、SDK 生产发布仍是三平台矩阵，不能只凭 Ubuntu 成功发布其他平台制品。
- Linux job 安装 QML 运行所需的系统库。工具、Qt 和 GoogleTest 仍经 Cargo 供给到 `.toolchain/`，不装成系统包，也不写入 `target/`。

## 桌面包与独立 SDK

桌面包在仓库根通过 `cargo caret-package [新的输出目录]` 安装；Release 使用 `cargo run --locked --release -p caret-tests -- package [新的输出目录]`。目录非空时拒绝覆盖。包包含运行时资源、Qt 部署依赖、Qt SBOM 和 Cargo 依赖许可证，不安装开发期静态库或头文件。`package.yml` 在三平台检查安装树、打包后移除开发环境变量启动；Windows 另检查 PE manifest，macOS 检查签名。

GoogleTest 的独立生产方式参考 [sdk-packaging](https://github.com/Jack-Lab-at-EIT/sdk-packaging) `a43519510326d26e3118b5d3f888285d4c38c955`。Caret 使用自己的源码、三平台构建、自检和归档管线，资产发布在当前仓库，不下载其他组织的 SDK。源码 commit、C++20、LLVM 和 CRT 元数据及许可说明见[SDK 发布说明](../../tools/sdk/releases/googletest-1.18.0.md)。普通应用构建不从源码生产 SDK。

`sdk-googletest.yml` 默认只上传归档及 SHA256 artifact；手动设置 `publish=true` 才发布当前仓库的 `sdk-googletest-1.18.0`。新 Release 先以 draft 上传完整三平台产物，再公开；已经发布的摘要不可覆盖。生产与发布的脚本存在不表示本次已执行远程发布。消费端仍须先固定经过核验的摘要，不能自动信任下载源旁的校验文件。

## 移植时要改的名字

环境变量和打包文件名使用 `caret`。来源中的 `PANTA_*` 在 Caret 里写成 `CARET_*`，包括系统工具旁路和界面测试用的截图变量。工具缓存指向 `.toolchain/`，不用来源仓库里 `target/` 下的工具目录。选择器比较的基线仍是 `origin/main`，可用参数改成其他 ref。
