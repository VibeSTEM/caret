# 技术规范索引

更新日期：2026-10-30。这些规范是实现和评审的长期约定。Rust/Cargo、CMake/native、QML 基础设施和 CI 已接入；基础文本编辑、文档存储、段落布局与结构导航已实现，公式、表格及部分高级能力仍在规划或实施。功能状态以[任务索引](../task-index.md)为准，不要把规划中的能力写成可用功能。

工程基础设施从维护者自己的 [panta](https://github.com/Yuki-Nagori/panta) 移植：QML + Rust 桌面栈、工具链、基础库、DSL、测试入口和 CI。移植提交写明来源仓库和 commit，并把 `panta` 前缀改成 `caret`。本规范不写本机绝对路径。

## 移植范围

| 类别 | 处理 |
| --- | --- |
| 工具链 | 移植 Rust 工具链、CMake、Ninja、LLVM、uv / Python 质量工具、Qt、GoogleTest、CXX。下载内容放在 `.toolchain/`，不放进 `target/` |
| Rust crate | 移植构建供给、进程设施、路径、运行时上下文、设置、语言、主题，以及 DSL 核心和 `dslc` |
| QML | 移植原子组件、组合组件、对话框外壳、设置 / 语言对话框和主题门面 |
| CI | 移植质量、构建、夜间检查、桌面打包和 GoogleTest SDK 管线 |
| 范围外 | 几何内核、网格、VTK、求解协议，以及只服务于这些领域的 crate、面板和工艺对话框 |

精确版本见[技术基线](baseline.md)。crate 与 QML 文件清单见 [Rust](rust.md) 和 [QML](qml.md)。

## 通用规范

| 主题 | 文档 | 何时阅读 |
| --- | --- | --- |
| 无死代码与兼容例外 | [代码生命周期](code-lifecycle.md) | 替换或删除功能 |
| 每次 commit 的一致性 | [提交规范](commits.md) | 提交、分支和 GitHub |
| 注释与接口契约 | [注释规范](comments.md) | 新增或修改代码 |
| 源码署名 | [源码署名](source-attribution.md) | 新增源码或批量补署名 |
| 文件、产物与忽略规则 | [仓库文件规范](repository-hygiene.md) | 添加配置、目录或资源 |
| 文档与示例 | [文档规范](documentation.md) | 新建或修改文档 |
| 验证与变更评审 | [验证与评审](validation-and-review.md) | 实施、独立评审和委派 |
| 测试目录与入口 | [测试规范](testing.md) | 新增或注册测试 |
| 分层与依赖方向 | [分层索引](layering.md) | 跨层改动、新 crate 或新适配器；再按范围阅读 Rust、Native 或依赖图专题 |
| 数值、定点精度与单位 | [数值与单位](numeric-values.md) | 配置、文档属性、跨语言接口和几何换算 |
| 事件通知与定时器 | [事件驱动与调度](event-driven.md) | UI 刷新、异步通知、定时器和轮询 |

## 技术规范

| 技术 | 文档 | 适用范围 |
| --- | --- | --- |
| 版本 | [技术基线](baseline.md) | 所有任务 |
| 依赖获取 | [依赖获取](dependency-acquisition.md) | 环境、工具和 SDK |
| CI | [持续集成](ci.md) | workflow 与缓存 |
| Rust 2024 | [Rust](rust.md) | crate、路径和 DSL |
| `.pa` DSL | [DSL](dsl.md) | 语言、变量、主题、状态机源文件 |
| Cargo | [Cargo](cargo.md) | workspace 与调度 |
| C++20 | [C++](cpp.md) | native 与适配器 |
| C++ 测试 | [GTest](gtest.md) | native 测试 |
| CMake | [CMake](cmake.md) | native 构建图 |
| Ninja | [Ninja](ninja.md) | 构建执行 |
| Python 工具 | [Python](python.md) | CMake 与 native 质量工具 |
| QML | [QML](qml.md) | 原子组件、组合组件、弹窗 |
| 图标 | [图标](icons.md) | 随组件移植的 SVG |
| Qt 6 | [Qt](qt.md) | 对象、线程、部署 |
| CXX | [CXX](cxx.md) | Rust 与 C++ 桥接 |
| 跨语言边界 | [FFI](ffi.md) | 所有权、错误和句柄 |

## 文档职责

| 内容 | 维护位置 |
| --- | --- |
| 产品范围和编辑器行为 | 架构文档 |
| 选型、版本和移植边界 | 本目录 |
| 单次工作的范围和证据 | 对应 task |
| 任务状态摘要 | 任务索引 |

任务里的结论经过验证后，再写回架构或规范。规划中的命令不能从本目录推断为已经实现。
