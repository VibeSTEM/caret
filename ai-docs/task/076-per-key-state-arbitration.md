# 076 — 逐键状态仲裁与键盘路由一致性

- 状态：in-progress
- 依赖：071
- 优先级：P1
- 创建 / 更新：2026-11-08 / 2026-11-09

## 目标与背景

`Session::run_editor_key` 已是实体编辑器键的入口，但各具体键文件管理状态优先级的程度不一致。Left/Right/Up/Down/Home/End 把主要仲裁交给通用 `route_navigation_key`；Backspace/Delete/Mod+Z 的选择又分散在键文件、`session.rs` 与 `math_edit.rs`。普通字符/命令前缀、字体候选 Enter/Tab 与 MathText 视觉边缘还存在直接路由路径。因此“使用统一入口”并不等于“每个键的状态顺序在其逐键模块中可读、可维护”。

本任务按 [071 宿主焦点协议](071-host-focus-and-key-routing.md) 和[快捷键管理架构](../architecture/interface/keyboard-shortcut-architecture.md)，让每个实际快捷键文件明确拥有其按键在 composition、命令输入、公式/文本、结构选区和正文之间的优先级；共用代码只保留有真实重复的候选校验、事务与领域操作，不用单一全能路由器抹平按键差异。已完成的 071 不回填未完成项。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[分层规范](../standards/layering.md)、[注释规范](../standards/comments.md)、[快捷键管理架构](../architecture/interface/keyboard-shortcut-architecture.md)、[键盘路由](../architecture/interface/keyboard-routing.md)、[071](071-host-focus-and-key-routing.md)

## 范围与非目标

- 逐个审查已支持的逐键文件：Escape、Enter、Tab、Backspace、Delete、Backslash、Dollar、方向键、Home/End、Mod+Backspace、Mod+C/V/Z、Mod+Space；明确不同键的优先级，不假设统一顺序。
- 将方向键、Home/End 的状态优先级从通用 router 移入对应具体键模块；只共享候选代际检查、无损提交等可证明相同的逻辑。
- 核对 `TextInput`、反斜线/美元前缀、MathText 视觉边缘、字体候选 Enter/Tab、过期 MathEdit Escape 等绕过逐键入口的路径；决定将其接入具体 handler 或记录明确的适配层例外。
- 核对复制/粘贴与撤销/重做在 MathText、MathTree、Source、command line、composition、结构选区和正文下的实际契约；同步注释与架构文档，禁止仅凭旧注释改变用户行为。
- 明确公式输入法范围：当前 MathTree/Source 禁止 composition，`ImhPreferLatin` 仅适用于允许 IME 的文本状态且不是 OS 锁定；检查 MathText 数学命令行的 `ImEnabled` 与 Session 拒绝 composition 是否一致。
- 接续 074 移交的平台输入体验验收：验证锁定 Qt 6.12 上 Latin 输入法提示在各目标平台的实际效果，并明确不能由应用保证切换 OS 输入法；确认数学结构中的中文标点/Unicode 符号如何显式输入、缺字时的可见反馈与恢复路径。
- 校验新增输入法上下文字段在完整布局快照和交互增量中的类型与缺失策略；覆盖 Rust→ViewModel→Canvas 的 true/false 状态更新、焦点切换和 `Qt::ImHints` 通知。
- 不将窗口 Shortcut 或控件本地快捷键塞入 Session；不增加静态快捷键运行时注册表；不重开已 done 的 071。

## 前置条件与待决策

- 074 的 Up/Down 分数与 MathTree 几何候选先按各自键文件的状态优先级实现；本任务迁移其他逐键仲裁与入口例外。
- 从现有用户可观察行为、逐键测试和领域事务确定每个键的顺序；冲突未决时先写测试和行为契约，不凭通用路由器当前顺序猜测。
- 通用辅助函数的边界需由调用点相同的不变量证明；如果不同按键只共享候选验证，不共享焦点状态决策，就只共享候选验证。
- 074 的结构几何与上下键行为由该任务负责；IME hint 契约、命令行状态不一致和跨层字段校验属于本任务，不能以 hint 代替结构化 IME composition 协议。

## 实施步骤

1. 按每个实体键绘制事件来源、handler、状态优先级、领域命令和消费边界；将所有旁路一并登记。
2. 先为有状态冲突的每个键添加可观察行为与失败原子性用例，再按文件迁移状态选择。
3. 把通用 router 收缩为单一分派与确实共用的候选/提交辅助；清理过期注释和重复状态分支。
4. 更新键盘管理架构中的实现现状、例外和目标状态表，修正“一致入口即一致仲裁”的错误结论。
5. 完成分域检查与独立 final review，复验逐键回归、native/QML 路由和失败后的焦点、选区、文档及历史不变性。

## 预计改动

`crates/caret-engine/src/editing/session/keyboards/`、`crates/caret-engine/src/editing/session/keyboards.rs`、必要的 `session.rs` / `command_line.rs` 最窄入口、`crates/caret-app/src/document_view/navigation.rs`、对应 Rust/native 测试、[快捷键管理架构](../architecture/interface/keyboard-shortcut-architecture.md)。不应改动窗口 Shortcut 或单纯搬移领域业务事务。

## 验收标准

- [x] 已支持键的优先级可从逐键 handler 阅读；Backspace/Delete 先处理 composition/命令行焦点，再进入共享 `Command::Delete` 领域事务，由该事务统一处理选区、结构边界、公式和正文删除。命令行与 MathEdit 共存、composition、文档副作用均有直接按键行为用例。
- [x] `keyboards.rs` 不再包含通用导航状态路由；共享 helper 只处理重复的候选校验/提交，`TextInput` 适配也已归入 `keyboards/helpers/`。
- [x] 按键与宿主文本适配公开入口均经过 Session 事务包装；命令行前缀、美元入口、字体候选、Undo/Redo 和导航候选不绕过代际校验及成功后的状态同步。
- [x] MathText、MathTree、Source、命令行、IME、结构选区和正文中的剪贴板/Undo/Redo 路由已核对并保留现有领域事务；选区锚点与焦点视觉侧由 Session 持有并随撤销/重做恢复。
- [ ] 各目标平台的 Latin 输入法提示有实测记录；MathTree 中中文标点/Unicode 符号输入与缺字 fallback 可发现且可恢复。
- [x] 失败与过时代际候选不更改焦点、选区、文档、composition 或历史；撤销/重做恢复正文选区两端视觉侧；Rust/native 测试、格式、Clippy 和独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-09 | `cargo test -p caret-engine --locked`；`cargo test -p caret-app --locked` | 逐键状态冲突、数学/结构状态、选区历史、IME 和 app 投影回归 | 676 engine 与 271 app 测试通过；含 Backspace/Delete 的命令行+MathEdit、composition 优先级、连续 Shift 跨块锚点视觉侧和撤销软换行回归 |
| 2026-11-09 | `cargo test -p caret-tests --test native --locked` | 完整快照、ViewModel→QML Canvas、C++/QML、格式及窗口装载 | 10/10 CTest 通过；输入法状态更新和 Qt `ImHints` true/false 查询通过；新增字段 schema 验证通过 |
| 2026-11-09 | `cargo fmt --all -- --check`；`cargo clippy --locked --workspace --all-targets -- -D warnings` | Rust 格式与 workspace 静态检查 | 两项通过；Clippy 发现并修复 ModZ 条件结构与测试中的字符串比较分配后复跑通过 |
| 2026-11-09 | Terra 分域评审 + Sol 最终只读评审 | engine/session、app/native/IME、文档及完整最终 diff | Terra 分域复审与 Sol 最新 diff 复审均通过；已按其结构锚点发现补回归并通过；审查指出的实机输入法/Unicode 验收仍待目标设备确认 |

## 风险与回退

逐键拆分可能改变过去隐含在通用 router 中的状态顺序。以现有用户可观察行为和新增优先级用例为迁移依据；若某例外暂不能统一，保留其已验证行为并记录在唯一架构文档，不能用通用优先级悄悄覆盖。

## 决策与工作记录

- 2026-11-08：只读审查发现多个方向/编辑键仍通过通用 router 管理状态；字符输入、字体候选、MathText 边缘及过期数学焦点也有特殊入口。根据用户设计新建独立架构任务，不修改已 done 的 071。
- 2026-11-08：独立评审补充发现 MathTree 输入法仍禁用、MathText 数学命令行的 IME enabled 状态与 Session 契约冲突，且 `preferLatin` 未在完整快照/交互增量统一校验；将这些非上下键问题纳入本任务。
- 2026-11-09：接收 074 移交的平台输入法提示验证、数学结构中的中文标点/Unicode 符号输入和缺字 fallback 验收；074 因此只关闭已有实现与测试覆盖的结构光标范围。
- 2026-11-09：从已同步的 `main` 创建 `yuki/task-076-per-key-state-arbitration` 开始实施；先审计逐键 handler 与实际入口，再按状态冲突补行为测试和迁移。
- 2026-11-09：Left/Right/Home/End 已各自在对应 handler 管理 composition、命令输入、MathEdit 与正文候选；候选提交/代际检查集中在 `keyboards/helpers/navigation.rs`。Shift+Left/Right 在 MathText 两端保持公式焦点，不再沿无候选路径越出文本流。
- 2026-11-09：命令前缀与宿主字符输入经 `keyboards/helpers/text_input.rs` 适配并委托给具体按键 handler；字体目录闭包仅由 Enter/Tab handler 在处理字体候选时调用。数学命令查询与 MathText 同时活动时，快照正确报告 IME disabled，MathText 离开命令查询后恢复 Latin hint；完整快照和交互增量都校验 `preferLatin` 类型。
- 2026-11-09：Terra 光标状态审计后修复嵌套块虚拟退出仍显示父环境的问题，并抽取重复的 inline span 边界包含判断。选区锚点和焦点视觉侧已改由 Session 持有并进入历史状态，移除 app 侧可变副本；新增软换行撤销回归用例覆盖原错误。
- 2026-11-09：按用户确认创建后续任务 [077](077-stable-math-caret-height.md)，处理全部数学环境中稳定光标高度；不扩入本任务的键盘仲裁范围。
- 2026-11-09：分域评审发现公开 lazy 按键/文本入口绕过 Session 事务、嵌套块退出的矩形高亮遗漏及结构历史锚点捕获不完整。统一入口现在通过共享事务包装处理过时代际、恢复和状态同步；结构历史显式保存原锚点视觉侧；块高亮排除退出节点并保留父块，完整快照与交互增量测试逐坐标比对父块矩形，根块退出仍为空。
- 2026-11-09：Backspace/Delete 的 handler 明确仲裁 composition 和命令行后委托共享 Delete 领域事务；架构行为表已记载这一边界，新增直接 EditorKey 冲突用例覆盖命令行与 MathEdit 共存及 composition 不变性。
- 2026-11-09：新增 native 用例验证输入法 `enabled` / `preferLatin` 从 ViewModel 经真实 QML 属性绑定到 Canvas、通知发出与 Qt 查询；Rust app 用例验证状态值源自 Session。目标平台实际输入法切换效果、数学模式下 Unicode/中文标点与缺字恢复仍需目标设备实测，不能由 Qt 查询测试替代。
- 2026-11-09：Sol 最终评审发现键盘扩展跨块时结构选区转换覆盖锚点视觉侧，且折叠的正文投影会被事务收尾误判为无选区。Session 现在在进入及连续扩展时独立保留结构锚点侧，新增两次 Shift+Right 跨块候选的 EditorKey 回归。
- 2026-11-09：本轮验证完成：engine 676/676、app 271/271；native CTest 10/10；格式与 workspace Clippy 通过。两次 Terra 复审及最终 Sol 复审的发现均已处理并复查。目标平台手动输入法与缺字恢复验收仍待完成。

## 完成摘要

未完成。
