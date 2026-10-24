# C++ 编码与资源管理

更新日期：2026-10-24。适用于自有 native 代码。基线是 C++20。第三方源码不套用本仓库的风格。格式由移植后的根 `.clang-format` 统一，检查使用同一套 LLVM 里的 `clang-format --dry-run -Werror`。

## 项目规则

- 每个自有 target 显式要求 C++20，并关闭编译器扩展。不把 C++23 专属设施当成默认依赖。
- 文件用 `snake_case.hpp` / `.cpp`。类型用 `PascalCase`，函数和变量用 `snake_case`。Qt 属性、槽和信号用 `camelCase`。
- 优先值和 RAII。唯一所有权用 `std::unique_ptr`。Qt parent 已经表达销毁关系时，不再叠一个独立释放者。
- `std::span`、引用和裸指针要写明有效期。不把短命栈数据保存到后台任务。
- 公开头只包含必要依赖。禁止头文件级 `using namespace`。
- 可恢复失败使用明确的错误契约。适配器在边界捕获异常并转换。析构不抛异常。
- 后台任务捕获输入快照和取消标记，不捕获生命周期不受控的 `this`。

告警、clang-tidy、include-cleaner、cppcheck 和 sanitizer 只作用于自有代码。第三方头的告警隔离开，不全局关闭。`cargo sanitize` 的组合沿用来源：以 ASan 加 UBSan 为主，TSan 与之互斥，只在工具链支持的平台运行。
