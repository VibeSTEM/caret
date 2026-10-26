# 030 — IME、键盘导航与选区交互

- 状态：ready
- 依赖：022、027
- 优先级：P2
- 创建 / 更新：2026-10-26 / 2026-10-26

## 目标与背景

022 已完成 Rust 编辑命令、Unicode 字素位置、选区和 composition 语义；027 已提供页面命中、caret 几何及基础键盘输入。Qt 窗口目前只接入直接文本输入、Enter、Shift+Enter 和 Backspace。此任务补齐输入法预编辑、视觉方向键导航和用户可见选区，使指针、键盘与 IME 共用 Rust Session 的文档位置和布局映射。

## 必读

- [编辑命令](../architecture/editor/commands.md)、[文字塑形](../architecture/editor/text-shaping.md)、[文稿光标与选区](../architecture/interface/caret.md)、[快捷键](../architecture/interface/keyboard-shortcuts.md)
- [分层](../standards/layering.md)、[FFI](../standards/ffi.md)、[验证与评审](../standards/validation-and-review.md)、[测试](../standards/testing.md)
- 任务：[022 文本编辑、选择与 IME](022-text-editing-and-ime.md)、[027 页面画布、命中与布局快照](027-canvas-and-hit-testing.md)

## 范围与非目标

- 接入 Qt `QInputMethodEvent` 的 preedit、commit 和 cancel；preedit 仅作视图叠加，不写入正文或增加文档 generation，commit 通过 Rust Session 恰好提交一次。提供 `inputMethodQuery` 所需的光标矩形、surrounding text、cursor/anchor 信息，使候选窗定位和上下文查询使用 Rust 当前快照。
- 处理 `replacementStart`/`replacementLength` 的 UTF-16 相对偏移：校验范围、转换为 Rust UTF-8 字节位置，并按 022 的扩展字素边界规则处理，不允许把组合字符或 emoji 拆开。
- 接入左右、上下、Home/End 等正文 caret 导航。左右按 Parley 视觉候选移动并保留逻辑 affinity；上下按目标视觉行保留期望 x；跨双向文本、软换行、段落边界和扩展字素簇的规则需有测试。
- 接入 Shift+方向键等键盘选区操作，以及鼠标拖选。拖动预览在 Qt 本地按当前布局更新，释放时将最终锚点/焦点位置一次提交 Rust；Rust Session 保存已提交的逻辑选区。
- 绘制跨行、跨页选区背景与选区文字颜色，复用 Rust 布局提供的几何；焦点、代际和异步结果规则遵守 027。
- 选区状态下的输入、Backspace、Enter 与软换行继续使用 Rust 命令事务。同一直接块容器内的 Body 段落选区应支持原子删除/替换和段落合并；跨不同 EnvironmentNode 容器、跨 EnvironmentTitle 与正文的选区按 020 规则拒绝且保持原状。未知字段或不兼容段落属性无法安全合并时拒绝整个事务，不丢失锚点、焦点或 preedit。

不包含撤销历史（018）、格式工具、命令模式、结构导航、协作编辑或平台特定输入法私有 API。系统输入法候选窗由 Qt 平台插件负责，不自行绘制。

## 前置条件与待决策

022 与 027 的 Rust 语义和几何可用。若 Qt 输入法事件与 Rust composition 生命周期间存在无法无损映射的通知顺序，先补适配契约和失败测试，不得在 QML 另存正文或 composition 副本。

## 实施步骤

1. 核实现有 Qt 输入事件、Rust composition/导航/选区命令和 Parley 视觉 caret API，固定 FFI DTO 与 generation 契约。
2. 将按键和 IME 事件适配放入画布输入层；鼠标点击/拖动预览与正文绘制保持分离。
3. 接通视觉导航、选区位置映射、跨页绘制和 preedit overlay，验证选区替换及取消/提交原子性。
4. 用真实 QML 窗口和可构造的 Qt 输入法事件覆盖英文、中文组字、AltGr、emoji、组合字符、bidi、软换行、跨页拖选和失焦。
5. 完成候选实现后由 subagent 独立 review，修复并复验，更新 task 和架构记录。

## 预计改动

`native/canvas/document_canvas/input.cpp` 键盘与 IME 事件适配、`native/viewmodels/document/` 命令桥接和布局快照适配、`qml/Canvas/` 输入组件、`qml/PageSurface.qml` 选区叠加、`caret-engine` `editing/session/` 子模块事务、`caret-app`/`caret-ffi` 编辑入口及 C++/QML 集成测试。必要时补充架构接口和快捷键文档。不得把编辑语义迁入 Qt。

## 验收标准

- [ ] Qt IME preedit 可见但不进入正文；提交只落一次，取消不改正文，焦点丢失或 Session 替换会正确结束/隔离旧 composition。
- [ ] 输入法查询返回当前光标矩形和正确的 surrounding text/cursor/anchor；replacement 范围经 UTF-16/UTF-8 转换并按字素边界安全应用。
- [ ] 左右移动遵循视觉 bidi caret；上下移动保持目标 x；所有导航停在扩展字素簇边界。
- [ ] Shift+方向键和鼠标拖动可生成方向稳定的 Rust 选区；拖动过程不对每个鼠标事件做 Rust 往返，释放时一次提交。
- [ ] 选区高亮覆盖多行与多页并与文字绘制对齐；颜色来自 QML Theme token，Qt 不重建选区规则。
- [ ] 输入、删除、Enter 和软换行对选区执行一次原子替换；编辑后位置映射正确，失败时正文与选区保持原状。
- [ ] 同一直接块容器内的跨段选区删除/替换原子合并；跨环境容器、跨标题与正文、未知字段或不兼容段落合并明确拒绝且不修改文档。
- [ ] 旧 layout/preedit 结果不能覆盖更新后的 Session；鼠标拖动、输入法更新和导航不触发无关整文档排版。
- [ ] Rust 领域测试、FFI 测试、QML lint/format 和真实窗口集成测试通过；三平台 CI 对应工作流通过或有明确环境限制。
- [ ] 独立 review 完成，发现项修复并复验，记录检查范围和限制。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | Rust 编辑与布局测试 | composition、导航、选区位置映射符合 020/022/027 规则 | 未执行 |
| — | Qt/C++/QML 窗口集成 | 输入法、焦点、拖选和跨页渲染可观察且代际正确 | 未执行 |
| — | 独立评审 | 无未解决的职责、原子性、视觉映射和性能问题 | 未执行 |

## 风险与回退

视觉左右移动不能用 UTF-8 偏移增减推导；必须使用同一 Parley 布局的视觉候选。IME preedit 生命周期和文档事务分开；事件重复或迟到不能重复提交。选区几何失效时保留 Session 的逻辑选区，不以 Qt 本地状态覆盖。若某平台测试设备不可用，记录限制并用 Qt 事件注入验证适配边界。

## 决策与工作记录

- 2026-10-26：从 022 Rust 编辑语义与 027 画布几何的剩余 Qt 集成范围拆分，作为独立可验收任务。

## 完成摘要

未完成。完成时填写 IME、视觉导航、选区交互、平台验证和独立评审结果，再与索引一起标 done。
