# Rust crate 分级

更新日期：2026-11-06。本文定义 Rust workspace crate 的分级、允许依赖和职责边界；全局图见[依赖图](dependency-graphs.md)，跨语言约束见[分层索引](../layering.md)。

## Rust 分级

下面列出每个 workspace crate 的允许依赖；`{}` 表示不依赖其他 workspace crate。依赖只能指向更低层，L3 内没有依赖边。

```text
L1 基础
├── caret-foundation: {}
├── caret-build: {}
├── caret-dsl-core: {}
└── caret-font-roles: {}

L2 叶子
├── caret-doc: {caret-foundation}
├── caret-dslc: {caret-dsl-core}
└── caret-grid-layout: {}

L3 派生
├── caret-layout: {caret-doc; caret-font-roles; caret-foundation}
├── caret-structure: {caret-doc; caret-foundation}
└── caret-math: {caret-doc; caret-font-roles; caret-grid-layout}

L4 会话
└── caret-engine: {caret-doc; caret-layout; caret-structure; caret-math; caret-foundation; caret-font-roles}

L5 应用
└── caret-app: {caret-engine; caret-doc; caret-dsl-core; caret-foundation; caret-font-roles}

L6 边界
└── caret-ffi: {caret-app; caret-foundation; caret-build (仅 build script)}

L7 组合根
├── caret-launcher: {caret-ffi; caret-build; caret-dsl-core}
└── caret-cli: {caret-app; caret-foundation}
```

`caret-dslc` 是独立的 `.pa` 检查和格式化命令，不是应用命令行，也不是桌面程序的组合根。`caret-build` 只供给工具；launcher 和测试入口可调用它，`caret-ffi` 仅 build script 可依赖它，以便 CXX 与 native 使用同一份托管 Clang。应用运行时依赖 `caret-app`，由应用服务生成并传递带代际的只读布局快照；文档模型、命令事务和排版仍留在 Rust 领域层。

`caret-font-roles` 是无 workspace 依赖的 L1 crate，统一拥有 Body、Math、Code 语义角色、当前默认族名和仓库静态字体字节。`caret-layout`、`caret-math`、`caret-engine` 与 `caret-app` 需要字体数据时直接向下依赖它；用户偏好、脚本/语言覆盖、候选顺序和 Fontique 查询仍由 `caret-layout` 管理，OpenType MATH 度量仍由 `caret-math` 管理。Qt 继续从同一 `resources/fonts/` 文件加载字体。

未来代码环境领域 crate `caret-code` 在 050/051 出现独立、可复用的代码语义 API 时再创建，并与 `caret-math` 同属 L3。代码领域 crate 不承载纯静态字体资源；布局不得依赖同级代码领域 crate，跨领域协调由更高层完成。当前不创建只有转发或资源的空壳 crate。

`caret-grid-layout` 只接收调用方测量好的单元格尺寸与已决列宽，计算列宽、基线行高、单元格位置和网格线坐标。它不负责文本换行、文档表格列宽求解、列表编号/缩进或分页。当前生产消费者是 `caret-math` 的 Typst 数学表格适配器；文档表格及列表在 046、051 接入前仍由各自任务定义布局策略。

当 046 或 051 实际接入该几何 crate 时，`caret-layout` 增加到 `caret-grid-layout` 的边需同步登记到 Cargo、依赖图和架构 lint；目前尚无该依赖。


`caret-cli` 把参数适配成 `caret-app` 上的同一批命令。它不依赖 `caret-ffi`、`caret-engine` 或 Qt。`caret-app` 不依赖 `caret-ffi`。`caret-ffi` 不依赖 `caret-engine` 来绕过 `caret-app`。这样不会形成应用和边界之间的环，命令行也不会绕过应用服务。
