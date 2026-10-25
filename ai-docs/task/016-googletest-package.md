# 016 — 独立 GoogleTest SDK 生产与发布

- 状态：implemented
- 依赖：001
- 优先级：P0
- 创建 / 更新：2026-10-25 / 2026-10-25

## 目标与背景

用户要求先在独立分支准备 GoogleTest package，再让 006 跟进。参考维护者的 SDK 打包仓库 `sdk-packaging` commit `a43519510326d26e3118b5d3f888285d4c38c955`，Caret 在本仓库独立构建、归档和发布 GoogleTest，避免跨组织依赖。生产入口不依赖 006 的其他 workflow、桌面打包或 SDK 消费接口。

## 必读

- [技术基线](../standards/baseline.md)、[依赖获取](../standards/dependency-acquisition.md)
- [持续集成](../standards/ci.md)、[验证与评审](../standards/validation-and-review.md)、[提交规范](../standards/commits.md)

## 范围与非目标

新增 GoogleTest SDK workflow、必要 Rust 工具链与缓存恢复 action、`tools/sdk/` 中仅 GoogleTest 的生产描述和自检，以及 `caret-tests` 的 `sdk-googletest` 命令。不引入桌面包、GoogleTest 消费 helper、领域 SDK 或其他 006 工作流，不修改 006 状态。

## 前置条件与待决策

001 的托管 CMake、Ninja、LLVM 和 Rust runner 已可用。GoogleTest 固定为 `1.18.0`、commit `063de7e9578f82b369302001269680b4b1553359`。`publish` 默认关闭；生产 job 保持只读权限，发布 job 仅在显式启用时获得 `contents: write`。此权限是用户要求独立打包发布流程的任务例外，不扩大普通 CI 权限。

## 实施步骤

1. 从参考仓库移植并裁剪 SDK 工作流，保留固定源码、三平台矩阵、静态库和许可证。
2. 通过托管 LLVM 22.1.7、CMake 和 Ninja 生产 Release C++20 SDK，Windows 全部使用 `/MD` 与 `_ITERATOR_DEBUG_LEVEL=0`。
3. 先归档，再解包到独立目录，用干净 CMake 树编译、链接并运行 CONFIG 消费自检。
4. 校验三平台归档摘要后上传 draft，完整上传后才发布；正式 Release 不覆盖，仅允许摘要一致的幂等重跑。
5. 本地验证、独立评审后记录实际证据；远程三平台和 Release 验收单独记录。

## 预计改动

新增 `.github/workflows/sdk-googletest.yml`、两个 `.github/actions/` 入口、`tools/sdk/`、`tests/src/sdk.rs` 和本任务；修改 runner 分派与任务索引。

## 验收标准

- [x] 独立分支可构建和运行 SDK 生产，不引用尚未落地的 006 入口。
- [x] 锁定源码，产出静态 SDK、元数据、许可证、平铺归档及 SHA256。
- [x] 归档的搬迁副本通过编译、链接和运行自检；错误 triple 或源码 pin 被拒绝。
- [x] 发布默认关闭，完整制品验收后才发布，不破坏正式 Release 的既有资产。
- [x] fmt、Clippy、相关测试和独立评审完成，远程未执行项如实记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-25 | `cargo fmt --all -- --check`、`cargo clippy --locked -p caret-tests --all-targets -- -D warnings`、`cargo test --locked -p caret-tests` | 通过 | 通过；36 passed、3 ignored（基线现有忽略用例），新增 SDK 参数与宿主拒绝用例通过 |
| 2026-10-25 | 在独立 worktree 执行 `cargo run --locked -p caret-tests -- sdk-googletest <锁定源码目录> target/sdk-production/work macos-arm64` | 编译、链接、运行通过 | 通过；归档解包后的独立干净消费者运行 1/1，通过托管 LLVM 22.1.7 生产；SHA256 `57e62c5c9a6980a2e81da1e6e6a45ab19f4659f689ce4d077a36b0759fc742a0` |
| 2026-10-25 | [首次三平台运行](https://github.com/VibeSTEM/caret/actions/runs/40171787099) | 实际远程结果 | Windows 在上游启用 C 时自动选择 GNU 风格 clang.exe，却收到 MSVC 参数，配置失败；未发布。修复为显式固定 C/C++ 编译器，Windows 均用 clang-cl，待复验 |
| 2026-10-25 | subagent `review_006`，`gpt-6.1-sol`、high，独立 worktree 全部 diff 与未跟踪文件 | 无未处理发现 | 通过；确认分支不依赖 006、pin/ABI/搬迁自检和发布保留资产均完整，远程结果仍待运行 |

## 风险与回退

SDK 源码与工具下载需要网络；摘要或 pin 不符必须失败。正式 Release 保持不可变，重新打包产生不同摘要时必须升级版本或使用新 tag。回退只移除本任务的新入口，不修改其他 CI 或既有发布。

## 决策与工作记录

- 2026-10-25：独立 worktree 本地 fmt、Clippy、runner 单元测试和 macOS SDK 生产自检通过。生产与编译输出均位于该 worktree 的 `target/`，只复用既有 `.toolchain/`。远程权限由主代理按当前会话授权执行，本代理未提交或推送。
- 2026-10-25：按用户要求从 006 工作区拆出独立分支 `yuki/googletest-package`，参考 `sdk-packaging a435195`，保留 Caret 自身的托管 LLVM 和 `.toolchain/` 约定。
- 2026-10-25：用户明确授权提交、推送、PR 合并并打包发布 SDK Release。独立评审通过，先标为 implemented，三平台运行与发布有证据后再收尾。
- 2026-10-25：独立 SDK 已由 [PR 6](https://github.com/VibeSTEM/caret/pull/6) 合并。首次远程验收发现 C 编译器驱动器混用，补齐显式 C 编译器配置后继续复验，不绕过失败 job 发布。
- 2026-10-25：首轮 Linux、macOS 的归档搬迁自检已通过，但 artifact 上传同时包含归档和校验文件，`archive: false` 仅支持单文件，上传失败。移除该选项，保留默认 artifact 封装供发布 job 解包收集；不改 SDK 归档本身。

## 完成摘要

未完成。远程三平台和发布仍待验收，不把本机产物当作三平台可分发证明。
