# 014 — 应用命令行

- 状态：ready
- 依赖：008
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-30

## 目标与背景

桌面和命令行要提交同一套命令。本任务加上 `caret-cli`：它把参数适配成 `caret-app` 的调用，打开 `.cdoc`、执行已有命令、再保存。它不启动窗口。

009 以后的命令出现时，命令行自动走同一张注册表，不必为每个里程碑再写一个进程。

## 必读

- [分层](../standards/layering.md)、[命令](../architecture/editor/commands.md)、[运行时](../architecture/core/runtime.md)
- [Cargo](../standards/cargo.md)、[Rust](../standards/rust.md)

## 范围与非目标

创建 `caret-cli`。允许依赖 `caret-app` 和 `caret-foundation`。不依赖 `caret-ffi`、`caret-engine`、`caret-build` 或 Qt。

第一版命令行至少能：显示帮助、打开一份 `.cdoc`、插入一段文字、保存到原路径或新路径。失败时进程以非零退出，并且不把半份文件留成最终文件名。

裸 `cargo run` 仍启动 launcher。命令行用 `cargo run -p caret-cli`。

不包含：交互式全屏界面、来源仓库里的网格或 STL 子命令、复刻全部快捷键。

## 前置条件与待决策

008 已 done，`file.save` 和文本插入已经存在。不依赖 007 的像素页。

## 实施步骤

1. 按分层新增 crate。参数解析只放在 `caret-cli`，文档修改调用 `caret-app`。
2. 帮助文本列出当前注册表里可用的命令，不手写第二份行为说明。
3. 进程测试覆盖成功保存、拒绝更高版本、保存失败时的退出码。
4. 确认默认成员仍是 launcher。
5. 独立评审后复验。

## 预计改动

新建 `crates/caret-cli/`。workspace 增加成员，不把它设为默认成员。

## 验收标准

- [ ] `caret-cli` 的依赖只有 `caret-app` 和 `caret-foundation`。
- [ ] 打开、插入、保存后再由桌面或测试读回，文字一致。
- [ ] 失败为非零退出，且不替换用户原来的文件。
- [ ] `cargo run` 不进入命令行。fmt、clippy 和进程测试通过。评审发现已处理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --locked -p caret-cli` | 通过 | 未执行 |
| — | 手工跑一次打开、插入、保存 | 文件被更新 | 未执行 |

## 风险与回退

若 `caret-app` 的公开接口只有给 FFI 用的形状，先补一个不依赖 Qt 的函数，不让命令行反过来依赖 `caret-ffi`。回退是去掉该 crate 的成员登记。

## 决策与工作记录

- 2026-10-30：008 已完成且无其他未满足的开始条件，状态从 planned 调整为 ready；与任务索引同步。

- 2026-10-25：创建任务。与 M1 之后的里程碑并行，不阻塞它们。

## 完成摘要

未完成。
