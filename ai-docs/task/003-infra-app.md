# 003 — 路径、设置、语言与主题

- 状态：done
- 依赖：001、002
- 优先级：P0
- 创建 / 更新：2026-10-25 / 2026-10-27

## 目标与背景

窗口和命令行以后都要解析路径、读写设置、切换语言和主题。这些服务放在 `caret-app`，本任务只移植服务本身。还不打开文档，也不创建会话。

## 必读

- [Rust](../standards/rust.md)、[DSL](../standards/dsl.md)、[分层](../standards/layering.md)
- [仓库结构](../architecture/core/repository-layout.md)、[运行时](../architecture/core/runtime.md)

## 范围与非目标

从源应用 crate 移植路径、运行时上下文、设置、语言和主题。六类逻辑路径按 Rust 规范实现：`project`、`user-config`、`app-data`、`cache`、`session`、`qrc`。

`caret-app` 本阶段可以依赖 `caret-foundation` 和 `caret-dsl-core`。不依赖 `caret-engine`、`caret-ffi` 或 Qt。

不包含：`.cdoc`、命令执行、QML 绑定、崩溃之外的界面状态。

## 前置条件与待决策

001、002 的 crate 已在同一分支，独立评审还没完成。设置 schema 只保留编辑器需要的键。来源里的工艺键不移植。

保留的键来自来源偏好快照，没有材料、网格或求解项。文件是调用方给定路径上的 JSON，字段用 camelCase，未知字段拒绝。

| 字段 | 规则 |
| --- | --- |
| `schemaVersion` | 必须是 `1` |
| `themeId` | `light` 或 `dark` |
| `language` | `en`、`zh-Hans`、`zh-Hant` 或 `ja`；旧值 `zh-CN` 读取时归一为 `zh-Hans` |
| `windowWidth` | `1280..=16384` |
| `windowHeight` | `720..=16384` |
| `windowMaximized` | 布尔 |
| `recentProjectPaths` | 最多 10 条绝对路径；重复项去掉，新路径排在前面 |

`recentProjectPaths` 只是绝对路径字符串，不是工程模型，也不是 `.cdoc`。

## 实施步骤

1. 列出要保留的设置键，删掉工艺键。记录来源 commit。
2. 实现路径解析和设置读写。损坏的设置文件隔离，不覆盖用户还没坏的那份。
3. 语言和主题通过 `caret-dsl-core` 读取 `.pa`，不在 `caret-app` 再写一个解析器。
4. 为路径、损坏设置和未知 scheme 补测试。
5. 独立评审后复验。

## 预计改动

新建 `crates/caret-app/`。`.pa` 源可以先放在 `resources/`，生成 QM 留到 004。

## 验收标准

- [x] 六类 scheme 的解析结果与规范一致。未知 scheme 失败，不落到进程当前目录。
- [x] 设置读写往返成功。损坏文件被隔离，内存中的上一份有效设置还在。
- [x] `caret-app` 的依赖符合 L5，且本阶段不依赖尚未创建的 `caret-engine`。
- [x] 没有工艺领域的设置键。fmt 与 clippy 通过。评审发现已处理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-25 | macOS，`cargo test --offline -p caret-app` | 通过 | 通过，44 项。含六类 scheme、未知 scheme、设置往返和损坏隔离 |
| 2026-10-25 | macOS，`cargo fmt --all -- --check` 与 `cargo clippy --locked --workspace --all-targets -- -D warnings` | 通过 | 通过。盘符前缀修复之后复跑仍通过 |
| 2026-10-25 | macOS，`cargo test --offline -p caret-app --lib path::tests::` | `C:secret` 被拒绝 | 16 项通过 |
| 2026-10-25 | 损坏设置夹具 | 隔离且不丢上一份 | `corrupt_settings_file_is_isolated_without_replacing_memory` 与 `invalid_snapshot_is_isolated_and_keeps_the_previous_snapshot` 通过 |

## 风险与回退

来源的路径实现和工程模型缠在一起。只搬路径和设置，工程模型留在来源仓库。回退是移除 `caret-app` 成员。

## 决策与工作记录

- 2026-10-25：创建任务。`caret-engine` 延后到 008，因此本任务的 `caret-app` 先不依赖它。008 再补上这条边。
- 2026-10-25：按要求先搬主题。`caret-app` 目前只有 `theme` 和 `theme_schema`，测试读取 `qml/Themes/` 的三份 `.pa` 和 `Theme.qml`。`Theme.qml` 的模块名改为 `Caret.Shell`，模块本身留到 004。路径、设置和语言还没搬，验收未完成。
- 2026-10-25：从 panta `ee84d95` 复制 `path`、`language`、`settings`、`runtime`。运行时测试不再依赖来源工程夹具。设置文件读写在 Rust：同目录临时文件、fsync、改名；损坏或校验失败的原字节改存为 `<文件名>.isolated`，内存快照不替换。工艺设置没有复制。
- 2026-10-25：独立评审发现 `C:secret` 这种盘符相对片段只在带斜杠时被拒绝。`validate_relative` 改为拒绝任意以「字母 + 冒号」开头的组件，并补了 `C:secret` 与 `notes/C:secret` 两条测试。评审里的其余项没有改代码。
- 2026-10-25：主题 schema 保留来源界面的 token 名，其中有工艺对话框尺寸。设置键没有工艺项。改这些 token 名留给后续界面任务。
- 2026-10-27：后续任务 029 将 `settings.rs` 归并为 `settings/` 模块：SettingsService 协调 Preferences、Theme、Language，SettingsRuntime 管理文件生命周期和 Ready/ReadOnly/Unavailable 状态；共享原子替换工具放在 `caret-app::storage`。此整理不改变 003 的路径语义或配置 JSON 兼容规则。

## 完成摘要

路径、设置、语言、主题和运行时上下文在 `caret-app`。不打开文档，也不创建编辑会话。QML 模块留到 004。
