# Rust crate 分级

更新日期：2026-10-30。本文定义 Rust workspace crate 的分级、允许依赖和职责边界；全局图见[依赖图](dependency-graphs.md)，跨语言约束见[分层索引](../layering.md)。

## Rust 分级

| 级 | 成员 | 允许依赖 |
| --- | --- | --- |
| L1 基础 | `caret-foundation`、`caret-build`、`caret-dsl-core` | 不依赖其他 workspace crate |
| L2 叶子 | `caret-code`、`caret-doc`、`caret-dslc` | `caret-code` 无 workspace 依赖；`caret-doc` 只可到 `caret-foundation`。`caret-dslc` 只可到 `caret-dsl-core`。三者互不依赖 |
| L3 派生 | `caret-layout`、`caret-structure`、`caret-math` | `caret-layout` 可依赖 `caret-math` 的稳定字体角色接口；其余依赖为 `caret-code`、`caret-doc` 和 `caret-foundation`。`caret-structure` 与其他 L3 crate 独立 |
| L4 会话 | `caret-engine` | `caret-doc`、`caret-layout`、`caret-structure`、`caret-math`、`caret-foundation` |
| L5 应用 | `caret-app` | `caret-code`、`caret-engine`、`caret-doc`、`caret-dsl-core`、`caret-foundation` |
| L6 边界 | `caret-ffi` | `caret-app`、`caret-foundation` |
| L7 组合根 | launcher、`caret-cli` | launcher 可到 `caret-ffi`、`caret-build`、`caret-dsl-core`。`caret-cli` 可到 `caret-app`、`caret-foundation`。二者互不依赖 |

`caret-dslc` 是独立的 `.pa` 检查和格式化命令，不是应用命令行，也不是桌面程序的组合根。`caret-build` 只供给工具，只有 launcher 和测试可以依赖它。L2 到 L6 和 `caret-cli` 不依赖 `caret-build`。`caret-ffi` 的 build script 可以依赖 `caret-build`，以便 CXX 与 native 使用同一份托管 Clang。运行时依赖 `caret-app`，由应用服务生成并传递带代际的只读布局快照；文档模型、命令事务和排版仍留在 Rust 领域层。

`caret-code` 只拥有代码文本领域类型、代码字体数据以及后续代码识别/高亮语义，不依赖 Parley、布局、编辑会话或 Qt。`caret-layout` 可消费其静态字体面数据并统一生成代码字体候选；`caret-app` 将其内置字体族名投影给 Qt 命令输入。Qt 仍从同一仓库资源目录加载字体文件，不反向依赖 Rust crate。

`caret-layout` 到 `caret-math` 的依赖仅用于读取数学角色的稳定默认族名；STIX 字节和 MATH 度量继续由 `caret-math` 独占。这样避免布局层复制字体族字符串，同时让 `font_selection.rs` 保持 Math/Code 候选策略的唯一入口。代价是单独构建 `caret-layout` 时也会构建 `caret-math` 及其 OpenType 解析依赖；目前接受此构建成本，以保留字体数据与族名的单一所有者。

`caret-cli` 把参数适配成 `caret-app` 上的同一批命令。它不依赖 `caret-ffi`、`caret-engine` 或 Qt。`caret-app` 不依赖 `caret-ffi`。`caret-ffi` 不依赖 `caret-engine` 来绕过 `caret-app`。这样不会形成应用和边界之间的环，命令行也不会绕过应用服务。
