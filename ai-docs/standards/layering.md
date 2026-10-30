# 分层与依赖方向

更新日期：2026-10-30。本文是跨语言依赖规则的入口索引。新增仓库内依赖时先更新[依赖图](layering/dependency-graphs.md)，再对照[Rust crate 分级](layering/rust.md)或[Native 分级](layering/native.md)确认允许边，最后修改 manifest 或 CMake 配置。

层号越小越靠下；生产代码只能依赖更低层，不能依赖同层成员或更高层。Rust 与 Native 层号各自计算。测试可依赖被测对象，生产代码不能依赖测试。图中的允许边以实际 Cargo manifest 和配置后的 CMake 图校验。

| 主题 | 权威文档 |
| --- | --- |
| Rust crate 依赖和职责 | [Rust crate 分级](layering/rust.md) |
| CMake、ViewModel、画布与 QML 边界 | [Native 分级](layering/native.md) |
| Rust 与 native 总体依赖图 | [依赖图](layering/dependency-graphs.md) |
| FFI、路径与设置权威边界 | 本页下方 |

## 边界

- `#[cxx::bridge]` 只声明映射和签名。FFI 实现只转换和转发。
- 小数据用值。大对象用说明了创建、释放、线程和失效行为的 opaque 所有者。整数句柄拒绝释放后复用。
- 可恢复的 C++ 异常在适配器变成结构化错误。panic 不是可恢复错误。
- 手写 unsafe 只留在已登记的崩溃模块。CXX 生成代码的 unsafe 按 [FFI](ffi.md) 审查，不扩散到业务调用方。
- 逻辑路径的宿主是 Qt 适配器；Rust 路径服务不包含 Qt 类型。
- 用户设置和 Preferences 的权威状态、校验与持久化由 `caret-app` 的 Rust 服务负责。QML 只展示和提交意图，ViewModel 只做类型转换与信号桥接；C++ 组件只接收当前操作所需的值，不保存持久化副本。

新增仓库内依赖时先更新[依赖图](layering/dependency-graphs.md)和对应的[Rust](layering/rust.md)或[Native](layering/native.md)分级表，再改 manifest 或 `target_link_libraries`。`cargo lint` 的 layering 检查读取 Cargo metadata，并配置专用 CMake 夹具核对 native 依赖边；评审以对应专题的分级表为准。
