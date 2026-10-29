# 039 — Settings 中的主题与界面语言

- 状态：done
- 依赖：003、004、029
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-10-29

## 目标与背景

Rust 已有 Settings、Theme、Language 服务，QML 有设置窗口和语言页，但主题选择和界面语言切换尚未形成完整、可发现的 Settings 流程。应用支持英语、简体中文、繁体中文和日语；语言目录与实际构建出的翻译资源必须保持一致。用户需要在 Settings 中切换界面主题和语言，并让语言目录成为单一、可校验的数据源。

## 必读

- [Settings 运行时](../architecture/core/runtime.md)、[字体选择](../architecture/editor/font-selection.md)
- [分层](../standards/layering.md)、[QML](../standards/qml.md)、[测试](../standards/testing.md)

## 范围与非目标

- 在 Settings UI 提供主题与应用界面语言选择；主题和语言继续由 Rust Settings/Theme/Language 服务持有并持久化，QML 不维护第二份已提交状态。
- 补齐并校验应用界面语言目录：每个可选 locale 均有可加载的翻译资源和明确的显示名称，构建资源、Rust 支持目录与 UI 选项一致。
- 语言切换先验证资源可加载，再提交 Rust 设置；失败时保留旧语言与界面，错误可见。
- 主题仅选择仓库当前支持的主题；外部用户主题编辑、导入/导出及文档排版主题不属于本任务。
- 不扩大文档正文的 BCP 47 语言能力；界面语言与文档排版语言继续分离。

## 实施决策

- Rust `SettingsRuntime` 原子验证并持久化主题、界面语言和设置页偏好；`SettingsViewModel` 提供选择目录与即时 Qt 投影，设置状态机只持有未提交草稿。
- 支持 `en`、`zh-Hans`、`zh-Hant`、`ja`；旧设置中的 `zh-CN` 读取时归一为 `zh-Hans`。Rust 目录、原生显示名称和 `.pa` 资源一一对应。
- 语言立即生效：先加载候选 QM，再提交 Rust 设置，成功后替换活动翻译器。界面语言与正文 BCP 47 标签独立。

## 实施步骤

1. 盘点 Settings 对话框、Rust 服务、ViewModel、翻译文件与 CMake/qrc 注册，列出缺失语言和主题链路。
2. 设计 Rust 提供的 locale 元数据（规范 locale、显示名称、资源状态），使选择器不靠 QML 临时推导支持范围。
3. 接通主题与语言草稿、校验、提交、持久化、即时投影及失败恢复。
4. 增加服务、跨层和 Qt 测试，检查切换失败、重复选择、重启恢复和翻译资源缺失。
5. 更新运行时、QML 与 task-index 文档，执行独立 final review。

## 预计改动

`crates/caret-app/src/settings/`、`crates/caret-ffi/`、`native/viewmodels/`、`qml/Dialogs/SettingsDialog.qml` 与设置页面、`resources/i18n/`、CMake 资源注册、相关测试和架构文档。

## 验收标准

- [x] Settings 中可访问主题与界面语言选择，当前值与 Rust 持久化快照一致。
- [x] 提交后界面主题/翻译即时生效；重启后恢复所选值。
- [x] 选择失败不改变当前设置或已显示的主题/语言，并呈现可理解的错误。
- [x] locale 目录与实际构建翻译资源一致，无重复 locale、缺失资源或不可达选项。
- [x] 应用界面语言与 `.cdoc` 的正文语言分离，Rust 是持久化状态的唯一权威来源。
- [x] Rust、Qt/QML 测试、格式和 lint 通过；独立评审无未解决问题。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-29 | `cargo test -p caret-app --locked settings::` | 设置、locale 归一与四语目录完整性 | 30 项通过；四个 locale 条目、完成状态、占位符、目录/资源映射一致 |
| 2026-10-29 | `cargo test -p caret-ffi --locked` | FFI 设置快照与提交接口 | 13 项通过 |
| 2026-10-29 | `cargo test -p caret-tests --test native --locked` | 编译 QM、Qt/QML 设置流程与窗口 smoke | CTest 7/7 通过；四种 QM 均可加载，状态机取消、设置提交、已有 QML 文本即时切换日语/简中、失败保留、主题通知和磁盘恢复通过 |
| 2026-10-29 | 锁定 `lupdate qml native -recursive` 与生成 TS 对照 | 翻译源完整同步 | 提取 84 条 context/source，四种生成 TS 完全一致；文档标题的未命名、保存中、已编辑和已保存状态均随界面语言切换 |
| 2026-10-29 | `cargo format --check`、`cargo lint --check`、`git diff --check` | 跨语言格式和静态检查 | 全部通过；lint 完成 Rust Clippy、依赖检查、CMake 格式与分层、QML 元数据与 qmllint、clang-tidy、include-cleaner、cppcheck |
| 2026-10-29 | 独立最终复审 | 检查跨层职责、失败路径、行为测试与翻译资源同步 | 无阻塞问题；设置链路覆盖取消、提交、即时翻译、主题 token 更新、重启恢复及失败状态保留 |

## 风险与回退

Qt 翻译资源在 Rust 提交前预加载；候选主题目录也由 Rust 校验。运行中切换依赖已启动的 `QCoreApplication` 安装有效 QM。若未来允许动态主题或外部语言资源，须把全部可失败的解析/准备步骤移到 Rust 提交之前，并验证回滚行为。

## 决策与工作记录

- 2026-10-29：建立任务；主题与界面语言属于 Settings 分类服务，字体/文档语言不是此处的界面偏好。
- 2026-10-29：增加通用简繁体脚本 locale 和日语，补全四语 QML 翻译，并通过 Qt Translator 即时切换。
- 2026-10-29：加入 locale、资源目录和占位符完整性测试；同步评审规范的翻译检查要求。
- 2026-10-29：人工验收发现切换后已有 QML 绑定未刷新，导致页面新旧语言混杂；翻译器安装后调用应用 QQmlEngine 的 `retranslate()`，并以已创建的 QML 文本属性验证切换与切回。
- 2026-10-29：最终复审发现窗口标题的保存状态和 Rust 默认文件名绕过翻译；改为 QML 按活动 locale 翻译未命名、保存中、已编辑和已保存，并补四语资源及即时切换测试。

## 完成摘要

设置 UI、Rust 持久化、四语目录、Qt 即时翻译、文档标题状态翻译及跨层行为验证均完成。格式、lint、差异空白检查和独立最终复审均通过。
