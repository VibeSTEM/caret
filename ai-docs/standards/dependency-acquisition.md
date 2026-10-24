# 依赖获取与主平台环境

更新日期：2026-10-24。版本选择见[技术基线](baseline.md)。供给程序计划落在 `caret-build`，行为对齐 panta 的构建 crate：固定 URL 与 SHA256、安装到构建树、缺包即失败。

## 托管原则

- Cargo 把预编译工具装到 `target/caret-tools/`，把 Qt 和 GoogleTest 装到 `target/caret-deps/`。普通构建不编译这些第三方的源码，也不把它们写进源码树。
- 资产必须匹配平台、架构和 ABI。个人机器上的同名编译器或 Qt 不构成基线。
- 只有显式设置 `CARET_USE_SYSTEM_TOOLS=1` 时，才读取 `CMAKE`、`CC`、`CXX` 或 `PATH` 里的本机工具。这条旁路只选择工具，不表示该平台的 Qt 已验收。

## 宿主前置

| 平台 | 开发者安装的能力 |
| --- | --- |
| 全部 | git、rustup。Rust 版本由根 `rust-toolchain.toml` 固定。首次供给需要网络，校验过的缓存可以离线构建 |
| macOS | Apple 命令行工具、SDK 和系统运行库。托管 LLVM 使用这份 SDK 作为 sysroot |
| Linux | glibc、链接用的 C 编译器、OpenGL 开发文件，以及 QML 所需的 EGL、xkbcommon 和软件渲染驱动 |
| Windows | MSVC Build Tools、Windows SDK 和 CRT。项目编译器是托管 clang-cl |

## 固定清单

精确 URL 和摘要放在供给代码里，本页不复制第二份。移植时一并带走来源仓库里的这些入口：

| 依赖 | 来源中的入口 | Caret 中的消费方式 |
| --- | --- | --- |
| CMake / Ninja / LLVM / uv / hyperfine | 构建 crate 的资产表 | 官方预编译包。格式化和 clang-tidy 使用其中的 LLVM |
| Qt | native 的 Qt 供给脚本 | 官方模块包。启用界面桥时再查找 StateMachine |
| Linux ICU | 同一 Qt 供给 | 与 Qt Linux 工具 ABI 匹配，不替换成系统 ICU |
| GoogleTest | SDK 清单 | `find_package(GTest CONFIG)`，仅测试构建 |

Qt 缓存在 `target/caret-deps/qt/staging`，跨 profile 共享。官方源失败时可以尝试来源仓库已有的镜像策略；每个来源都做强校验。SDK 放在 `target/caret-deps/sdk/<name>/<version>/<triple>`，用限制搜索路径的 `find_package` 消费。

## 下载预算

工具下载由父进程限制含重试在内的总时间，解包和 Windows 安装另有上限。超时不发布完成标记。不能只用单次传输的超时代替总预算，因为重试会重置那次计时。

## 缓存

CI 只缓存 Cargo registry 和能够按版本与摘要校验的工具资产。不缓存 `target/native` 或 Cargo 的编译产物。缓存键包含平台、架构、工具版本和供给清单。
