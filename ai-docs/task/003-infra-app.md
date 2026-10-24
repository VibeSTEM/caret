# 003 — 路径、设置、语言与主题

- 状态：planned
- 依赖：001、002
- 优先级：P0
- 创建 / 更新：2026-10-25 / 2026-10-25

## 目标与背景

窗口和命令行以后都要解析路径、读写设置、切换语言和主题。这些服务放在 `caret-app`，本任务只移植服务本身。还不打开文档，也不创建会话。

## 必读

- [Rust](../standards/rust.md)、[DSL](../standards/dsl.md)、[分层](../standards/layering.md)
- [仓库结构](../architecture/core/repository-layout.md)、[运行时](../architecture/core/runtime.md)

## 范围与非目标

从源应用 crate 移植路径、运行时上下文、设置、语言和主题。六类逻辑路径按 Rust 规范实现：`project`、`user-config`、`app-data`、`cache`、`session`、`qrc`。

`caret-app` 本阶段可以依赖 `caret-foundation` 和 `caret-dsl-core`。不依赖 `caret-engine`、`caret-ffi` 或 Qt。

不包含：`.cdoc`、命令执行、QML 绑定、崩溃之外的界面状态。

## 前置条件与待决策

001、002 已 done。设置 schema 只保留编辑器需要的键。来源里的工艺键不移植。缺键表时，先列一份最小键（语言、主题）写进本任务，再实现。

## 实施步骤

1. 列出要保留的设置键，删掉工艺键。记录来源 commit。
2. 实现路径解析和设置读写。损坏的设置文件隔离，不覆盖用户还没坏的那份。
3. 语言和主题通过 `caret-dsl-core` 读取 `.pa`，不在 `caret-app` 再写一个解析器。
4. 为路径、损坏设置和未知 scheme 补测试。
5. 独立评审后复验。

## 预计改动

新建 `crates/caret-app/`。`.pa` 源可以先放在 `resources/`，生成 QM 留到 004。

## 验收标准

- [ ] 六类 scheme 的解析结果与规范一致。未知 scheme 失败，不落到进程当前目录。
- [ ] 设置读写往返成功。损坏文件被隔离，内存中的上一份有效设置还在。
- [ ] `caret-app` 的依赖符合 L5，且本阶段不依赖尚未创建的 `caret-engine`。
- [ ] 没有工艺领域的设置键。fmt 与 clippy 通过。评审发现已处理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --locked -p caret-app` | 通过 | 未执行 |
| — | 损坏设置夹具 | 隔离且不丢上一份 | 未执行 |

## 风险与回退

来源的路径实现和工程模型缠在一起。只搬路径和设置，工程模型留在来源仓库。回退是移除 `caret-app` 成员。

## 决策与工作记录

- 2026-10-25：创建任务。`caret-engine` 延后到 008，因此本任务的 `caret-app` 先不依赖它。008 再补上这条边。

## 完成摘要

未完成。
