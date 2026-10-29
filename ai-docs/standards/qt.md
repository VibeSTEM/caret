# Qt 对象、线程与部署

更新日期：2026-10-27。锁定版本见[技术基线](baseline.md)。对象线程模型见 [Qt 文档](https://doc.qt.io/qt-6/threads-qobject.html)。界面组件见 [QML](qml.md)。

## 项目规则

- 一个应用对象，初始化顺序固定。ViewModel 在 GUI 线程。长任务在 worker 里跑，用排队事件更新可观察状态。
- 类型注册走 QML 模块。可变属性在值变化时发通知。模型的增删使用模型 API。
- 发信号使用 `emit`。实现文件直接包含声明该宏的头，方便 include-cleaner 看到依赖。
- `QObject` parent 表示唯一销毁关系。回调捕获对象时检查对象仍然存活。
- 停止线程前完成取消和对象清理。`QThread` 对象本身不等于已经运行在 worker 线程上。
- 可观察错误进入界面，诊断进入日志。后台错误不在任意线程弹对话框。窗口关闭后，迟到任务不得更新已销毁的 ViewModel。
- 交互阶段复杂时，由 C++ 持有 `QStateMachine`。QML 只发意图并绑定状态。状态机不决定 Rust 事务是否成功。简单视觉状态继续用属性绑定。
- 屏幕、DPI 和窗口几何留在 C++。不把 `QScreen*`、屏幕下标或整数缩放比写进持久化状态。逻辑像素和物理像素的转换使用运行时的 device pixel ratio。
- 键盘按焦点作用域分层：对话框/弹出菜单处理本地快捷键，窗口快捷键在 `qml/App.qml`，画布物理按键由 `DocumentCanvas` 转成编辑意图；文档交互状态优先级交由 Rust Session 唯一裁决。详细路由见[键盘路由](../architecture/interface/keyboard-routing.md)。
- `DocumentCanvas` 的按键映射与 IME 事件分文件维护：`document_canvas/keyboard.cpp` 识别 Qt 按键并发出意图，`document_canvas/input.cpp` 处理 IME 上下文和预编辑事件。C++ 不复制 Rust 文档交互状态；Qt 焦点和弹窗生命周期仍由 Qt 管理。

## 交互状态机

复杂的多阶段交互由其专属 C++ controller 持有 `QStateMachine`；QML 只发送用户意图并绑定公开状态，不再用多个布尔量拼接同一流程。状态机负责界面阶段转换，不持有业务数据的权威副本，也不推断 Rust 操作的结果。

设置对话框是该模式的实例：`Closed → Editing → Applying → Closed/Failed`。`Editing` 中的临时表单值属于 Qt controller；打开时从 Rust 管理的偏好快照初始化，取消时丢弃，确认时提交给 Rust。只有 Rust 回报成功后才能进入 `Closed`；失败进入 `Failed`，保留草稿供用户修正或重试。持久化状态、校验、只读状态和保存诊断仍由 `caret-app::settings::SettingsRuntime` 管理。

不为简单属性绑定、光标可见性等视觉状态引入状态机。新增状态转换须说明可达状态、触发意图、失败路径和关闭/销毁时的清理行为，并以转换测试覆盖。

发布包检查 Qt 插件、QML imports 和实际链接的动态库。部署脚本不会自动带上所有非 Qt 依赖。无开发环境变量的启动是独立验收，不能只用构建机上的 `cargo run` 代替。
