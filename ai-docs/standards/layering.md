# 分层与依赖方向

更新日期：2026-10-24。这是移植后的目标方向。具体 crate 还没有建立，不能用“当前能链接”代替本表。编辑器的文档和排版模型由架构文档规定，不在这里预建空 crate。

```text
QML 原子组件 / 组合组件 / 对话框
        ↓ 意图和属性
C++ ViewModel / 窗口 / 输入
        ↓ CXX 签名
Rust 应用服务 / 领域模型 / DSL 快照
```

画布上的逐帧绘制、命中和输入留在 C++。数据按快照跨边界，不在每一帧往返 Rust。

## Rust

依赖只能指向更低的职责。测试 crate 可以依赖被测 crate，生产 crate 不能依赖测试。

| 职责 | 规划成员 | 允许依赖 |
| --- | --- | --- |
| 基础 | `caret-foundation`、`caret-build`、`caret-dsl-core` | 不依赖其他 workspace crate |
| 工具入口 | `caret-dslc` | `caret-dsl-core` |
| 应用服务 | 持有路径、运行时、设置、语言和主题的服务 crate | 基础库和 DSL 核心。不依赖 Qt、CXX 或 FFI |
| 语言边界 | `caret-ffi` | 应用服务和 foundation。只做 DTO、错误和句柄 |
| 组合根 | launcher | FFI、构建供给，以及启动所需的 DSL 聚合 |

`caret-dslc` 是独立命令行，不是桌面程序的组合根。禁止应用服务依赖 FFI，再由 FFI 依赖应用服务形成环。

## Native 与 QML

| 职责 | 允许的依赖 |
| --- | --- |
| Qt 适配器 | Qt 和系统 API。不依赖 FFI、界面桥或画布 |
| ViewModel 与界面桥 | Qt 适配器，以及 Rust FFI staticlib |
| QML 模块 | 只通过 ViewModel 使用服务 |
| 应用入口 | 适配器、QML 模块和 FFI |

QML 表达意图。它不解析 `.pa`，不读写文档存储，不创建 Rust 对象。ViewModel 做 Qt 属性和参数转换，不实现文档事务。

用户可见的行为由与 Qt 无关的 Rust 服务提供。界面提交意图；以后的命令行适配同一服务，不复制规则。命令 id 的目录写在架构文档里。

## 边界

- `#[cxx::bridge]` 只声明映射和签名。FFI 实现只转换和转发。
- 小数据用值。大对象用说明了创建、释放、线程和失效行为的 opaque 所有者。整数句柄拒绝释放后复用。
- 可恢复的 C++ 异常在适配器变成结构化错误。panic 不是可恢复错误。
- 手写 unsafe 只留在已登记的崩溃模块。CXX 生成代码的 unsafe 按 [FFI](ffi.md) 审查，不扩散到业务调用方。
- 逻辑路径的宿主是 Qt 适配器；Rust 路径服务不包含 Qt 类型。

新增仓库内依赖时先改本表，再改 manifest 或 `target_link_libraries`。分层检查计划读取 Cargo metadata 和配置后的 CMake 图；检查还没落地时，评审仍按本表执行。
