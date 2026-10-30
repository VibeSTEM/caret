# 059 — 命令字体选择器交互与布局优化

- 状态：planned
- 依赖：024、025
- 优先级：P2
- 创建 / 更新：2026-10-30 / 2026-10-30

## 目标与背景

025 已为命令输入中的 `\font` 和 `\defaultfont` 提供字体候选弹窗基础实现。两者共用命令输入入口和弹窗外框，但候选语义不同：`\font` 查询本机字体族并应用局部 Font 环境；`\defaultfont` 按 Script 与 Script+Language 层级修改文档默认字体。现有交互与尺寸仍需整体复核，`\defaultfont` 的层级选择展示也需要进一步完善。

本任务专注命令输入场景，不设计或实现 Settings 中的默认字体界面；后者由[056](056-font-settings-and-family-picker.md)负责，并等待独立布局设计。字体族身份、目标映射、存储与回退规则以[字体选择规范](../architecture/editor/font-selection.md)为唯一权威。

## 必读

- [格式命令](025-text-formatting.md)、[字体设置专用布局](056-font-settings-and-family-picker.md)
- [字体选择规范](../architecture/editor/font-selection.md)、[命令输入与键盘路由](../architecture/interface/keyboard-routing.md)、[画布高亮](../architecture/interface/canvas-highlighting.md)
- [QML 规范](../standards/qml.md)、[分层规范](../standards/layering.md)、[测试](../standards/testing.md)

## 范围与非目标

- 复核并完善 `\font`、`\defaultfont` 命令模式下的候选展示、层级提示、选中态、查询输入、关闭/返回、无结果和错误状态；实现前先记录可观察行为设计。
- 保持 `\font` 与 `\defaultfont` 的候选语义清楚。两者可复用通用 popup frame 和原子行组件，但不因共用布局而合并成含大量模式分支的单体组件。
- 优化字体候选弹窗尺寸、可见候选数量、截断/溢出、滚动或翻页行为、插入点锚定和可用画布边界适配；具体策略以交互设计和可验证行为为准。
- Rust `Session` 持有命令查询、候选和层级状态；字体目录仍复用 024 Fontique catalog/generation。QML 负责渲染与转发意图，不扫描字体、不重新实现过滤、目标映射或提交逻辑。
- 保持 `\font` 接受后按既有事务创建/应用 Font 环境；`\defaultfont` 选定目标字体后经既有文档设置命令/API 提交并保持选择器可继续修改，直至显式关闭。
- 不实现通用 Settings UI、系统字体安装/管理、字体文件分发、数学字体切换或第二套字体目录；不改变 `.cdoc` 字段和字体回退语义。

## 前置条件与待决策

- 025 当前命令模型、字体候选查询和文档字体写入 API 稳定；若命令后端有缺陷，应在对应 Rust Session/命令模块修复，而不是在 QML 补第二份状态。
- 实施前人工/自动梳理当前交互，对 `\font` 查询和接受、`\defaultfont` Script/Language 层级进入/返回/选择、Escape/关闭、IME、无匹配和目录刷新建立明确状态表。
- 与用户确认涉及新产品行为的视觉/键盘设计；纯尺寸和组件边界选择可依现有 QML 规范自主决定，并记录依据。
- 弹窗根据字体名称长度和画布可用空间裁剪时，必须明确宽度上限、滚动/截断规则及焦点归属；不得隐藏全量系统字体来测量宽度。

## 实施步骤

1. 审核 `FontPickerPopup.qml`、`CommandPopupFrame.qml`、候选 DTO 与 Session 行为，整理当前状态、重复实现、硬编码尺寸和失败路径。
2. 编写简短交互规格和命令状态表，区分字体族搜索与默认字体目标层级；覆盖候选激活、关闭、返回、空结果、IME 和字体目录 generation 更新。
3. 根据规格精简或拆分 QML 展示职责；复用公共 frame/row 组件，不让共享组件承担字体业务策略。
4. 按需修正 Rust 候选/命令状态与 FFI 投影，使 QML 只接收足够且稳定的呈现 DTO；旧 generation 的异步结果不能覆盖当前候选。
5. 为命令可观察行为补 Rust 测试，为弹窗焦点、键盘路由、锚定/裁剪与候选状态补 Qt/QML 行为测试；覆盖大字体目录时确认键入不触发逐项全量测量或文件 I/O。
6. 同步字体选择架构说明、任务状态与所有支持语言翻译，按验证规范完成独立评审和复验。

## 预计改动

`qml/Canvas/FontPickerPopup.qml`、`qml/Canvas/CommandPopupFrame.qml`、`qml/Canvas/CommandCompletion.qml`、`qml/Components/Atoms/CommandCandidateItem.qml`，必要时修改 `crates/caret-engine/src/editing/command_line/`、Session 快照/FFI/Qt Canvas 命令输入投影，以及 QML 行为测试和[字体选择规范](../architecture/editor/font-selection.md)。以审核结果为准，不预先拆 crate。

## 验收标准

- [ ] `\font` 和 `\defaultfont` 的状态、候选语义、键盘行为、关闭/返回和错误状态有清晰行为规格，彼此没有重复或冲突状态。
- [ ] `\font` 仅查询可用本机字体族，接受后对当前选区/插入点按 025 既定规则应用局部 Font 环境。
- [ ] `\defaultfont` 按既定 Script 与 Script+Language 层级工作；选择后提交当前文档设置、返回可继续编辑的层级，显式关闭后才退出命令选择流程。
- [ ] 弹窗在不同候选数量、长字体名、语言变化和可用画布尺寸下布局可读；无结果、截断/滚动、锚定与边缘适配符合规格。
- [ ] 键盘、IME、Escape 和画布焦点交接不会让一次按键触发两种操作；命令行光标位置/查询与候选 selection 保持同步。
- [ ] Rust 持有权威选择状态与字体目录 generation；QML 不扫描目录、不计算字体覆盖或写文档设置。
- [ ] 输入热路径没有隐藏全量候选测量、重复目录查询或过期异步结果安装；如宣称性能改善，提供代表性字体目录和可复现测量证据。
- [ ] 所有支持 locale 的静态界面文案已翻译；字体名按系统数据原样显示。
- [ ] Rust 与 Qt/QML 行为测试覆盖主要成功、失败、取消、IME 和 generation 边界；独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Rust 命令/Session 定向测试 | 搜索、层级、应用、关闭和 generation 行为符合规格 | 未执行 |
| — | Qt/QML 加载与交互测试 | 锚定、焦点、滚动/裁剪、键盘和空状态正确 | 未执行 |
| — | 大字体目录输入基准或分配测量 | 输入候选时无全目录重复扫描/隐藏委托测量 | 未执行 |
| — | 独立 final review | 分层、复用、状态同步、边界和性能证据通过 | 未执行 |

## 风险与回退

弹窗行为横跨 Rust 命令状态、FFI 快照、Qt 输入转发和 QML 布局。若只修视觉而未修权威状态，可能掩盖导航/IME 问题；若共享组件引入字体业务分支，则会增加后续普通命令弹窗的耦合。变更须保留单一状态源，且可按候选 DTO、交互组件和布局策略分组回退。

## 决策与工作记录

- 2026-10-30：将命令中的字体选择器优化与 Settings 文档字体 UI 分拆；本任务只负责 `\font` / `\defaultfont` 命令交互。
- 2026-10-30：保留共享弹窗外框/基础候选行的复用；字体专属尺寸、查询和目标层级留在字体弹窗/ Rust Session，不扩散到普通命令补全组件。

## 完成摘要

未完成。
