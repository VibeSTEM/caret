# 键盘路由

更新日期：2026-11-08。用户可见的键位表见[快捷键](keyboard-shortcuts.md)；仓库快捷键的职责与路由架构见[快捷键管理架构](keyboard-shortcut-architecture.md)；命令 ID 见[命令](../editor/commands.md)。

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

窗口级 `Shortcut`、控件本地按键和画布按键属于不同焦点作用域，不要求集中成一个大型全局路由器。每个作用域内部只保留一个事件入口；最内层焦点所有者处理后接受事件，不让同一次按键继续触发外层动作。Qt 在 macOS 上把 `ControlModifier` 映射到物理 Command、`MetaModifier` 映射到物理 Control；需要物理修饰键语义的画布快捷键统一通过 `native/canvas/keyboard_modifier_policy.hpp` 映射，避免每个快捷键各自处理平台差异。QML 控件的简单关闭行为使用 Qt 的 Popup/Window 焦点和关闭策略；多阶段 UI 流程按 [Qt 规范](../../standards/qt.md)使用 C++ `QStateMachine`。

## 详细规则

本文只定义跨层边界。画布按键的单键优先级、状态迁移和各处理器职责以[快捷键管理架构](keyboard-shortcut-architecture.md)为唯一权威；用户可见键位清单以[快捷键](keyboard-shortcuts.md)为唯一权威。本页不重复按键行为表。

当前所有已支持的画布编辑器快捷键都通过 `Session::run_editor_key` 分派到 `session/keyboards/` 的具体处理器。方向键、Home/End 的布局候选由 app 计算后随意图交回 Session；Qt `commandMode` 只投影光标显示，不改变编辑器键语义。窗口和控件作用域不并入编辑器 Session。尚未实现的表格快捷键只在键位规划中标注，不宣称已接入。

平台 IME 候选界面可先消费 Escape；画布 IME 预编辑的物理事件由 Qt 输入适配处理。除这些平台输入例外外，Qt 不根据命令、公式、结构选择等编辑状态自行仲裁按键。当前 Rust 按键集合没有 `ShiftTab`；表格导航须在功能实现时定义并进入对应键处理器，不能把键位表中的目标行为读作已支持。

行为矩阵、命令行与公式的当前边界、目标逐键文件、允许/拒绝的状态组合和测试要求，统一维护在[快捷键管理架构](keyboard-shortcut-architecture.md)。

快捷键的新增、冲突处理、测试和扩展规则统一见[快捷键管理架构](keyboard-shortcut-architecture.md)。
