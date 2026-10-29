# 键盘路由

更新日期：2026-10-29。用户可见的键位表见[快捷键](keyboard-shortcuts.md)；命令 ID 见[命令](../editor/commands.md)。

## 目标

同一按键在弹窗、命令输入、输入法预编辑、选区和文档结构等上下文中只执行一个动作。按键处理先确定焦点作用域，再把编辑器语义交给唯一权威状态持有者；不能让 QML、C++、Rust 各自保存一份可变交互状态并竞争同一事件。

## 路由和职责

| 层 | 实现位置 | 拥有的状态与职责 |
| --- | --- | --- |
| 模态对话框、菜单弹出层 | 对应 QML Dialog / Popup | Qt 焦点栈先把事件交给最内层可见控件；对话框或弹出菜单关闭时消费该事件，不再传给正文画布 |
| 应用窗口 | `qml/App.qml` 的 `Shortcut` | 打开、保存、撤销/重做、缩放等窗口级动作；跨平台键位使用 Qt `StandardKey` |
| 正文画布 | `DocumentCanvas` 的 `document_canvas/keyboard.cpp` | 把 Qt `QKeyEvent` 和平台修饰键映射为编辑意图；不改文档，也不替 Rust 判断命令、IME、选区或结构状态 |
| IME | `document_canvas/input.cpp` 与 QML 输入法事件桥 | Qt 保存用于绘制的预编辑投影并处理提交/光标查询；Rust Session 持有编辑会话 composition 的权威状态。Rust 通过 Escape 实际取消 composition 后，在布局响应中返回 `compositionCancelled`；QML 仅在该标志为真时清除 Qt 预编辑投影并同步空输入法事件 |
| 编辑会话 | `caret-engine::editing::session::keyboards` | 持有编辑上下文的权威状态并裁决有冲突的编辑意图；每种按键行为单独维护 |

窗口级 `Shortcut`、控件本地按键和画布按键属于不同焦点作用域，不要求集中成一个大型全局路由器。每个作用域内部只保留一个事件入口；最内层焦点所有者处理后接受事件，不让同一次按键继续触发外层动作。QML 控件的简单关闭行为使用 Qt 的 Popup/Window 焦点和关闭策略；多阶段 UI 流程按 [Qt 规范](../../standards/qt.md)使用 C++ `QStateMachine`。

## Rust 编辑器按键仲裁

Qt 将以下语义按键交给 Rust `EditorKey`，由 `Session::run` 单点分发：

| 意图 | 优先级和结果 |
| --- | --- |
| `Escape` | 命令输入 → IME 预编辑 → 非空选区 → 当前最内层可退出结构 → 无操作。收起选区只收起；再次按 Esc 才退出结构 |
| `Enter` | 命令输入时接受当前候选；否则复用 `InsertParagraphBreak` |
| `ShiftEnter` | 命令输入时无操作，防止改到查询背后的正文；否则复用 `InsertSoftBreak` |

未来增加公式、表格等可编辑上下文时，先定义它们在作用域树中的父子关系，再按最内层优先加入对应按键处理器；不得再让 C++ 通过 `commandMode` 等状态副本抢先执行正文命令。平台 IME 候选界面可先消费 Esc；只有事件到达画布时才进入上述 Session 优先级。

`Escape`、`Enter`、`ShiftEnter` 的状态转换是 Rust Session reducer 中的领域交互，不需要 Qt `QStateMachine`。Rust 单元测试验证状态优先级和文档副作用；Qt shell 测试验证实体按键到 `EditorKey` 的映射、修饰键和平台路由。两层组合测试至少覆盖：命令查询 Enter 接受候选、查询 Esc 只取消查询、Shift+Enter 不改查询后的正文，以及选区 Esc 后再次 Esc 才退出结构。

## 新增快捷键

1. 先在快捷键表说明用户动作、平台键序列、焦点作用域及与现有按键的冲突关系。
2. 应用窗口动作加到 `qml/App.qml` 的窗口快捷键区；控件本地动作由拥有该控件焦点的组件处理；画布动作只在 `document_canvas/keyboard.cpp` 做 Qt 键位映射。
3. 若结果取决于命令输入、IME、选区、节点种类等 Rust 会话状态，新增或复用 Rust 命令意图，并在 `session/keyboards/` 中添加行为；不要在 Qt 复制状态逻辑。
4. 为映射和行为分别写测试，覆盖无活动上下文、相互冲突的上下文及事件是否继续传播。命令状态、文档修订和可见副作用由 Rust 测试验证；焦点/控件所有权由 Qt/QML 测试验证。
5. 单纯增加一个键位不自动引入运行时可配置快捷键注册表。只有用户可重映射或动态扩展成为需求时，才增加描述符注册表；届时描述符应包含稳定动作 ID、平台键序列、焦点作用域、冲突优先级和可用条件，行为仍委托给现有命令入口。
