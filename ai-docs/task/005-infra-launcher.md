# 005 — launcher、CMake 与 FFI 骨架

- 状态：planned
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

- `crates/launcher`，`build.rs` 只用标准库，按 profile 调用 CMake。
- `caret-ffi` 只导出证明链接用的最小接口，例如版本字符串。不出现文档、命令或页面快照类型。
- native 入口加载 `Caret.Shell`，显示一个空白窗口。关闭窗口后进程退出。
- workspace 默认成员改为 launcher。裸 `cargo run` 打开这个窗口。

不包含：画布、ViewModel 上的文档状态、设置对话框的业务保存。那些在 007 和 008。

## 前置条件与待决策

003、004 已 done。Qt 已由 `caret-build` 装到 `.toolchain/`。

## 实施步骤

1. 移植 launcher 和 CMake 骨架，改名前缀。记录来源 commit。
2. 写最小 `#[cxx::bridge]`。实现只转发，不放业务。
3. 空白窗口加载 QML 模块。确认没有把来源的工艺页登记进工程。
4. 跑通配置、编译、启动、关闭。Debug 和 Release 都用 `/MD` 的平台按规范检查运行库。
5. 独立评审后复验。

## 预计改动

新建 `crates/launcher/`、`crates/caret-ffi/`、`native/`、`qml/App.qml` 的空白页。CMake 不回调 Cargo。

## 验收标准

- [ ] `cargo run` 打开空白窗口，关闭后进程退出，不残留子构建进程。
- [ ] `caret-ffi` 不依赖 `caret-engine`。launcher 的依赖停在 `caret-ffi`、`caret-build`、`caret-dsl-core`。
- [ ] CMake 消费已经生成的 staticlib，不从 `build.rs` 再启动 Cargo。
- [ ] 窗口里看不到工艺面板。fmt、clippy 和本次 native 构建通过。评审发现已处理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo run` | 空白窗口可关闭 | 未执行 |
| — | 检查 CMake 与 FFI 的依赖边 | 符合分层 | 未执行 |

## 风险与回退

Qt 与 CXX 的运行库不一致时，链接会在启动时失败。失败就停在本任务，不把库路径硬编码到用户机器。回退是恢复默认成员并去掉 native 目标。

## 决策与工作记录

- 2026-10-25：创建任务。编辑器 DTO 不在这层骨架里预留。

## 完成摘要

未完成。
