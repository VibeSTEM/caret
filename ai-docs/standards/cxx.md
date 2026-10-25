# CXX

更新日期：2026-10-25。锁定版本见[技术基线](baseline.md)。生成器必须和 Rust 依赖是同一次 release。官方说明见 [CXX](https://cxx.rs/concepts.html)。

## 位置

```text
QML → QObject ViewModel → CXX → Rust 应用服务
```

`#[cxx::bridge]` 里只有类型和签名。模块外的代码做转换和转发。先接小而稳定的服务，不把 `QObject` 继承体系暴露给 Rust，也不为逐帧绘制跨语言。

## 类型与错误

- 小 DTO 用共享结构体。Rust 服务用 opaque 类型和 `Box`。需要 C++ 所有权时用 `UniquePtr`。
- Qt 字符串和容器在适配器里转成 CXX 支持的类型。
- 借用的 slice 只在声明的有效期内使用。后台任务不保存调用结束后会失效的缓冲区。
- 可恢复错误使用显式 `Result` 或错误 DTO。未声明 Result 的 C++ 异常会导致终止。Rust panic 会 abort，即使签名返回 Result 也一样。
- 异常不越过 Qt 事件回调。适配器先接住，再变成应用错误。

## 构建

1. Cargo 产出供 native 链接的 staticlib。这个库不反向触发完整桌面构建。
2. 锁定版本的 cxxbridge 生成头和 C++ 源文件，CMake 编译胶水并链接 Rust 库。
3. 外层 Cargo 调度 native 构建，launcher 启动程序。

不使用机器上任意版本的全局 cxxbridge。cxx-build 如果启用，不能和另一条路径重复编译同一份胶水。链接顺序要在干净构建和增量构建里都验证。

最低验收：C++ 调用 Rust、Rust 调用 C++，覆盖共享 DTO、所有权和显式错误；非 ASCII 文本、空 slice、重复创建和销毁、非法输入各有证据。
