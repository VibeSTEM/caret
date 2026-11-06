# 064 — 内置字体角色与统一默认标记

- 状态：implemented
- 依赖：024、025、042
- 优先级：P2
- 创建 / 更新：2026-11-01 / 2026-11-01

## 目标与背景

统一文档字体角色的默认值表示，并为数学与代码角色提供跨平台内置字体。`.cdoc` 的 `"<default>"` 是共享标记，具体默认字体由角色解析：正文继续交给系统正文字体后端，当前 Math 默认使用 STIX Two Math，Code 默认使用 JetBrains Mono。命令输入与未来代码文本 shaping 使用同一代码字体资源。L1 `caret-font-roles` 提供共享字体角色、当前默认族名和静态字体字节；`caret-math` 负责解析数学字体并读取 OpenType MATH 度量。

L1 `caret-font-roles` 统一持有共享字体角色、内置默认族名和所有静态字体字节，包括 STIX Two Math 与 JetBrains Mono。代码领域 API 尚未实现，不为静态资源单独保留 `caret-code`；050/051 出现独立代码语义能力时再创建 L3 代码领域 crate，不依赖同层布局或数学 crate。

## 必读

- [多文字体系字体选择](../architecture/editor/font-selection.md)、[数学公式](../architecture/editor/math.md)
- [字体资源目录](../../resources/fonts/README.md)、[源码署名](../standards/source-attribution.md)
- [验证与评审](../standards/validation-and-review.md)、[测试](../standards/testing.md)

## 范围与非目标

- 为 Math 与 Code 使用同一个 `<default>` 标记；标记不得泄漏到具体字体族查询。
- 建立无 workspace 依赖的 L1 `caret-font-roles` crate，持有 Body、Math、Code 角色、默认族名、STIX Two Math 字节和 JetBrains Mono Regular、Bold、Italic、Bold Italic 字体面接口；字体文件与上游 OFL 放在 `resources/fonts/`，Rust 与 Qt 共用同一文件。
- 命令输入、字体候选和其他代码型 UI 使用内置 JetBrains Mono；显式文档字体继续优先，缺字继续交给平台 fallback。
- 桌面 Runtime 包含 JetBrains Mono 与 STIX Two Math 的许可证，并在各平台包检查中验证。
- 不实现语法高亮、代码块模型或代码编辑器；不为纯字体资源建立空的代码领域 crate。

## 实施步骤

1. 在文档模型定义统一默认标记，在字体角色适配器中各自解析默认族。
2. 由 L1 `caret-font-roles` 持有所有字体角色、默认族名和静态字节；`caret-math` 只负责 STIX 字体解析与 MATH 度量，`caret-layout` 消费共享角色/代码字体面并保留偏好策略，`font_selection.rs` 是候选决策的唯一入口；Qt 继续从 `resources/fonts/` 加载代码字体资源。
3. 更新 Qt、Rust 快照与字体候选链路，保留显式族名优先级及缺字 fallback。
4. 更新字体授权、来源、桌面打包校验和架构/task 文档。
5. 覆盖 `.cdoc` 默认值、候选优先级、字体目录注册、Qt 资源加载与包许可证；按验证规范完成领域评审和独立 final review。

## 预计改动

现存：`crates/caret-doc/`、`crates/caret-layout/`、`crates/caret-math/`、`native/adapters/`、`native/viewmodels/`、`qml/`、`resources/fonts/`、`tests/`。新增：`crates/caret-font-roles/`、`crates/caret-layout/src/fonts.rs`、`native/adapters/fonts/`、`resources/fonts/code/jetbrains-mono/`。

## 验收标准

- [ ] Math/Code 使用统一 `<default>`；正文显式设为该标记时仍走正文系统默认解析。偏好匹配和候选策略由 `caret-layout/src/font_selection.rs` 收口；当前默认族名、角色接口和静态字节来自 `caret-font-roles`。
- [ ] L1 `caret-font-roles` 统一定义 Body、Math、Code 角色、内置默认族名以及全部静态字体字节；`caret-math` 负责 MathFont 实例/MATH 数据，`caret-layout` 负责 Fontique 注册和候选策略。
- [ ] 用户显式选择的代码字体优先于 JetBrains Mono；缺字可继续使用现有字体 fallback，不改变保存的请求。
- [ ] Regular、Bold、Italic、Bold Italic 在 Fontique 和 Qt 中可加载；目录刷新后 Fontique 仍含内置代码字体。
- [ ] macOS、Windows、Linux Runtime 均包含两种字体的许可文件；仓库 NOTICE 和资源来源说明完整。
- [ ] `caret-layout`、`caret-math` 与未来代码领域 crate 保持同层零依赖；静态字体文件由共享 `resources/fonts/` 提供，角色 crate 是 Rust 字节接口的唯一归属，Qt 资源清单只引用这些相同文件。
- [ ] 相关 Rust、native/QML 加载、打包检查和文档索引通过；未运行的跨平台包验证如实记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-01 | `cargo test --locked --offline -p caret-code -p caret-doc -p caret-layout -p caret-math -p caret-app` | 模型默认、角色候选、字体注册和快照行为通过 | 通过：caret-app 196、caret-code 1、caret-doc 54、caret-layout 102、caret-math 38 |
| 2026-11-01 | `cargo test --locked --offline -p caret-tests --test native` | 静态资源可加载，命令输入使用 JetBrains Mono | 最新 Qt 改动后 `ShellLoadTest::builtinCodeFontIsAvailableToQt` 仍通过；ShellLoad 其余 40 项通过，未改动的 `documentPathAdapterReturnsLocalPathsAndDefaultSaveLocation` 仍因 `default_url.isLocalFile()` 失败，需单独查明 |
| 2026-11-06 | `cargo test --workspace --locked --offline` | 共享字体角色、默认名、Fontique 注册和 Qt 资源加载回归通过 | 通过；native CTest 9/9，包括 `builtinCodeFontIsAvailableToQt` |
| 2026-11-06 | `cargo lint --check`、`cargo format --check`、`cargo fmt --all -- --check` | 字体 crate、资源引用与工作区规则通过 | 通过；包含 layering、CMake/QML、clang-tidy、include-cleaner、cppcheck |
| — | 三平台 Runtime package workflow | 两种字体的许可文件随包移动并通过启动 smoke | 未执行 |

## 风险与回退

同名字体可能已安装在用户系统中；Qt 与 Fontique 将优先注册的应用字体提供给 JetBrains Mono 族查询，系统字体只作为覆盖不足时的 glyph fallback。若 Qt 或 Fontique 无法加载资源，应让构建/行为测试失败，不静默退回平台 monospace。包许可证路径由 Runtime 组件收集，三平台迁移检查必须随资源路径一同维护。

当前 `MathLayoutEngine::new()` 仍直接使用 `MathFont::builtin()`；虽然字体角色候选已定义，文档中的数学字体偏好尚未影响实际公式布局。044 接通数学字体选择和诊断前，不把 Math 自定义族描述为已生效。

## 决策与工作记录

- 2026-11-01：Math 和 Code 共用 `<default>` 序列化标记；角色映射分别由数学与代码字体模块持有。
- 2026-11-01：初版曾以 `caret-code` 持有 JetBrains Mono 数据；后续架构整理确认静态资源不应属于代码领域 crate。
- 2026-11-06：按维护者方向将所有静态字体资源归入无 workspace 依赖的 L1 `caret-font-roles`，并移除只包装字体数据的 `caret-code`。未来代码领域 API 由 050/051 在具备真实职责时创建为 L3；偏好与脚本/语言候选策略仍由 `caret-layout` 管理，MATH 度量仍由 `caret-math` 管理。
- 2026-11-06：Qt 与 Rust 从同一 `resources/fonts/` 文件加载字体；若更换内置 Code 字体，需同步更新角色默认族、CMake/QRC 文件映射，并由 Fontique 与 Qt 加载测试验证族名和字形面一致。
- 2026-11-06：分领域 review 和 workspace 验证通过。三平台 Runtime package workflow 尚未执行，因此状态为 implemented，待外部打包验收后再标 done。

## 完成摘要

共享字体角色、静态字节接口和 Qt 加载已实现并通过仓库内验证；三平台 Runtime 打包 smoke 尚待执行。
