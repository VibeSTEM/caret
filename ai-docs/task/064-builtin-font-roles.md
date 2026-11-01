# 064 — 内置字体角色与统一默认标记

- 状态：in-progress
- 依赖：024、025、042
- 优先级：P2
- 创建 / 更新：2026-11-01 / 2026-11-01

## 目标与背景

统一文档字体角色的默认值表示，并为数学与代码角色提供跨平台内置字体。`.cdoc` 的 `"<default>"` 是共享标记，具体默认字体由角色解析：正文继续交给系统正文字体后端，Math 使用 STIX Two Math，Code 使用 JetBrains Mono。命令输入与未来代码文本 shaping 使用同一 JetBrains Mono 资源；数学公式的字体数据和 OpenType MATH 度量由 Rust 数学后端独立持有。

`caret-code` 是独立代码文本领域 crate，当前提供代码字体族与字形资源；后续代码语言识别、token、高亮范围和增量重算由它持有。它不依赖编辑器、Parley、Qt 或文档布局。此任务不提前定义未实现的高亮 token API。

## 必读

- [多文字体系字体选择](../architecture/editor/font-selection.md)、[数学公式](../architecture/editor/math.md)
- [字体资源目录](../../resources/fonts/README.md)、[源码署名](../standards/source-attribution.md)
- [验证与评审](../standards/validation-and-review.md)、[测试](../standards/testing.md)

## 范围与非目标

- 为 Math 与 Code 使用同一个 `<default>` 标记；标记不得泄漏到具体字体族查询。
- 建立无领域依赖的 `caret-code` crate，持有 JetBrains Mono 族名和 Regular、Bold、Italic、Bold Italic 字形资源接口；字体文件与上游 OFL 放在 `resources/fonts/code/jetbrains-mono/`，Rust Fontique 与 Qt 资源共用同一文件。
- 命令输入、字体候选和其他代码型 UI 使用内置 JetBrains Mono；显式文档字体继续优先，缺字继续交给平台 fallback。
- 桌面 Runtime 包含 JetBrains Mono 与 STIX Two Math 的许可证，并在各平台包检查中验证。
- 不实现语法高亮、代码块模型或代码编辑器；`caret-code` 先建立可复用领域边界，不提前添加高亮框架。

## 实施步骤

1. 在文档模型定义统一默认标记，在字体角色适配器中各自解析默认族。
2. 由 `caret-code` 提供代码族名和四种字体数据；`caret-math` 提供数学族名、STIX 数据与 MATH 度量；`caret-layout` 的对称角色适配器消费两者，`font_selection.rs` 是候选决策的唯一入口；Qt 适配器独立加载代码字体资源。
3. 更新 Qt、Rust 快照与字体候选链路，保留显式族名优先级及缺字 fallback。
4. 更新字体授权、来源、桌面打包校验和架构/task 文档。
5. 覆盖 `.cdoc` 默认值、候选优先级、字体目录注册、Qt 资源加载与包许可证；按验证规范完成领域评审和独立 final review。

## 预计改动

现存：`crates/caret-doc/`、`crates/caret-layout/`、`crates/caret-math/`、`native/adapters/`、`native/viewmodels/`、`qml/`、`resources/fonts/math/`、`tests/`。新增：`crates/caret-code/`、`crates/caret-layout/src/fonts/`、`native/adapters/fonts/`、`resources/fonts/code/jetbrains-mono/`。

## 验收标准

- [ ] Math/Code 使用统一 `<default>`；正文显式设为该标记时仍走正文系统默认解析。具体角色策略由 `caret-layout/src/font_selection.rs` 收口，角色默认值分别来自同构的 `fonts/math.rs`、`fonts/code.rs` 适配器。
- [ ] Math 默认族名与 MathFont 实例都由 `caret-math` 提供 STIX Two Math；Code 默认族名和字体数据都由 `caret-code` 提供 JetBrains Mono。
- [ ] 用户显式选择的代码字体优先于 JetBrains Mono；缺字可继续使用现有字体 fallback，不改变保存的请求。
- [ ] Regular、Bold、Italic、Bold Italic 在 Fontique 和 Qt 中可加载；目录刷新后 Fontique 仍含内置代码字体。
- [ ] macOS、Windows、Linux Runtime 均包含两种字体的许可文件；仓库 NOTICE 和资源来源说明完整。
- [ ] `caret-code` 是不依赖编辑器或布局的代码文本领域 crate；`caret-layout` 消费两种领域 crate 的稳定字体接口，但不把 shaping 与目录逻辑反向放入它们。
- [ ] 相关 Rust、native/QML 加载、打包检查和文档索引通过；未运行的跨平台包验证如实记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-01 | `cargo test --locked --offline -p caret-code -p caret-doc -p caret-layout -p caret-math -p caret-app` | 模型默认、角色候选、字体注册和快照行为通过 | 通过：caret-app 196、caret-code 1、caret-doc 54、caret-layout 102、caret-math 38 |
| 2026-11-01 | `cargo test --locked --offline -p caret-tests --test native` | 静态资源可加载，命令输入使用 JetBrains Mono | 最新 Qt 改动后 `ShellLoadTest::builtinCodeFontIsAvailableToQt` 仍通过；ShellLoad 其余 40 项通过，未改动的 `documentPathAdapterReturnsLocalPathsAndDefaultSaveLocation` 仍因 `default_url.isLocalFile()` 失败，需单独查明 |
| — | 三平台 Runtime package workflow | 两种字体的许可文件随包移动并通过启动 smoke | 未执行 |

## 风险与回退

同名字体可能已安装在用户系统中；Qt 与 Fontique 将优先注册的应用字体提供给 JetBrains Mono 族查询，系统字体只作为覆盖不足时的 glyph fallback。若 Qt 或 Fontique 无法加载资源，应让构建/行为测试失败，不静默退回平台 monospace。包许可证路径由 Runtime 组件收集，三平台迁移检查必须随资源路径一同维护。

当前 `MathLayoutEngine::new()` 仍直接使用 `MathFont::builtin()`；虽然字体角色候选已定义，文档中的数学字体偏好尚未影响实际公式布局。044 接通数学字体选择和诊断前，不把 Math 自定义族描述为已生效。

## 决策与工作记录

- 2026-11-01：Math 和 Code 共用 `<default>` 序列化标记；角色映射分别由数学与代码字体模块持有。
- 2026-11-01：创建 `caret-code` 作为独立代码文本领域；代码与数学在布局层使用相同结构的字体角色适配器。`caret-layout` 依赖 `caret-math` 的稳定字体角色接口，候选策略仍集中在 `font_selection.rs`。

## 完成摘要

未完成。
