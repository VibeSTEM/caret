# 071 — 宿主焦点协议与键盘意图路由

- 状态：done
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
- 不把焦点字段机械合成互斥状态：已确认 MathEdit 可与 MathText composition 共存，也可与 command line 共存；其他状态组合逐项确认允许/拒绝契约，不推断为任意组合均可共存。先建模合法状态组合、宿主锚点和迁移，再选择表示方式。
- 定义稳定的宿主层可进入对象/焦点目标契约，让 Rust 会话权威决定焦点目标分派、进入/离开、边界停靠、空对象删除与结构选择。实现可用 enum、显式命令结果或其他合适的 Rust 机制；不预设动态 trait 或把所有状态塞进单个 enum。
- 将 app 视觉导航缩为快照/几何查询、意图转交与交互状态投影。视觉移动几何仍由布局快照提供；Qt 只映射实体按键及修饰键，不持有环境语义。
- `keyboard-shortcuts.md` 中所有当前已支持的画布/编辑器快捷键都由 `crates/caret-engine/src/editing/session/keyboards/` 管理，包括命令前缀、Backspace、方向键、Home/End、Tab/Enter/Escape、结构快捷键、Mod+C/V、Mod+Z/Redo 和新增 `$` 数学模式入口。按实体键或语义独立快捷键家族拆文件：只改变扩选/修饰状态的变体留在基础键文件，Mod+Backspace、Mod+Z 等独立语义可有单独文件；共享入口只做按键分派。新编辑器快捷键按相同规则扩展。窗口级打开/保存/缩放仍由窗口焦点作用域处理，不进入编辑器 Session。不要新建按上下文命名的 `math.rs` 路由模块，也不要让 `math_edit.rs`、app、QML 各自仲裁编辑器快捷键。
- app 持有并计算页面布局模型及视觉几何，只把单次视觉候选、preferred x 或 visual affinity 等必要标量作为意图数据交给具体键处理器；Session 校验候选、裁决当前层级焦点并提交状态迁移。页面布局模型、几何计算及其持久状态不进入 engine。IME 预编辑、generation/font-generation 校验仍要正确传播；剪贴板 IO 仍由 app 适配，Mod+C/Mod+V 的会话语义和上下文判断由对应按键模块负责。
- 审核并合并重复的块路径、相邻对象和宿主边界查找；`structure` 与 `math_edit` 的现有查找支持不同节点类型和返回契约，只有统一类型、嵌套遍历和失败语义兼容后才合并，不为缩短大文件而迁移代码。
- 保留与 IME composition、command line 等正交的交互状态；只有证明确属互斥的编辑焦点才收敛为单一权威状态。消除冗余投影时保留可恢复的选择、撤销与焦点历史。
- 不将整套方向键几何算法搬进 keyboards；页面布局模型、几何计算和持久化状态归 app，单次几何候选与必要标量可作为意图输入 engine。平台键映射归 Qt；不为目录整洁新建 crate。
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

`crates/caret-engine/src/editing/session/`、`crates/caret-app/src/document_view/navigation.rs`、对应 Rust/Qt 行为测试、[数学公式架构](../architecture/editor/math.md)、[结构导航架构](../architecture/editor/structure-navigation.md) 及关联 task。不改变 crate 依赖层级；Qt 只映射实体键和平台修饰键，不持有编辑状态优先级。

## 验收标准

- [x] 对应 issue 证据在当前 HEAD 上已确认或注明过时；关键行为有可复现的成功、边界、拒绝和撤销恢复用例。
- [x] 对照唯一键位清单迁移所有当前已支持的画布快捷键；对应处理器按实体键或语义独立快捷键家族放在 `session/keyboards/`，包括必要的修饰变体；不在 `math_edit.rs`、app 或 Qt 留编辑器快捷键优先级分支。尚未实现的表格键只登记目标文件，实际功能留给 045/046。
- [x] `$` 字符在正文插入点创建并进入空行内公式；处理器按语义字符命名为 `dollar.rs`，不绑定 `Shift+4` 等物理键位；`、`、`¥` 和 `￥` 保持普通文本输入。
- [x] `keyboard-shortcuts.md` 是实际键位清单的唯一权威；`keyboard-shortcut-architecture.md` 是逐键模块和优先级架构的唯一权威；`keyboard-routing.md` 仅作入口，不复制清单或行为矩阵。
- [x] 逐键文件能独立表达不同上下文层级；命令输入、IME composition、结构选区、MathTree/MathText/MathSource、块环境退出和正文根据各键规则优先处理，正交状态不被误合并。
- [x] 从相邻正文用方向键可进入 InlineMath、Equation、Align；在对象边界再按一次可返回正确宿主位置，扩选和视觉方向对称。
- [x] Escape、Enter 等按键优先级有单一权威分派；公式上下文下不再由前置拦截绕过共享意图路由。
- [x] app 只负责布局候选、快照及 effect 投影，不再承担环境退出或相邻结构语义判断；Qt 不实现业务路由；engine 不重算 Parley 页面几何。
- [x] 公式内部 MathTree/源码导航语义仍独立；IME 与 command line 等正交状态未被错误合并，失败事务保留有效焦点、选区与文档。
- [x] 空对象退格按 InlineMath、Equation、Align cell、MathText 文本岛、空构造参数、块环境和普通段落分别仲裁；Mod+Backspace、结构选区 Backspace/Delete 与撤销/重做焦点恢复有明确契约和回归用例，不强制不同对象视觉/删除语义完全一致。
- [x] 嵌套块环境中的非空 Equation/Align 不会被误判为空而随整个环境删除；相关修复和回归由 073 完成并通过。
- [x] 契约相同的重复块路径/边界定位合并或记录保留原因；未引入无消费者抽象、L3 同级依赖或只为拆分文件的新 crate。
- [x] `caret-grid-layout` 仍只做已测量网格的纯几何；如尚无第二个生产消费者，不新增其 API 或依赖。
- [x] Rust 与 Qt 直接消费者行为、格式、lint、覆盖率范围及独立评审证据通过并记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-08 | `cargo test --locked -p caret-engine` | 导航/键盘状态、失败原子性及公式焦点语义 | 654 passed |
| 2026-11-08 | `cargo test --locked -p caret-app` | app 候选生成、宿主焦点进入/离开及响应投影 | 255 passed |
| 2026-11-08 | `cargo test --locked --workspace` | 全部 Rust crate、公开消费者、native/QML 聚合 | passed；CTest 10/10，QML 格式通过 |
| 2026-11-08 | `cargo format`、`cargo lint` | Rust/C++/QML 格式、Clippy、依赖/分层、native 静态检查和 qmllint | format passed；lint 9/9 阶段通过，包括 Clippy、unused dependencies、CMake、分层、Qt/QML/clang-tidy/include-cleaner/cppcheck |
| 2026-11-08 | `cargo coverage`、`cargo coverage native` | Rust/native 产品覆盖率函数与行门槛 | Rust 85.10% 函数 / 89.05% 行；native 产品 90.44% 函数 / 85.64% 行；门槛通过 |
| 2026-11-08 | `cargo deny check bans licenses sources`（临时 `CARGO_HOME`） | 非 advisory 依赖策略 | 通过；配置中未遇到的 license allowlist 有既存 warning |
| 2026-11-08 | `cargo quality` advisory 阶段 | RustSec 数据库漏洞检查 | 未能完成：沙箱不能写宿主 `~/.cargo/advisory-dbs/db.lock`；临时 Cargo home 下数据库需要联网更新，但环境无法解析 github.com |
| 2026-11-08 | 分域复审与 Sol/medium 最终只读评审 | 领域边界审查及完整 diff 复核 | 分域发现已修复；最终只读复审未发现阻断问题 |
| 2026-11-08 | `cargo test --locked -p caret-engine editing::session::structure::tests` | 结构选区两种删除键、Mod+Backspace 不误操作、撤销恢复焦点 | 37 passed |
| 2026-11-08 | `cargo test --locked -p caret-app backspace_on_keyboard_selected_environment_deletes_and_restores_its_subtree` | Qt 编辑器按键入口的 Backspace、Mod+Z/Redo 与 caret/selection 投影 | 1 passed |
| 2026-11-08 | `cargo test --locked -p caret-app` | app 全部直接消费者与生产键盘入口 | 255 passed |
| 2026-11-08 | `cargo clippy --locked -p caret-engine -p caret-app --all-targets -- -D warnings`、`cargo fmt --check`、`git diff --check` | 受影响 crate lint 与格式/空白检查 | 通过 |
| 2026-11-08 | `gpt-6.1-sol` / medium 独立只读 final review，main 到本分支最终工作区完整 diff | 状态迁移、边界分层、回归覆盖和文档一致性 | 未发现需修复问题；未运行验证，实际窗口绘制及平台实体键未覆盖 |

## 风险与回退

焦点目标与正文 `Selection`、MathEdit 内部位置是不同坐标和状态；合并时若把外层焦点误当成内部 caret，会造成输入、撤销和边界导航失配。按模块/行为分步提交，出现回归时先恢复原路由并保留新增黑盒用例。

## 决策与工作记录

- 2026-11-08：依赖 010、021、027、030、043、044、073 均已完成；从更新后的 main 新建 `yuki/task-071-host-focus-key-routing` 开始核对当前代码与行为。
- 2026-11-08：通过 `gh issue view` 审查 #105/#107；确认宿主路由的 app/engine 重复边界仍在 HEAD，task 040 已完成的 AutoSave/FFI 与 QML 清理不重开。确认 068 已建立的 grid 几何边界正确，数学 table adapter 已使用共享网格线坐标；不因“偏薄”增加 crate 职责，也不将“两个消费者”提升为通用 crate 规则。
- 2026-11-08：分域静态审查发现 `environment_has_content` 忽略 Equation/Align，可能造成含公式块环境被退格整体删除；安全修复拆为 073，并置于本架构重构前。
- 2026-11-08：结构删除明确支持 Backspace/Delete；结构选区期间 Mod+Backspace 保持无操作，避免使用其保存的普通文字光标解除其他环境。Undo 恢复操作前的普通选择和视觉 affinity，并清除失效结构高亮；Redo 恢复删除后焦点。逐类契约已补到环境模型与数学架构文档。
- 2026-11-08：全量 engine 验证发现 Undo/Redo 原先在历史查询前清理结构选区；把历史命令移到该清理之前，无历史时现可保留选区与文档状态。
- 2026-11-08：创建任务承接共享焦点和键盘意图边界；数学文本岛的 bidi/拖选和平台验收继续由 069 负责。
- 2026-11-08：维护者明确所有当前支持的编辑器快捷键须统一按实体键或语义独立快捷键家族在 `session/keyboards/` 管理；覆盖四向键、Home/End、Delete/Backspace、Mod+C/V、Mod+Z/Redo、命令前缀和结构修饰键。表格快捷键仍按 045/046 计划状态记录，不提前实现。新增 `$` 输入行为在正文创建并进入空行内公式，以 `dollar.rs` 管理；`、` / `¥` / `￥` 保持普通文本。普通文本通过 `Command::TextInput` 交给 Session 选定命令行、前缀或正文目标；QML 不检查命令行活动状态。文档权威分工为键位清单 `keyboard-shortcuts.md`、管理架构 `keyboard-shortcut-architecture.md`、路由入口 `keyboard-routing.md`。架构文档先单独提交，再进行代码迁移。

## 完成摘要

宿主焦点目标、键盘意图与快捷键状态在 Rust Session 中统一仲裁；app 只传递布局候选并发布交互投影，Qt/QML 不复制编辑业务规则。块环境、数学对象、结构选区及撤销焦点契约已分别记录在权威架构文档，相关行为测试、独立评审和受影响 crate 验证通过。
