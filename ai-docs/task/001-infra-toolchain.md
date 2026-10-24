# 001 — 工具链、workspace 与基础 crate

- 状态：done
- 依赖：无
- 优先级：P0
- 创建 / 更新：2026-10-25 / 2026-10-25

## 目标与背景

仓库还没有 Cargo 工程。本任务建立可编译的 workspace，并把工具下载到 `.toolchain/`。完成后，`caret-build` 能把锁定版本的 CMake、Ninja、LLVM 装进仓库根的 `.toolchain/`，`cargo clean` 不清这棵目录。`caret-foundation` 能单独编译。

来源是任务索引里的 panta 提交。移植时改 `panta` 前缀为 `caret`，并改掉来源里把工具装进 `target/` 的路径。

## 必读

- [技术基线](../standards/baseline.md)、[依赖获取](../standards/dependency-acquisition.md)、[Rust](../standards/rust.md)、[Cargo](../standards/cargo.md)
- [分层](../standards/layering.md)、[仓库结构](../architecture/core/repository-layout.md)
- [提交规范](../standards/commits.md)、[测试规范](../standards/testing.md)

## 范围与非目标

本次交付：

- 根 `Cargo.toml`、`rust-toolchain.toml`、`.cargo/config.toml`。默认成员先只包含本任务创建的 crate；launcher 还没有时，不把裸 `cargo run` 写成已经能开窗口。
- `caret-build`、`caret-foundation`、`caret-tests` 的调度入口。
- 工具安装到 `.toolchain/tools/`。Rust 编译器仍由 rustup 管理。

不包含：DSL、应用服务、QML、CMake 工程、FFI、CI workflow、编辑器 crate。不移植几何、网格和求解相关 crate。

## 前置条件与待决策

本地有 git 和 rustup。首次安装工具需要网络。无未决产品问题。

## 实施步骤

1. 对照来源提交记录实际复制的 commit。按[依赖获取](../standards/dependency-acquisition.md)改安装根。
2. 建立 workspace 和上述三个 crate。`caret-build` 只保留工具供给、安装事务和 `.toolchain/` 定位。
3. 用一个锁定工具做往返：缺包失败、摘要不符失败、成功后 `cargo clean` 仍能找到已安装的二进制。
4. `cargo fmt` 与 `cargo clippy --locked --workspace --all-targets -- -D warnings`。
5. 独立评审后复验，把结果记在本文件。

## 预计改动

新建根 Cargo 清单、工具链文件，以及 `crates/caret-build/`、`crates/caret-foundation/`、`tests/`。`.toolchain/` 不提交。

## 验收标准

- [x] 工具装在仓库根 `.toolchain/`，不在 `target/`。未设置 `CARET_TOOLCHAIN_ROOT` 时不回退到 `target/`。
- [x] `cargo clean` 之后已安装的 CMake 或 Ninja 仍可调用。
- [x] `caret-foundation` 不依赖其他 workspace crate，也不依赖 Qt。
- [x] 工作区里没有编辑器 crate，也没有几何、网格、VTK 相关 crate。
- [x] fmt 和 clippy `-D warnings` 通过。独立评审的发现已处理，范围和限制有记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-25 | macOS，`cargo fmt --all -- --check` | 无差异 | 通过 |
| 2026-10-25 | macOS，`cargo clippy --locked --workspace --all-targets -- -D warnings` | 无警告 | 通过 |
| 2026-10-25 | macOS，安装锁定 CMake 4.4.3 后 `cargo clean`，再执行该二进制 `--version` | 工具仍在 `.toolchain/` | `cmake version 4.4.3`。路径在 `.toolchain/tools/cmake/`，不在 `target/` |
| 2026-10-25 | macOS，`cargo test --workspace --locked --offline` | 通过 | 通过。caret-build 23 项，caret-foundation 4 项加 1 项忽略，caret-tests 34 项加 3 项忽略 |

## 风险与回退

下载地址或摘要失效时，安装必须失败并留下明确诊断，不用本机同名工具冒充。回退是删掉对应 `.toolchain` 子目录后按锁定清单重装。

## 决策与工作记录

- 2026-10-25：创建任务。来源提交以索引为准。
- 2026-10-25：从 panta `ee84d95` 整份复制 `caret-build`、`caret-foundation` 和 `caret-tests` 调度入口，再改名并把安装根改到仓库根 `.toolchain/`。`CARET_TOOLCHAIN_ROOT` 未设置时不读 `target/`。`cc` 锁在 `1.4.6`，与来源一致。同一工作区里还放了 002 的 DSL 和 003 的应用服务，见对应任务。
- 2026-10-25：独立评审没有验收缺陷。成功发布后删除安装归档，与来源一致，规范改成不靠常驻压缩包离线重装。`paths` 注释和 `deny.toml` 里过时的 syn 理由已改正。`cargo quality`、`cargo lint`、`cargo format` 仍会找尚未存在的 launcher、Qt 和 native 工程，那是 004–006 的范围。

## 完成摘要

工具链、workspace、`caret-build`、`caret-foundation` 和测试调度已落地。编辑器 crate 和 Qt 工程不在本次。
