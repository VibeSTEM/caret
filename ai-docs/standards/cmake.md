# CMake target 与依赖规范

更新日期：2026-10-24。CMake 由 Cargo 供给并调度，版本见[技术基线](baseline.md)。适用于规划中的 `native/` 和其中的 QML 模块。

## 项目规则

- 在源码树外构建。标准、include、宏、链接和告警绑定到 target。不为一个适配器改整个工程的搜索路径。
- 自有目标使用 C++20，`CXX_STANDARD_REQUIRED ON`，`CXX_EXTENSIONS OFF`。公开头需要 C++20 时向消费者传播。
- 依赖使用 imported target。上游没有合适导出时，在本地适配器里封装。
- 源文件、QML 和资源显式列出。生成代码声明输入和输出，避免只在 configure 时跑一次。
- 共享 presets 不含个人路径。Cargo 调度和手工诊断使用同一套有效配置。
- `cmake_minimum_required` 沿用移植来源里已经验证的声明。托管 CMake 的版本可以更高，这不自动抬高源码声明。
- native 层不调用发起本次构建的同一个 Cargo target。

构建默认值和平台 ABI 放在单一策略文件里，在 `find_package` 和 `qt_add_qml_module` 之前加载。子目录只声明自己的目标和依赖。

## Windows ABI

Rust / CXX staticlib 与 native 全配置使用动态 Release CRT：`/MD` 和 `_ITERATOR_DEBUG_LEVEL=0`。Debug 保留符号和未优化代码，不使用 Debug CRT。不用 `/NODEFAULTLIB` 掩盖 ABI 冲突。

这两项 ABI 设置作为目录策略，让 Qt 生成的 object library 一并继承。告警和业务宏仍绑定具体 target。Debug 配置消费预编译依赖的 Release 导入库，并在找到 Qt 之前设置映射。configure 结束时审计实际目标，发现策略被覆盖就失败。

## 与 Cargo 的顺序

打开 FFI 时，先确认生成头和 staticlib 已经存在，再配置依赖。Qt Test 和 GoogleTest 只在 `BUILD_TESTING=ON` 时发现。单配置生成器默认 Debug；CTest 在需要配置名时带上 `-C`。
