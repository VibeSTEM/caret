# 071 — 宿主焦点协议与键盘意图路由

- 状态：planned
- 依赖：010、021、027、030、043、044、073
- 优先级：P1
- 创建 / 更新：2026-11-08 / 2026-11-08

## 目标与背景

统一正文、块环境、显示公式、行内环境与行内公式在宿主层的焦点进入/离开和边界行为，并明确 Qt 输入适配、应用视觉几何与 Rust 会话决策各自的职责。当前 app `document_view/navigation.rs` 同时选择数学/正文路径并判断环境退出；engine 的 MathEdit 又维护一套公式退出、删除和相邻块查找。生产代码中 `EnterMath` 是否接通、焦点相关状态是否重复，须在当前代码基线上逐项复核，避免沿用过期 issue 描述。

Issue：[#105 宿主焦点与环境协议](https://github.com/VibeSTEM/caret/issues/105)、[#107 键盘路由与 grid 边界](https://github.com/VibeSTEM/caret/issues/107)。本任务承接两者共享的焦点/导航架构；`caret-grid-layout` 保持当前窄几何职责，待 046/051 出现实际消费者后再评估是否扩展。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[分层](../standards/layering.md)、[Rust 分层](../standards/layering/rust.md)、[依赖图](../standards/layering/dependency-graphs.md)
- [数学公式架构](../architecture/editor/math.md)、[结构导航](../architecture/editor/structure-navigation.md)、[快捷键](../architecture/interface/keyboard-shortcuts.md)
- [010 结构导航](010-impl-structure.md)、[030 输入交互](030-ime-navigation-selection.md)、[043 公式模型与编辑](043-math-model-and-editing.md)、[044 公式布局与画布](044-math-layout-and-rendering.md)、[069 数学交互验收](069-math-interaction-and-platform-acceptance.md)

## 范围与非目标

- 先以当前代码和可复现行为确认不同焦点状态、导航入口、公式边界、块环境停靠、结构选区、删除与撤销恢复之间的真实差异；issue #105/#107 记录的是较早基线，不能直接视为现状。
- 不把焦点字段机械合成互斥状态：composition、command line 与 math edit 在当前实现中可共存；先建模合法状态组合、宿主锚点和迁移，再选择表示方式。
- 定义稳定的宿主层可进入对象/焦点目标契约，让 Rust 会话权威决定焦点目标分派、进入/离开、边界停靠、空对象删除与结构选择。实现可用 enum、显式命令结果或其他合适的 Rust 机制；不预设动态 trait 或把所有状态塞进单个 enum。
- 将 app 视觉导航缩为快照/几何查询、意图转交与交互状态投影。视觉移动几何仍由布局快照提供；Qt 只映射实体按键及修饰键，不持有环境语义。
- 在 engine session 层建立键盘意图路由的合适边界，覆盖方向键、Home/End、Shift 扩选、preferred x、视觉 affinity、Escape、Enter 等有焦点优先级的意图；正文与 MathTree 的内部移动算法留在各自领域模块。`session/keyboards/` 目前仅处理 `EditorKey`，是否扩展或使用相邻模块须等几何接口确定后再决定。IME 预编辑、generation/font-generation 校验仍要正确传播。
- 审核并合并重复的块路径、相邻对象和宿主边界查找；`structure` 与 `math_edit` 的现有查找支持不同节点类型和返回契约，只有统一类型、嵌套遍历和失败语义兼容后才合并，不为缩短大文件而迁移代码。
- 保留与 IME composition、command line 等正交的交互状态；只有证明确属互斥的编辑焦点才收敛为单一权威状态。消除冗余投影时保留可恢复的选择、撤销与焦点历史。
- 不将整套方向键算法搬进 keyboards，不把几何或平台映射放入 engine，也不为目录整洁新建 crate。
- 不扩展 `caret-grid-layout` 的职责；本任务只记录真实第二消费者出现时的评估触发条件，不把未来表格/列表接入算作已实现。

## 前置条件与待决策

- 010、021、027、030、043、044 已完成。
- 实施前复核 #105/#107 提及符号在 HEAD 上的调用点、行为测试及文档契约；列出确需改变的行为和兼容边界。
- 确认键盘意图取得布局几何的接口：显式传入快照/查询对象，或由 engine 请求调用方实现的窄接口。选依赖方向最直接且不复制页面状态的一种；保留布局 generation/font-generation 校验。
- 焦点目标的状态迁移需覆盖 composition、结构选区与公式编辑叠加时的所有权，不能把正交状态误合并。

## 实施步骤

1. 记录现状调用图与失败/边界行为；先补宿主方向键进入/离开公式、块环境和行内环境的行为用例。
2. 确定焦点与可进入对象契约及状态迁移表，明确 app、engine、布局快照和 Qt 的字段/职责。
3. 按依赖方向迁移路由，合并重复边界与块定位逻辑；保持公式树路径、槽位、文本岛和 Align 内部导航留在数学领域代码。
4. 覆盖正文、行内环境、InlineMath、Equation、Align、结构选区、Shift 扩展、Escape/Enter、空对象删除、撤销焦点恢复及失败原子性。
5. 更新数学/结构导航架构文档及 045/046/051/069 的依赖边界，完成分域审查和独立 final review 后复验。

## 预计改动

`crates/caret-engine/src/editing/session/`、`crates/caret-app/src/document_view/navigation.rs`、对应 Rust/Qt 行为测试、[数学公式架构](../architecture/editor/math.md)、[结构导航架构](../architecture/editor/structure-navigation.md) 及关联 task。预计不会改 Qt 按键语义映射，也不改 crate 依赖层级。

## 验收标准

- [ ] 对应 issue 证据在当前 HEAD 上已确认或注明过时；关键行为有可复现的成功、边界、拒绝和撤销恢复用例。
- [ ] 从相邻正文用方向键可进入 InlineMath、Equation、Align；在对象边界再按一次可返回正确宿主位置，扩选和视觉方向对称。
- [ ] Escape、Enter 等按键优先级有单一权威分派；公式上下文下不再由前置拦截绕过共享意图路由。
- [ ] app 不再承担环境退出、焦点类型选择或相邻结构语义判断；Qt 不实现业务路由；engine 不重算 Parley 页面几何。
- [ ] 公式内部 MathTree/源码导航语义仍独立；IME 与 command line 等正交状态未被错误合并，失败事务保留有效焦点、选区与文档。
- [ ] 空对象退格、Mod+Backspace、结构选区删除和撤销焦点恢复均有逐类明确的产品契约；不强制不同对象视觉/删除语义完全一致。
- [ ] 嵌套块环境中的非空 Equation/Align 不会被误判为空而随整个环境删除；相关修复和回归由 073 完成并通过。
- [ ] 契约相同的重复块路径/边界定位合并或记录保留原因；未引入无消费者抽象、L3 同级依赖或只为拆分文件的新 crate。
- [ ] `caret-grid-layout` 仍只做已测量网格的纯几何；如尚无第二个生产消费者，不新增其 API 或依赖。
- [ ] Rust 与 Qt 直接消费者行为、格式、lint、覆盖率范围及独立评审证据通过并记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | engine session navigation/keyboard 行为测试 | 多焦点目标进入、离开和失败状态一致 | 未执行 |
| — | app navigation 与 Qt 输入行为测试 | app 仅转交意图，Qt 仅适配按键 | 未执行 |
| — | `cargo format --check`、受影响 crate clippy/test、`cargo lint --check` | 受影响 Rust/native/QML 边界通过 | 未执行 |

## 风险与回退

焦点目标与正文 `Selection`、MathEdit 内部位置是不同坐标和状态；合并时若把外层焦点误当成内部 caret，会造成输入、撤销和边界导航失配。按模块/行为分步提交，出现回归时先恢复原路由并保留新增黑盒用例。

## 决策与工作记录

- 2026-11-08：通过 `gh issue view` 审查 #105/#107；确认宿主路由的 app/engine 重复边界仍在 HEAD，task 040 已完成的 AutoSave/FFI 与 QML 清理不重开。确认 068 已建立的 grid 几何边界正确，数学 table adapter 已使用共享网格线坐标；不因“偏薄”增加 crate 职责，也不将“两个消费者”提升为通用 crate 规则。
- 2026-11-08：分域静态审查发现 `environment_has_content` 忽略 Equation/Align，可能造成含公式块环境被退格整体删除；安全修复拆为 073，并置于本架构重构前。
- 2026-11-08：创建任务承接共享焦点和键盘意图边界；数学文本岛的 bidi/拖选和平台验收继续由 069 负责。

## 完成摘要

未完成。
