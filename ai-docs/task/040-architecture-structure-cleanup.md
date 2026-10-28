# 040 — 架构与结构审计整改

- 状态：done
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

- [x] FFI 只承担 DTO 转换与调用转发；应用策略仍由应用服务拥有。
- [x] 保存冲突、恢复绑定/写入状态、替换能力和过期 deadline 等跨层状态使用明确字段，不从错误文案解析；其他稳定错误码仅用于诊断与错误分类。
- [x] 每个已删除 QML 项均确认无静态、构建和测试消费者，相关 CMake 注册、主题 token 与文档一并清理。
- [x] 大模块只按职责拆分；crate 边界有收益说明，不为目录整洁额外建 crate。
- [x] 未使用依赖、重复实现和孤儿文件已清理，或有明确保留依据。
- [x] 相关行为测试、分层检查和独立评审通过；架构文档反映最终结构。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-29 | `cargo test --locked -p caret-app settings::theme::tests` | 清理主题 token 后 schema 与内置主题保持完整 | 通过，8 项主题服务测试全部通过。 |
| 2026-10-29 | `cargo test --locked -p caret-tests --test native` | QML 注册、Shell 加载与格式验证 | 通过；CTest 4/4，包括 `Qml.ShellLoad` 和 `Qml.FormatCheck`。 |
| 2026-10-29 | `cargo format --check`、`git diff HEAD --check`、QML 注册路径核对 | 格式、空白和 CMake QML 文件路径有效 | 通过；注册的 QML 文件全部存在，Theme.qml token 与 Rust schema 一一对应。 |
| 2026-10-29 | `cargo test --locked -p caret-app -p caret-doc -p caret-layout -p caret-ffi` | 应用生命周期、模型/布局拆分与 FFI 回归 | 通过；应用 132 项、文档 26 项、布局 81 项、FFI 11 项全部通过，含未领取 pending lease 释放测试。 |
| 2026-10-29 | `cargo test --locked -p caret-tests --test native` | C++/QML shell 编译、冲突对话框场景及 QML 检查 | 通过；CTest 4/4，包含 `Qml.ShellLoad`、`Qml.FormatCheck`。 |
| 2026-10-29 | `cargo clippy --locked -p caret-app -p caret-doc -p caret-layout -p caret-ffi --all-targets -- -D warnings`、`cargo format --check`、`cargo machete`、`git diff --check` | 修改 crate 无告警，跨语言格式、依赖和差异清洁 | 通过；额外完整 `cargo lint --check` 九阶段通过，含 Clippy、依赖、CMake/QML、分层、clang-tidy、include-cleaner、cppcheck。 |
| 2026-10-29 | 未参与实现的 `gpt-6.1-sol` subagent 对完整 diff 及整改后 diff 只读评审 | 状态语义、层次、模块拆分、恢复租约生命周期无阻断问题 | 两轮均无阻断。第二轮确认过期 recovery-update/Replace 回调已防护，未领取租约释放及 stale deadline 处理符合预期；未直接自动化覆盖窗口销毁情形。 |

## 风险与回退

移除 QML 资源或迁移应用编排可能影响运行时加载和多窗口后续扩展。每项迁移需保持行为回归测试；删除可分步提交，发生遗漏时可按提交单独恢复。

## 决策与工作记录

- 2026-10-29：创建任务承接整体架构审计中不适合顺手修改的整改项；本次修复 CI 的小范围问题不要求等待本任务。
- 2026-10-29：按产品当前范围移除未使用的 Ribbon、后台菜单、文档/面板页签、信息面板、工具条和表单分组 Composites，并同步清理专属 Theme token。移除无调用方且只是通用按钮薄包装的 `ChromeToolButton`，删除无复用逻辑的 `PanelSurface`；将仅由复选框使用的 `CheckIndicator` 内聚到 `ThemedCheckBox`，将菜单专用命名改为准确的 `CommandBarTabButton`，并将数字输入重命名为 `ThemedNumericField`。复选框、单选框和数字输入仍保留为设置页可复用的语义控件。
- 2026-10-29：QML 组件清理、主题 token/schema 和组件规范同步完成，本机主题服务与 QML native 聚合测试通过。
- 2026-10-29：将每文档 AutoSave 与恢复快照策略协调移入 `caret-app::document_lifecycle::DocumentLifecycleService`；FFI 只转交编辑 revision、deadline 事件及写入结果。类型化保存结果明确表达冲突、Replace 权限、恢复项 ID/新租约和错误；Qt 结果所有者在未绑定时释放新租约，绑定后不触碰会话租约。过期 deadline 也改用显式字段。
- 2026-10-29：将 `caret-doc::model` 的编解码、功能标识推导和校验拆为子模块；将 Parley 的交互几何与分页行映射拆分。确认无用的 `caret-foundation` workspace dependency 别名并移除，crate 本身及 crash 能力保留；`cargo machete` 未发现其他未使用依赖。去除断行决策的重复 `Arc` 包装并统一 ICU 强制断行类别判断。
- 2026-10-29：修复异步保存结果仅校验代际时意外序列化整份文档的问题；改用轻量会话代际 FFI 查询。为 recovery-update 与冲突替换回调增加旧会话返回防护，并覆盖未领取新租约取消后的恢复项保留。

## 完成摘要

040 验收完成。未增加 crate；AutoSave/recovery 协调、跨层保存结果和主要模型/布局模块已按职责归位与拆分，复核与本机检查通过。Qt 窗口销毁情形仍缺直接自动化测试，后续如调整 worker/result 所有权应补覆盖。
