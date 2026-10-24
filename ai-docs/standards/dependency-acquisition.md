# 依赖获取与主平台环境

更新日期：2026-10-25。版本选择见[技术基线](baseline.md)。供给程序在 `caret-build`：固定 URL 与 SHA256、安装到仓库根的 `.toolchain/`、缺包即失败。不把工具装进 `target/`。

## 目录

`.toolchain/` 只放下载和解包后的依赖。Cargo 编译产物、native 构建树和 `OUT_DIR` 仍在 `target/`。`cargo clean` 不清 `.toolchain/`。要重装工具时单独删除对应子目录。

默认根是仓库根下的 `.toolchain/`。`CARET_TOOLCHAIN_ROOT` 可以把它指到别处，未设置时不回退到 `target/`。

```text
.toolchain/
├── tools/<name>/<id>/                 # CMake、Ninja、LLVM、uv、hyperfine、Python
├── qt/staging/                        # Qt 与 Linux ICU，各 profile 共用
├── sdk/<name>/<version>/<triple>/     # GoogleTest 等静态 SDK
├── archives/                          # 下载校验时的临时压缩包，发布成功后删除
└── locks/                             # 安装锁
```

`<id>` 包含版本、平台、架构和摘要，避免不同资产共用一个目录。整个 `.toolchain/` 不提交。

成功发布后删除归档，避免和已解包的工具长期各存一份。安装树损坏时删掉对应的 `tools/<name>/<id>/` 再下载。不靠常驻压缩包离线重装。

## 托管原则

- 普通构建不编译这些第三方的源码，也不把它们写进源码树或 `target/`。
- 资产必须匹配平台、架构和 ABI。个人机器上的同名编译器或 Qt 不构成基线。
- 只有显式设置 `CARET_USE_SYSTEM_TOOLS=1` 时，才读取 `CMAKE`、`CC`、`CXX` 或 `PATH` 里的本机工具。这条旁路只选择工具，不表示该平台的 Qt 已验收。
- rustup 按 `rust-toolchain.toml` 管理 Rust 编译器，不复制进 `.toolchain/`。crates.io 的源码缓存仍用 Cargo 自己的 registry。

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

Qt 放在 `.toolchain/qt/staging`，跨 profile 共享。官方源失败时可以尝试来源仓库已有的镜像策略；每个来源都做强校验。SDK 放在 `.toolchain/sdk/<name>/<version>/<triple>/`，用限制搜索路径的 `find_package` 消费。

## 下载预算

工具下载由父进程限制含重试在内的总时间，解包和 Windows 安装另有上限。超时不发布完成标记。不能只用单次传输的超时代替总预算，因为重试会重置那次计时。

## 缓存

CI 分开缓存两样东西：Cargo registry，以及按版本和摘要校验过的 `.toolchain/`。不缓存 `target/`。缓存键包含平台、架构、工具版本和供给清单。命中缓存后仍要核对目录里的版本和摘要。
