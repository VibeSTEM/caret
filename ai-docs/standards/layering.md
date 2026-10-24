# 分层与依赖方向

更新日期：2026-10-24。这是移植后的目标方向。具体 crate 还没有建立，不能用“当前能链接”代替本表。编辑器模块见[仓库结构](../architecture/core/repository-layout.md)，没有消费者之前不建空 crate。

层号越小越靠下。生产代码只能依赖更小的层，不能依赖同层的另一个成员，也不能依赖更大的层。Rust 的层号和 Native 的层号各算各的。测试可以依赖被测对象，生产代码不能依赖测试。

这些 crate 和 native 目标都还没有建立。图里的箭头是允许的依赖，不是已经写进 manifest 的边。

## 依赖图

```text
caret-dslc ──────────────► caret-dsl-core
caret-layout ────────────► caret-doc
caret-structure ─────────► caret-doc
caret-engine ────────────► caret-doc
            ├────────────► caret-layout
            └────────────► caret-structure
caret-app ───────────────► caret-engine
         ├───────────────► caret-dsl-core
         └───────────────► caret-foundation
caret-ffi ───────────────► caret-app
         └───────────────► caret-foundation
launcher ────────────────► caret-ffi
        ├───────────────► caret-build
        └───────────────► caret-dsl-core

QML ──► ViewModel ──► Qt 适配器
                 └──► caret-ffi（staticlib）
画布 ──► Qt 适配器
native 入口 ──► QML、ViewModel、画布、Qt 适配器、caret-ffi
```

`caret-foundation`、`caret-build`、`caret-dsl-core` 没有指向其他 workspace crate 的边。`caret-doc`、布局、结构索引和会话在需要时可以指向 `caret-foundation`，图里不再重复这几条边。画布和 ViewModel 互不依赖，由 native 入口把它们接在一起。逐帧绘制、命中和输入留在画布里；跨到 Rust 的是命令和页面快照，不是每个指针事件。

## Rust 分级

| 级 | 成员 | 允许依赖 |
| --- | --- | --- |
| L1 基础 | `caret-foundation`、`caret-build`、`caret-dsl-core` | 不依赖其他 workspace crate |
| L2 叶子 | `caret-doc`、`caret-dslc` | `caret-doc` 只可到 `caret-foundation`。`caret-dslc` 只可到 `caret-dsl-core`。二者互不依赖 |
| L3 派生 | `caret-layout`、`caret-structure` | `caret-doc`，以及 `caret-foundation`。二者互不依赖 |
| L4 会话 | `caret-engine` | `caret-doc`、`caret-layout`、`caret-structure`、`caret-foundation` |
| L5 应用 | `caret-app` | `caret-engine`、`caret-dsl-core`、`caret-foundation`。需要文档类型时可以再依赖 `caret-doc` |
| L6 边界 | `caret-ffi` | `caret-app`、`caret-foundation` |
| L7 组合根 | launcher | `caret-ffi`、`caret-build`、`caret-dsl-core` |

`caret-dslc` 是独立的检查和格式化命令，不是桌面程序的组合根。`caret-build` 只供给工具，只有 launcher 和测试可以依赖它。L2 到 L6 不依赖 `caret-build`。

`caret-app` 不依赖 `caret-ffi`。`caret-ffi` 不依赖 `caret-engine` 来绕过 `caret-app`。这样不会形成应用和边界之间的环。

## Native 分级

| 级 | 成员 | 允许依赖 |
| --- | --- | --- |
| N1 适配 | Qt 适配器 | Qt 和系统 API。不依赖 FFI、ViewModel、画布或 QML |
| N2 界面能力 | 画布、ViewModel | 都可以依赖 N1。ViewModel 还可以链接 `caret-ffi` 的 staticlib。画布和 ViewModel 互不链接 |
| N3 界面 | QML 模块 | 只依赖 ViewModel 暴露的类型。不链接 Rust，不链接画布 |
| N4 入口 | native 应用 | N1、画布、ViewModel、QML 模块，以及 `caret-ffi` 的 staticlib |

QML 表达意图。它不解析 `.pa`，不读写文档存储，不创建 Rust 对象。ViewModel 做 Qt 属性和参数转换，不实现文档事务。用户可见的行为由与 Qt 无关的 Rust 服务提供。界面提交意图；以后的命令行适配同一服务。命令 id 的目录写在[命令](../architecture/editor/commands.md)里。

## 边界

- `#[cxx::bridge]` 只声明映射和签名。FFI 实现只转换和转发。
- 小数据用值。大对象用说明了创建、释放、线程和失效行为的 opaque 所有者。整数句柄拒绝释放后复用。
- 可恢复的 C++ 异常在适配器变成结构化错误。panic 不是可恢复错误。
- 手写 unsafe 只留在已登记的崩溃模块。CXX 生成代码的 unsafe 按 [FFI](ffi.md) 审查，不扩散到业务调用方。
- 逻辑路径的宿主是 Qt 适配器；Rust 路径服务不包含 Qt 类型。

新增仓库内依赖时先改上面的图和分级表，再改 manifest 或 `target_link_libraries`。分层检查计划读取 Cargo metadata 和配置后的 CMake 图，对照这两张表。检查还没落地时，评审仍按表执行。
