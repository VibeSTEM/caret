# Rust crate 分级

更新日期：2026-10-30。本文定义 Rust workspace crate 的分级、允许依赖和职责边界；全局图见[依赖图](dependency-graphs.md)，跨语言约束见[分层索引](../layering.md)。

## Rust 分级

| 级 | 成员 | 允许依赖 |
| --- | --- | --- |
| L1 基础 | `caret-foundation`、`caret-build`、`caret-dsl-core` | 不依赖其他 workspace crate |
| L2 叶子 | `caret-doc`、`caret-dslc` | `caret-doc` 只可到 `caret-foundation`。`caret-dslc` 只可到 `caret-dsl-core`。二者互不依赖 |
| L3 派生 | `caret-layout`、`caret-structure` | `caret-doc`，以及 `caret-foundation`。二者互不依赖 |
| L4 会话 | `caret-engine` | `caret-doc`、`caret-layout`、`caret-structure`、`caret-foundation` |
| L5 应用 | `caret-app` | `caret-engine`、`caret-doc`、`caret-dsl-core`、`caret-foundation` |
| L6 边界 | `caret-ffi` | `caret-app`、`caret-foundation` |
| L7 组合根 | launcher、`caret-cli` | launcher 可到 `caret-ffi`、`caret-build`、`caret-dsl-core`。`caret-cli` 可到 `caret-app`、`caret-foundation`。二者互不依赖 |

`caret-dslc` 是独立的 `.pa` 检查和格式化命令，不是应用命令行，也不是桌面程序的组合根。`caret-build` 只供给工具，只有 launcher 和测试可以依赖它。L2 到 L6 和 `caret-cli` 不依赖 `caret-build`。`caret-ffi` 的 build script 可以依赖 `caret-build`，以便 CXX 与 native 使用同一份托管 Clang。运行时依赖 `caret-app`，由应用服务生成并传递带代际的只读布局快照；文档模型、命令事务和排版仍留在 Rust 领域层。

`caret-cli` 把参数适配成 `caret-app` 上的同一批命令。它不依赖 `caret-ffi`、`caret-engine` 或 Qt。`caret-app` 不依赖 `caret-ffi`。`caret-ffi` 不依赖 `caret-engine` 来绕过 `caret-app`。这样不会形成应用和边界之间的环，命令行也不会绕过应用服务。
