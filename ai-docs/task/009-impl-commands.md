# 009 — M1：`\` 命令

- 状态：done
- 依赖：008
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-29

## 目标与背景

用户用 `\` 改标题、列表和换页，并在插入点旁看见候选。这是需求的 M1，含输入法约束。通用命令入口在这里实现；后续结构任务可注册 EnvironmentNode 操作，沿用同一命令入口，不增加环境工具按钮。补全层直接在 QML 中实现和迭代，不保留 HTML 原型。

## 必读

- [命令输入](../architecture/editor/command-input.md)、[命令](../architecture/editor/commands.md)
- [快捷键](../architecture/interface/keyboard-shortcuts.md)、[界面外壳](../architecture/interface/ui-shell.md)

## 范围与非目标

实现 Rust Session 权威持有的命令查询、候选过滤/顺序/选择和 `cmdline.begin`、`cmdline.type`、`cmdline.accept`、`cmdline.cancel`。先接入已有编辑能力的换行命令。标题、段落样式和列表需要 010/025 提供语义目标后再接入；分页符目标尚未具备完整编辑命令，本任务不伪造。公式和表格别名留到 011、012，可以显示为尚未可用。
公式命令候选使用 `\math`、`\equation`、`\align`，分别对应行内、单行和多行结构；普通段落由直接输入形成，不注册 `\paragraph` 命令。

组字期间不进入命令模式。空格无效。Esc 取消并清掉查询。查询不改变文档 `rev`。

补全层只表达查询、高亮项和空结果，锚在插入点旁。候选面板采用双列，列宽随当前候选内容收缩、单列最大为 105 个逻辑像素；高亮项中超出列宽的文本自动循环滚动，不显示滚动条。滚至末尾停 1 秒，复位后停 1 秒再循环。候选多于可见行时纵向滚动，面板在视口底部空间不足时显示在查询框上方。命令输入背景与当前行等高，文字使用光标位置实际 Run 字号减 1 pt，并按画布缩放换算；命令光标跟随命令文字高度，长查询横向滚动以保持光标可见。不新做对话框，也不改 007 的窗口骨架。

命令查询/补全提示背景使用独立 GUI token `color-command-prompt`，与行内环境和块级环境的背景高亮使用不同颜色。此任务只接入命令提示 token；环境 token 分别由 031 实现。命令模式和环境高亮可同时显示，不互相覆盖状态或范围。

不包含：结构跳转、公式树和表格编辑。候选面板显示可在 Preferences 中关闭，默认开启；该设置只隐藏面板，不改变命令查询、键盘导航和执行行为。

## 前置条件与待决策

008 已完成，当前文档编辑、保存和会话能力足以支撑命令输入；其 Windows、macOS、Linux 合成 IME/Unicode Nightly 验收已通过。无未决的命令键位。

## 实施步骤

1. 在 QML 中实现插入点旁的补全层，并覆盖候选、空结果与超长候选展示状态。
2. 在会话里实现命令模式和别名匹配。匹配顺序按命令输入文档。补全层绑定 Rust 返回的真实候选。画布在命令模式认领按键，QML 只反映状态并转发用户输入。
3. 用真实输入法做一次组字检查。组字中的 `\` 不进入命令模式。
4. 验证现有可执行的段内换行；标题、列表、分页和数学结构待其目标任务实现后再做往返验收。
5. 独立评审后复验。

## 预计改动

补全 QML、QML 候选行与自动循环文本 Atom、Rust 命令状态/注册表、FFI 与画布按键分派。不改 007 的窗口骨架，除非补全层被它挡住。

## 验收标准

- [x] 补全层锚在插入点旁，能显示候选和空结果；宽度自适应，候选可滚动，空间不足时不遮挡输入光标。
- [x] Preferences 中可开关候选面板；关闭面板后命令查询与键盘执行仍可用。
- [x] 命令提示背景使用独立的 `color-command-prompt` 主题 token，支持默认、浅色和深色主题；不改变执行后环境节点的高亮色。
- [x] `\newline` 插入段内软换行，整条命令是一个撤销步；结构标题和列表候选在 010/025 提供语义目标后可执行。
- [x] Enter 执行当前候选；非空查询 Tab 补全当前候选别名但不执行，空查询 Tab 或重复前缀键插入字面 `\`；空格不执行也不插入。
- [x] Esc 取消且没有撤销步。组字期间不进入命令模式。
- [x] 命令模式查询不主动触发排版；fmt、跨语言构建、行为验证与独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-29 | `cargo build --locked -p caret-ffi` | 最新 Rust Session 与 FFI 生成静态库 | 通过 |
| 2026-10-29 | `cmake --build target/native/debug-coverage --target caret_native_app -j2` | Qt/QML 应用完整构建 | 通过；链接器提示托管 LLVM LTO 库缺失和静态库部署目标偏新，未阻止构建 |
| 2026-10-29 | `cmake --build target/native/debug-coverage --target caret_shell_qmllint -j2` | QML 静态检查通过 | 通过 |
| 2026-10-29 | `cargo fmt --check`、`git diff HEAD --check` | Rust 格式与完整待提交差异无空白错误 | 通过；真实 IME 与窗口验收未执行 |
| 2026-10-29 | `cargo check --locked -p caret-app -p caret-engine` | 命令快照与 Session 改动编译通过 | 通过 |
| 2026-10-29 | `cargo test --locked -p caret-engine failed_editor_command_keeps_the_active_command_query` | 编辑命令失败时命令查询仍保持活动 | 通过，1 项 |
| 2026-10-29 | `cargo test --locked -p caret-engine tab_completes_a_filtered_command_without_editing_the_document`、`accepting_the_newline_command_inserts_a_soft_break_and_closes_the_query` | Tab 补全无文档副作用；接受换行命令插入软换行并退出查询 | 通过，各 1 项 |
| 2026-10-29 | `cargo test --locked -p caret-app command_candidate_snapshot_omits_search_only_labels`、Qt `commandLineQueryProjectsRustCandidatesWithoutEditingTheDocument` | 候选快照不传搜索专用名称；Rust Session 状态经 FFI/Qt 投影，Tab 不改正文，错误和取消状态符合契约 | 通过，各 1 项 |
| 2026-10-29 | `cargo coverage` | Rust 产品 crate 全局函数/行覆盖率均至少 85% | 通过；函数 85.61%，行 89.66% |
| 2026-10-29 | Criterion 0.8.2 release 对照：旧快照结构（含候选 `label`）与当前结构；20 个样本、预热 1 秒、测量 2 秒 | 以 10 万字符文档上的命令查询快照比较序列化耗时和负载大小 | macOS arm64、Rust 1.99.0；100,499 个 Unicode 字符、500 段、5 个候选：旧版 538 B，耗时区间 3.0967–3.1275 μs（点估计 3.1092 μs）；当前 422 B，区间 2.6177–2.7090 μs（点估计 2.6540 μs）。负载减少 21.6%，微基准点估计耗时减少 14.6%（455 ns）。文档准备不计入计时；只衡量命令状态快照序列化，不代表整次按键、Qt 渲染或端到端延迟；未测分配次数。该对照为一次性测量，临时基准代码未保留 |
| 2026-10-29 | `cmake --build target/native/debug-coverage --target caret_shell_qmllint caret_native_app -j4` | QML lint 与 macOS 应用构建通过 | 通过；链接器有托管 LLVM LTO 文件缺失和静态库部署目标版本较新的警告 |
| 2026-10-29 | `cargo coverage native` | native CTest 与 C++ 函数/行覆盖率至少 85% | CTest 7/7 通过；函数 87.58% 达标，行 82.96% 未达标，native 覆盖率门槛尚未通过 |
| 2026-10-29 | `cargo coverage native`（补充 Canvas 按键分派、命令跨层执行与 Preferences 成功/失败行为测试后） | native CTest 与 C++ 函数/行覆盖率至少 85% | CTest 8/8 通过；函数 89.09%，行 85.06%，均达标。无 SettingsRuntime 的失败行为使用独立 CTest 进程，避免依赖共享测试顺序。结果报告位于 `target/native-coverage/` |
| 2026-10-29 | `cargo format --check`、`cargo test --locked --workspace` | 跨语言格式与 workspace 行为回归通过 | 通过；Rust 单元与集成测试、QML 检查和 CTest 全部通过 |
| 2026-10-29 | macOS 桌面应用手工验收 | 打开补全、关闭补全后继续用键盘输入和导航；确认命令执行结果 | 用户确认实测无问题；当前只有 `\newline` 已接入执行，其余结构、列表、分页、数学命令等待对应任务提供语义后接入 |
| 2026-10-29 | Qt `autoScrollTextMovesOnlyWhenActiveAndOverflowing`、`autoScrollTextRepeatsWithTheDocumentedPauseCycle` | 候选选中态驱动两列溢出文本滚动，取消选中后复位；到末尾停 1 秒、复位后停 1 秒再循环 | 通过；无头 Qt 测试 2 项，profile 输出位于 `target/native-coverage/profiles/` |
| 2026-10-29 | `git diff HEAD --check` | 完整待提交差异无空白错误 | 通过 |
| 2026-10-29 | 独立 subagent 完整差异评审（gpt-6.1-sol，medium，只读） | 检查状态一致性、重复数据、QML 自动滚动、文件注册和文档 | 前次发现的命令失败状态分叉、无用 catalog 热路径和 HTML 原型引用已修复；本轮发现的共享运行时测试顺序依赖改为独立进程验证后复审 |

## 风险与回退

当前命令执行范围限于 `\newline`；其他候选只提供查询与提示，不会伪装为已实现的命令。标题/环境、列表与分页、数学和表格能力分别由后续任务接入。命令入口若出现回归，可通过 Preferences 隐藏候选面板；这不会改变命令会话或编辑状态。

## 决策与工作记录

- 2026-10-25：创建任务。
- 2026-10-29：命令候选使用结构化名称；数学入口定为 `\math`、`\equation`、`\align`，普通段落不注册命令。候选面板显示开关归入 Rust Preferences，默认开启；关闭只隐藏面板。
- 2026-10-29：命令补全采用双列布局，列宽随当前候选收缩且单列最大为 105 个逻辑像素；仅当前高亮项的超长文本自动循环滚动，每轮末尾停 1 秒、复位后停 1 秒再继续。命令文字取光标处 Parley Run 字号减 1 pt 并随画布缩放。高亮共用 `HighlightBackground` 外观，命令提示的行高与水平留白仍由输入控件单独决定。

## 完成摘要

已接入 Rust 命令会话与候选注册、Tab 别名补全、轻量命令状态通道、Qt/QML 补全展示、主题色和候选显示首选项；命令候选行与自动循环文本已抽为 Atoms。命令键真实画布路由、IME 期间拒绝命令模式、空查询 Tab/重复前缀、空格无副作用、换行命令单步撤销重做，以及 Preferences 保存与运行时失败保留状态均有行为测试。用户已在 macOS 实际验收补全面板与键盘操作，并确认当前可执行命令仅为 `\newline`。`cargo format --check`、完整 workspace 测试、native CTest、qmllint、QML 格式检查和覆盖率门槛通过；Rust 全局覆盖率为函数 85.61%、行 89.66%，native 函数覆盖率为 89.09%、行 85.06%。10 万字符文档上的 Criterion 对照显示，命令快照序列化耗时点估计减少 14.6%（3.1092 μs 至 2.6540 μs），负载减少 21.6%（538 B 至 422 B）；这不代表端到端输入或渲染延迟，且未测分配次数。其他结构、列表、分页及数学命令等待其语义任务接入。
