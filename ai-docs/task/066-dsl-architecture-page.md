# 066 — `.pa` DSL 架构页与进程崩溃处理器接入

- 状态：done
- 依赖：无
- 优先级：P2
- 创建 / 更新：2026-11-06 / 2026-11-06

## 目标与背景

落实 [issue #76](https://github.com/VibeSTEM/caret/issues/76) 架构审查提出的文档缺口：为 `caret-dsl-core` 增加独立架构页，集中说明 `.pa` DSL 的用途、实际使用方和稳定性边界，并链接到现有 DSL 规范；同时把 `caret-foundation` 的进程崩溃处理器接入 native GUI 进程启动。

issue 的其他反馈是后续设计约束或条件触发建议（编辑 crate 边界、结构索引职责和 CXX/ViewModel 责任）；本 task 不据此拆 crate 或改变运行时。

## 必读

- [架构核心索引](../architecture/core/README.md)
- [`.pa` DSL 规范](../standards/dsl.md)
- [技术选择](../architecture/core/tech-stack.md)
- [验证与评审](../standards/validation-and-review.md)

## 范围与非目标

- 新增 `ai-docs/architecture/core/dsl.md`，写清 DSL 职责、构建期与运行期边界、各 kind 的使用方和兼容承诺。
- 从核心架构索引链接该页，并明确架构页与语法规范的分工。
- 在运行时架构页说明 crash handler 的启动位置、日志范围及与 Qt 日志的边界。
- 将 `caret-foundation` 的进程崩溃处理器经 `caret-ffi` 接入 native GUI 进程的早期启动阶段；安装失败输出诊断但继续启动。
- 明确该处理器只记录进程级崩溃，不替代 Qt `qWarning` / `qCritical` 消息处理。
- 非目标：修改 DSL 实现、格式、构建产物、编辑会话状态或 issue 中其他架构建议。

## 实施步骤

1. 对照 DSL 规范和实际 workspace 消费者核实描述。
2. 新增架构页并更新核心架构索引。
3. 校验页面链接、消费者表与现有规范一致。
4. 按验证与评审标准完成领域检查和独立 final review，记录发现与修复。

## 预计改动

- `ai-docs/architecture/core/dsl.md`（新增）
- `ai-docs/architecture/core/README.md`
- `ai-docs/architecture/core/runtime.md`
- `Cargo.toml`、`Cargo.lock` 与 `crates/caret-ffi/Cargo.toml`
- `crates/caret-ffi/src/lib.rs`
- `native/app/main.cpp`
- `tests/src/layering.rs`
- `ai-docs/task/066-dsl-architecture-page.md`（本 task）
- `ai-docs/task-index.md`

## 验收标准

- [X] 架构页说明 `.pa` DSL 服务的用途、输入与运行期边界。
- [X] `language`、`theme` / `variables`、`fsm` 与 `caret-dslc` 的消费者及职责准确。
- [X] 文档区分源格式输入契约、未发布的 Rust 内部 API 和派生构建产物。
- [X] 核心架构索引及相关规范的链接有效、信息不重复冲突。
- [X] 运行时文档准确说明进程崩溃处理器的启动位置、输出和 Qt 日志边界。
- [x] crash handler 在 GUI 子进程中、Qt 初始化前安装；安装失败可见且不阻止启动。
- [x] 分层期望与 Cargo 依赖同步，并由 build/lint 检查覆盖集成结果。
- [x] 领域检查与独立 final review 已完成，发现的问题已修复并复验。

## 验证计划与结果

| 日期       | 环境 / 命令                                                                                                                                                  | 预期                                                                                  | 实际结果                                                                                                  |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| 2026-11-06 | 手工交叉核对 `ai-docs/standards/dsl.md`、`caret-dsl-core`、`caret-dslc`、`caret-app`、`crates/launcher/build.rs` 与 `native/i18n/CMakeLists.txt` | 架构说明与现状一致                                                                    | 通过；确认主题由 Rust 初始化时求值、FSM 暂无生产消费者、TS/QM 各自构建入口准确                            |
| 2026-11-06 | Markdown 相对链接检查（5 个相关文档）；`git diff --check`                                                                                                  | 本地链接目标存在且无空白错误                                                          | 通过；5 个文档的本地链接均有效，diff 无空白错误                                                           |
| 2026-11-06 | `gpt-6.1-terra` medium 只读领域检查及修正后复查                                                                                                            | DSL 边界和消费者描述准确；审查建议落实                                                | 通过；初次检查 4 项描述问题均已修正，复查未发现遗留问题                                                   |
| 2026-11-06 | `gpt-6-sol` medium 独立 final review（完整 5 文件 diff）                                                                                                   | 与 issue、规范和实际消费者一致；无剩余问题                                            | 通过；review 对照 issue、DSL 规范、launcher/CMake 构建、ThemeService、FSM API 及 task 040，未发现遗留问题 |
| 2026-11-06 | `~/eit/panta` 接入方式对照                                                                                                                                 | FFI 返回 `Result<String>`，native main 在 Qt 前安装；错误直接写 UTF-8 stderr 并继续 | 通过；对照 panta `crates/panta-ffi/src/support.rs`、bridge 和 `native/app/main.cpp`                   |
| 2026-11-06 | `cargo format --check` 与 `cargo lint --check`                                                                                                            | Rust/C++ 格式、Clippy、CMake、分层、qmllint、clang-tidy 等检查通过                    | 通过；完整 lint 9/9 阶段通过                                                                             |
| 2026-11-06 | `cargo build --locked -p caret-launcher`                                                                                                                   | 编译 CXX bridge、native main 与 launcher                                              | 通过                                                                                                      |
| 2026-11-06 | Terra medium FFI/启动生命周期领域检查                                                                                                                      | 安装调用在正确进程和启动阶段；错误传播、依赖方向准确                                 | 通过；按只读代码检查确认与 Panta 模式一致；未触发真实崩溃场景                                            |
| 2026-11-06 | 运行时崩溃/安装失败行为 | 实际触发处理器并观察 stderr、日志文件和退出语义 | 未执行；本次完成构建、静态评审与 lint，未启动受控崩溃进程、模拟不可写临时目录或在 Windows 构建运行 |
| 2026-11-06 | Sol medium 独立 final review（完整 11 文件 diff）                                                                                                             | 完整 diff 无遗留问题                                                                  | 通过；确认 Panta 用法、FFI 错误传播、native 启动时机、分层与文档一致，无剩余发现                          |

## 风险与回退

崩溃处理器安装失败不会阻断启动，但没有可用 stderr 时用户可能看不到安装诊断；崩溃日志仍需依赖系统临时目录可写。它记录进程崩溃，不会采集 Qt 常规日志。恢复时移除 FFI 转发和 native 启动调用，并同步删掉 foundation 直接依赖及分层期望；DSL 架构页可以独立保留。

## 决策与工作记录

- 2026-11-06：根据 issue #76 的 DSL 文档建议及 crash handler 延后接入建议建立 task；对编辑 crate 边界、结构索引和 CXX/ViewModel 反馈保留为后续设计约束。
- 2026-11-06：维护者确认接入并要求沿用`panta` 用法。照其模式通过 `caret-ffi` 暴露 `Result<String>`，在 native `main` 最早期安装；失败捕获 `rust::Error` 后写 UTF-8 `stderr` 并继续启动。此处理器记录进程崩溃，不代替 Qt 消息处理器。
- 对照来源：`~/eit/panta`，HEAD `a101c08c79b8ec93e302aa42a4b6ee138807ca8a`；本任务改动遵循其 FFI 签名、调用时机与失败处理模式。

## 完成摘要

新增 `.pa` DSL 架构页并接入核心架构索引；`caret-foundation` crash handler 已按 Panta 模式通过 FFI 在 native GUI 进程早期安装。构建、格式、lint、分层检查及 Terra/Sol 独立审查通过。未触发真实崩溃或安装失败场景，也未在 Windows 构建运行。
