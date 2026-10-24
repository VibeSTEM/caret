# 技术基线与版本决策

更新日期：2026-10-24。下表是准备从 panta 移植的锁定版本。Caret 仓库尚未接入这些工具；移植时以当时源文件里的 URL、SHA256 和 commit 为准，并在同一次提交里更新本表。

## 当前选择

| 项目 | 选择 | 说明 |
| --- | --- | --- |
| 主验证平台 | macOS arm64 | 三平台 CI 覆盖构建，不代替实机窗口验收 |
| C++ | 自有代码 C++20 | 编译器是托管 LLVM，不是系统 clang 的浮动版本 |
| LLVM | 22.1.7 | Unix 用 clang / clang++，Windows 用 clang-cl；格式和静态检查用同一套 |
| Rust | stable 1.99.0，edition 2024 | MSRV 1.95；工具链文件锁定 channel 和 rustfmt、clippy |
| Cargo | 开发入口 | 调度构建、测试、格式、lint、覆盖率和打包 |
| CMake + Ninja | CMake 4.4.3 + Ninja 1.13.2 | Cargo 供给固定资产。native 树的 `cmake_minimum_required` 沿用来源，当前为 3.22 |
| Qt | 6.11.2 | qtbase、qtdeclarative、QtSvg、QtTools、QtScxml；Linux 附带与该 Qt 匹配的 ICU 73.2 |
| GoogleTest | 1.18.0 | 仅 `BUILD_TESTING=ON` 时取静态 SDK |
| CXX | 1.0.202 | Rust 与 C++ 的桥接；生成器和库版本必须相同 |
| Python 工具 | uv 0.12.18，CPython 3.14.7 | 只服务质量工具。cmakelang 0.6.13，cppcheck 1.5.1 |
| 路径与锁 | Rust `Path` / `PathBuf`，fs4 1.1.0 | 逻辑路径服务和安装锁一起移植 |
| DSL | pest 2.8，`.pa` | `caret-dsl-core` 与 `caret-dslc` |
| 基准计时 | hyperfine 2.0.0 | 按需安装，不进入默认构建 |
| macOS 部署目标 | 13.0 | LLVM 不替代 Apple SDK；sysroot 仍用系统 SDK |

## 版本维护

优先固定的稳定发布，不用浮动的 latest 或 nightly 代替锁定版本。升级时同一提交写来源、ABI、启用模块、验证和回退。第三方许可证随发布包保留：仓库的 Apache-2.0 不覆盖 LLVM、Qt、CMake 或其他依赖。

表格是选择，不是验收证据。工具能下载、能 configure、能打开窗口，分别记录。
