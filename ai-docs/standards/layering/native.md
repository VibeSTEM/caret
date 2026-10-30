# Native 分级

更新日期：2026-10-30。本文定义 CMake、适配器、ViewModel、画布、QML 和应用入口的依赖边界；全局图见[依赖图](dependency-graphs.md)，跨语言约束见[分层索引](../layering.md)。

## Native 分级

| 级 | 成员 | 允许依赖 |
| --- | --- | --- |
| N1 适配 | Qt 适配器 | Qt 和系统 API。不依赖 FFI、ViewModel、画布或 QML |
| N2 界面能力 | 画布、ViewModel | 都可以依赖 N1。ViewModel 还可以链接 `caret-ffi` 的 staticlib。画布和 ViewModel 互不链接 |
| N3 界面 | QML 模块 | 依赖 ViewModel 暴露的属性和画布公开的声明式绘制项。不链接 Rust；画布不读取文档，文档数据由 ViewModel 提供 |
| N4 入口 | native 应用 | N1、画布、ViewModel、QML 模块，以及 `caret-ffi` 的 staticlib |

当前 N1 适配模块是 `native/adapters/` 的 `caret_adapters`（`caret::adapters`、QML URI `Caret.Adapters`），集中平台字体、路径、窗口装饰与图标适配。`native/app/` 只负责启动装配。N2 的 `native/viewmodels/` 按 Settings、Theme、Document 分类，Preferences 投影不持有 Theme 或 Language 的状态。Settings 总运行时由独立 bridge 持有；设置窗口的 C++ `QStateMachine` 只管理打开、草稿、提交与失败交互，持久化结果以 Rust 返回值为准。

`caret_shell` 可依赖 `caret_adapters`；Qt 为静态 QML 模块生成的 `caret_shellplugin → caret_adaptersplugin` 只负责注册适配模块。插件包装与其所属层一致，不增加领域依赖。分层检查精确登记该生成边，适配模块仍不能依赖 ViewModel、画布、Shell 或 FFI。

QML 表达意图并装配画布公开项。它不解析 `.pa`，不读写文档存储，不创建 Rust 对象。ViewModel 做 Qt 属性和参数转换，不实现文档事务。用户可见的行为由与 Qt 无关的 Rust 服务提供。界面和 `caret-cli` 提交同一种意图。命令 id 的目录写在[命令](../../architecture/editor/commands.md)里。
