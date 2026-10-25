# Ninja 构建执行规范

更新日期：2026-10-25。版本见[技术基线](baseline.md)。Ninja 由 Cargo 供给，供 CMake 生成 native 构建；它不负责依赖选版。

## 项目规则

- 由 CMake 生成 Ninja 文件。不手工维护或提交 `build.ninja`。
- 使用单配置 Ninja。Debug 和 Release 分开放到 `target/native/<profile>`。
- 通过 `cmake --build` 调用。直接跑 Ninja 只用于诊断。
- 编排层协调 Cargo 和嵌套 Ninja 的并行度，避免两边同时占满机器。
- 生成文件写全依赖。删除一个产物后应能重建。
- 记录失败命令和退出码。重试成功不能代替依赖图正确。

共享构建目录不并发跑多份 configure 或 build。
