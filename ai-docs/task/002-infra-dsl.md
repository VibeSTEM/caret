# 002 — DSL 核心与 dslc

- 状态：done
- 依赖：001
- 优先级：P0
- 创建 / 更新：2026-10-25 / 2026-10-25

## 目标与背景

主题、语言和变量要用 `.pa` 描述。本任务移植 `caret-dsl-core` 和 `caret-dslc`，使单文件检查、格式化和生成可以在命令行跑通。文法、kind 和诊断保持来源行为。改扩展名或关键字不在本次。

## 必读

- [DSL](../standards/dsl.md)、[Rust](../standards/rust.md)、[分层](../standards/layering.md)
- [测试规范](../standards/testing.md)、[提交规范](../standards/commits.md)

## 范围与非目标

交付 `caret-dsl-core`、`caret-dslc`，以及运行检查所需的最小 `.pa` 样例。`caret-dslc` 只依赖 `caret-dsl-core`，不链接 Qt，也不下载 Qt。

不包含：编辑器命令、QML 主题门面、语言包嵌入。主题门面在 004。不移植来源里的工艺领域 `.pa`。

## 前置条件与待决策

001 已 done。无未决文法问题。

## 实施步骤

1. 移植两个 crate 并改名。记录实际来源 commit。
2. 保留来源的解析、诊断、格式化和求值测试，删掉只服务工艺领域的夹具。
3. 跑通 `caret-dslc check`、`caret-dslc format`、`caret-dslc format --check`。
4. 确认分层：`caret-dslc` 不依赖 `caret-doc` 或 `caret-app`。
5. 独立评审后复验。

## 预计改动

新建 `crates/caret-dsl-core/`、`crates/caret-dslc/`。workspace 成员增加这两个 crate。样例放在 crate 测试或 `tests/fixtures/`。

## 验收标准

- [x] `caret-dslc check` 对一份合法 `.pa` 成功，对一份故意写错的 `.pa` 失败并给出诊断。
- [x] `format --check` 能区分已格式化和未格式化的输入。
- [x] crate 不依赖 Qt、FFI 和编辑器 crate。
- [x] fmt 与 clippy `-D warnings` 通过。评审发现已处理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-25 | macOS，`cargo test --workspace --locked --offline` | 通过 | 通过。dsl-core 库 30 项、catalog 7 项、features 31 项、fsm 21 项；dslc 单测 14 项、cli 2 项 |
| 2026-10-25 | 同上，`check` 与 `format --check` 由 `caret-dslc` 的 cli 测试覆盖 | 合法通过，非法失败 | 通过 |

## 风险与回退

来源测试夹具夹带工艺语义时，改成中性样例，不把那些语义带进仓库。回退是撤掉这两个 crate 的成员登记。

## 决策与工作记录

- 2026-10-25：创建任务。
- 2026-10-25：从 panta `ee84d95` 整份复制 `caret-dsl-core` 和 `caret-dslc`。fsm 的声明、校验、格式化和 Rust 元数据生成在 `caret-dsl-core`。来源里打开 STL 的消费者状态机和对应 `.pa` 没有复制。
- 2026-10-25：测试夹具 `open-saved-stl` 改成中性样例 `open-saved-note`。状态、转移和 guard 的形状保留，用来覆盖语言，不代表产品流程。`cargo test --offline -p caret-dsl-core --test fsm` 21 项通过。
- 2026-10-25：独立评审没有验收缺陷。打开 STL 的消费者不在仓库里。

## 完成摘要

`caret-dsl-core` 与 `caret-dslc` 可以检查、格式化并生成，包含状态机设计。不链接 Qt。
