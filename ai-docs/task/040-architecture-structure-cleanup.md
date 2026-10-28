# 040 — 架构与结构审计整改

- 状态：in-progress
- 依赖：无
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-10-29

## 目标与背景

整体架构审查发现少量职责边界和遗留组件问题。此任务收敛可独立验证的结构整改，避免在无关功能中机械拆文件或新建 crate。已发现的候选项包括：AutoSave/recovery 编排落在 FFI；保存错误字符串承载状态协议；未被生产消费的 QML 组件子树；以及过大的 Rust 模块。实施前须复核当前 main，避免重复修复或因简单文本搜索误删仍由动态注册使用的资源。

## 必读

- [运行时](../architecture/core/runtime.md)、[仓库布局](../architecture/core/repository-layout.md)
- [分层](../standards/layering.md)、[验证与评审](../standards/validation-and-review.md)、[仓库文件](../standards/repository-hygiene.md)

## 范围与非目标

- 将审计发现按职责逐项验证并整改；优先用稳定类型替代跨层字符串协议，并将应用策略从 FFI 边界归回应用服务。
- 清理已证实无消费者的 QML 资源及其依赖闭包，并同步 CMake、组件目录与文档。
- 对大文件只在职责边界明确且能保持测试归属时拆分；不按行数机械拆分。
- 检查重复实现、孤儿文件、未使用依赖和 crate 边界；仅在独立职责/复用/安全边界成立时新增 crate。
- 不改变 AutoSave、文档状态机或跨层行为语义；不包含多窗口架构实现。

## 前置条件与待决策

- 先复核 037 完成状态与 038 边界，确定 AutoSave 文档运行时是否已被后续改动处理。
- 对 QML 组件使用方式交叉检查静态 QML、CMake/qmldir/qrc、测试及运行时加载后再删除。
- 如类型化 FFI 结果要改变公开 ABI，记录迁移计划和调用方清单。

## 实施步骤

1. 基于当前 main 复审候选项，记录问题、触发路径、影响与收益。
2. 将独立边界的整改分成可审查的小提交；保留领域策略单一权威来源。
3. 清理不再使用的资源、注册、依赖及文档引用。
4. 为行为边界补回归测试，运行受影响 crate、Qt/QML 测试及分层检查。
5. 更新架构文档与此任务，执行独立 final review。

## 预计改动

`crates/caret-app/`、`crates/caret-ffi/`、`native/viewmodels/`、`qml/Components/`、CMake/QML 资源注册、测试、架构与仓库结构文档。最终范围由复审确认。

## 验收标准

- [ ] FFI 只承担 DTO 转换与调用转发；应用策略仍由应用服务拥有。
- [ ] 跨层结果使用明确类型，不从本地化错误文案解析状态或能力。
- [x] 每个已删除 QML 项均确认无静态、构建和测试消费者，相关 CMake 注册、主题 token 与文档一并清理。
- [ ] 大模块只按职责拆分；crate 边界有收益说明，不为目录整洁额外建 crate。
- [ ] 未使用依赖、重复实现和孤儿文件已清理，或有明确保留依据。
- [ ] 相关行为测试、分层检查和独立评审通过；架构文档反映最终结构。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-29 | `cargo test --locked -p caret-app settings::theme::tests` | 清理主题 token 后 schema 与内置主题保持完整 | 通过，8 项主题服务测试全部通过。 |
| 2026-10-29 | `cargo test --locked -p caret-tests --test native` | QML 注册、Shell 加载与格式验证 | 通过；CTest 4/4，包括 `Qml.ShellLoad` 和 `Qml.FormatCheck`。 |
| 2026-10-29 | `cargo format --check`、`git diff HEAD --check`、QML 注册路径核对 | 格式、空白和 CMake QML 文件路径有效 | 通过；注册的 QML 文件全部存在，Theme.qml token 与 Rust schema 一一对应。 |
| 2026-10-29 | 独立 subagent 对当前 diff 的只读 final review | 组件删除闭包、主题 token、任务交接与恢复锁无未解决问题 | 通过；指出的文档更新日期/任务措辞问题已修正。未宣称 037 跨平台 CI 已通过。 |

## 风险与回退

移除 QML 资源或迁移应用编排可能影响运行时加载和多窗口后续扩展。每项迁移需保持行为回归测试；删除可分步提交，发生遗漏时可按提交单独恢复。

## 决策与工作记录

- 2026-10-29：创建任务承接整体架构审计中不适合顺手修改的整改项；本次修复 CI 的小范围问题不要求等待本任务。
- 2026-10-29：按产品当前范围移除未使用的 Ribbon、后台菜单、文档/面板页签、信息面板、工具条和表单分组 Composites，并同步清理专属 Theme token。移除无调用方且只是通用按钮薄包装的 `ChromeToolButton`，删除无复用逻辑的 `PanelSurface`；将仅由复选框使用的 `CheckIndicator` 内聚到 `ThemedCheckBox`，将菜单专用命名改为准确的 `CommandBarTabButton`，并将数字输入重命名为 `ThemedNumericField`。复选框、单选框和数字输入仍保留为设置页可复用的语义控件。
- 2026-10-29：QML 组件清理、主题 token/schema 和组件规范同步完成，本机主题服务与 QML native 聚合测试通过。Rust/FFI 职责迁移、类型化结果、其他大型模块审查与全仓最终评审仍未完成。

## 完成摘要

未完成。
