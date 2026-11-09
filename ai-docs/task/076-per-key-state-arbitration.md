# 076 — 逐键状态仲裁与键盘路由一致性

- 状态：planned
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

- [ ] 每个已支持编辑器键的状态优先级能直接从其逐键 handler 阅读，且测试覆盖优先状态共存时获胜者与文档/历史副作用。
- [ ] `keyboards.rs` 不含隐藏所有按键差异的通用状态优先级分支；保留的共享 helper 只执行可证明相同的校验/提交操作。
- [ ] 所有键盘入口旁路均已接入对应 handler，或有受测的焦点作用域/平台例外与明确文档。
- [ ] MathText、MathTree、Source、命令行、IME、结构选区和正文中的剪贴板/Undo/Redo 行为符合用户契约；注释准确反映实现。
- [ ] 各目标平台的 Latin 输入法提示有实测记录；MathTree 中中文标点/Unicode 符号输入与缺字 fallback 可发现且可恢复。
- [ ] 失败与过时代际候选不更改焦点、选区、文档、composition 或历史；格式、相关测试、lint 和独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 逐键状态优先级行为测试 | 每键冲突按契约仲裁，失败不产生副作用 | 未执行 |
| — | app/native 键盘入口集成测试 | 无意外旁路、事件不重复消费 | 未执行 |
| — | 格式/lint 与独立 review | 跨层改动符合门禁和架构边界 | 未执行 |

## 风险与回退

逐键拆分可能改变过去隐含在通用 router 中的状态顺序。以现有用户可观察行为和新增优先级用例为迁移依据；若某例外暂不能统一，保留其已验证行为并记录在唯一架构文档，不能用通用优先级悄悄覆盖。

## 决策与工作记录

- 2026-11-08：只读审查发现多个方向/编辑键仍通过通用 router 管理状态；字符输入、字体候选、MathText 边缘及过期数学焦点也有特殊入口。根据用户设计新建独立架构任务，不修改已 done 的 071。
- 2026-11-08：独立评审补充发现 MathTree 输入法仍禁用、MathText 数学命令行的 IME enabled 状态与 Session 契约冲突，且 `preferLatin` 未在完整快照/交互增量统一校验；将这些非上下键问题纳入本任务。
- 2026-11-09：接收 074 移交的平台输入法提示验证、数学结构中的中文标点/Unicode 符号输入和缺字 fallback 验收；074 因此只关闭已有实现与测试覆盖的结构光标范围。

## 完成摘要

未完成。
