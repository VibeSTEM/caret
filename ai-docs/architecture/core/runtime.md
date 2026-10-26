# 运行时

更新日期：2026-10-26。`caret-engine` 已开始实现 `Session`、UTF-8 位置和基础文本/IME 命令；历史、布局缓存、结构索引与 FFI 接入仍按后续任务推进。路径、设置、语言、主题和运行时上下文在 `caret-app`。

## 权威与派生

| 数据 | 谁写 | 谁读 |
| --- | --- | --- |
| `Document` | 只有文档命令 | 布局、结构索引、保存 |
| 选区、公式内光标 | 导航和编辑命令 | 画布、状态栏 |
| 命令模式查询 | `cmdline.*`，不进文档 | 补全弹窗 |
| `DocLayout` | 布局器，按缓存键 | 画布、命中、焦点框 |
| `StructureIndex` | 结构索引，按段落 `structureRev` 和大纲样式代际 | 结构命令、状态栏 |
| 应用设置与用户偏好 | `caret-app` 中的 Rust 服务 | QML 经 ViewModel；C++ 只消费当前视图所需的临时值 |
| 应用主题、界面语言 | `ThemeService`、`LanguageService` | QML 经 ViewModel |

鼠标 hover 和按下后的拖选预览留在 C++ 画布。鼠标释放时，画布把最终插入点或选区作为导航更新提交到 Rust；`Session` 始终保存已提交的选区。

派生数据可以整份丢掉再算。丢了 `Document` 就不能算回来。

## 会话

`Session` 放在 `caret-engine`。界面经 `caret-ffi` 进入，`caret-cli` 经 `caret-app` 进入，测试可以直接调用。修改都走 `Session::run`。

应用设置的权威状态与持久化属于 Rust `caret-app`，QML 和 C++ 不保存平行副本。Settings 是设置界面的总入口；用户可调偏好归入 Preferences，由 Rust 偏好服务验证、提交和持久化，再通过 ViewModel 发布只读快照。QML 仅显示控件值并提交用户意图；ViewModel 转换 Qt 参数和信号，不实现或持久化设置。画布等 C++ 组件只接收绘制所需的当前值，例如光标闪烁开关。现有 `SettingsService` 尚未接入界面；按此边界接入 Preferences 时，统一由 Rust 服务持有，不在 QML、C++ 或 Theme DSL 重复存储。

界面主题的颜色、间距和控件外观属于 Theme，不属于 Preferences。`Theme.qml` 是 QML 只读 token 门面，内置主题数据位于 `qml/Themes/*.pa`，由 Rust `ThemeService` 校验、合并并经 ThemeViewModel 发布快照。用户选择的主题 ID 属于 Rust Preferences。若开放外部自定义主题，定义仍作为主题数据文件交给 Rust ThemeService 校验；不把可变 token 写进 `Theme.qml`。切换主题只发布一份新的 token 快照，QML 控件从本地绑定读取，不按属性、控件或帧回调 Rust。当前 ThemeService 支持内置主题快照，外部用户主题加载尚未实现。

```text
Session
├── document
├── selection
├── math: Option<MathEdit>
├── cmdline: Option<CmdLine>
├── undo / redo
├── layout_cache + last_layout
└── structure
```

`Session::run(command) -> Result<Effect>` 是唯一的修改入口；命令类型提供稳定 id。`Effect` 只说明这次变了文档、选区、叠加层还是插入点。QML 不保存第二份正文。

命令分两类：

- **文档命令**成功后形成一个可撤销的原子文档变更；历史记录表示、输入合并和容量由[018](../../task/018-document-lifecycle-history.md)决策。
- **纯命令**（补全移动、取消命令模式）不改文档，不产生撤销步。

连续打字如何合并、保存点如何与撤销/重做交互，由 018 定稿。M0 不预设撤销栈容量或快照实现。

关闭文档或切换文件会使这一份 `Session` 失效。迟到的回调带着旧的会话代际，必须丢掉。

## 一次按键

1. C++ 收到键或 IME 事件，先判断组字。组字中的文本不进入命令模式。
2. 若命令模式、公式或结构导航认这个键，就组成命令 id 和参数。
3. `run` 在 Rust 里执行。M0 可以让整个文档布局失效；结构索引随 M2 的结构命令更新；达到 M4 时，再按段落 `layoutRev` 做增量排版，具体布局策略见[增量排版](../editor/incremental-layout.md)。
4. Rust 交出新的页面快照。C++ 画布在后续帧里用这份快照绘制和做命中。hover 和拖选中的指针移动不回到 Rust；释放时才提交最终选区。
5. QML 更新补全列表、状态栏和焦点。它不解析文档字节。

长任务（打开大文件、以后的导出）在 worker 里做，完成时回到 GUI 线程提交结果。失败保留用户还有效的文档。

## 保存

保存走 `.cdoc`，格式和原子写入见[文档模型](../editor/document-model.md)。路径由应用服务解析后再交给引擎，引擎不调用 Qt。保存开始时捕获文档 revision；worker 只写这个快照。同一 Session 对同一已解析目标路径的保存请求按发起顺序串行写入。完成时回报该请求的写入结果并更新对应保存点，不替换当前 `Session`。若期间产生较新的 revision，它仍是未保存修改。
