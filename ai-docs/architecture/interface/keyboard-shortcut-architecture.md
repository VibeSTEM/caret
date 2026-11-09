# 快捷键管理架构

更新日期：2026-11-08。用户可见的按键清单见[快捷键](keyboard-shortcuts.md)，事件分层入口见[键盘路由](keyboard-routing.md)。本文说明快捷键的管理边界、状态优先级和逐键模块设计。

文档单一权威：`keyboard-shortcuts.md` 只权威维护用户可见键位清单；本文只权威维护逐键模块、上下文优先级和层间职责；`keyboard-routing.md` 仅介绍入口并链接到本页，不复制行为矩阵。Task 只记录范围与验收，不另维护一份键位表。

## 设计目标

快捷键是“焦点作用域 + 实体按键 + 当前交互上下文”共同决定的一次意图。相同按键在弹窗、命令输入、输入法、公式编辑和正文中可以有不同含义，但一次事件只能由当前最内层有效处理者执行。Qt/QML 负责把事件送到作用域入口，Rust Session 对依赖编辑状态的意图做领域裁决；布局只提供视觉几何。

编辑器快捷键统一由 `crates/caret-engine/src/editing/session/keyboards/` 管理。每个现有快捷键或语义独立的快捷键家族有具体处理文件；同一处理器可收纳只改变扩选/修饰状态的变体，像 Mod+Backspace 这种独立动作保留自己的 `mod_backspace.rs`。`keyboards.rs` 负责入口和事件分派。不要按编辑上下文新建 `math.rs` 之类的路由文件，也不要把按键优先级留在 `math_edit.rs`、app 或 QML。数学模块只提供编辑、移动和退出等领域操作，由对应快捷键处理文件决定何时调用。

窗口快捷键和控件本地快捷键仍分别属于窗口与焦点控件作用域，不并入编辑器 Session。已支持的画布快捷键统一进入 Session 单点入口；各项未实现能力仍明确列为后续目标。

## 一次按键的路由

```text
最内层弹窗 / 有焦点控件
        │ 未消费
        ▼
应用窗口快捷键（仅窗口动作）
        │ 正文画布获得焦点
        ▼
Qt QKeyEvent → 平台键映射 → 编辑器意图
        │
        ├─ IME 预编辑/提交：Qt 输入适配 + Rust composition 状态
        └─ Session 单次路由目标 → 命令输入 / 结构选区 / 数学编辑 / 正文
                                  │
                                  └─ 几何候选由 app 布局快照提供
```

事件处理者消费事件后不得让同一事件继续触发外层快捷键。打开、保存和缩放属于窗口级动作，仍放在 `qml/App.qml`；撤销/重做当前由窗口 Shortcut 发起，但画布编辑焦点下的状态优先级目标由 `mod_z.rs` 管理，其他控件焦点不进入编辑器 Session。控件自身的关闭、选择等动作由拥有焦点的控件负责；画布键位由 `document_canvas/keyboard.cpp` 映射。依赖命令行、选区、公式或文档结构状态的行为必须由 Rust 命令/Session 决定。

画布提交的普通文本通过 `Command::TextInput` 进入 Session。Session 根据当前焦点把文本交给命令查询、将 `\\` / `$` 识别为命令或公式入口，或继续执行普通文本输入；QML 不查询 `commandLine.active` 来选择目标，也不维护字符前缀路由。低层 `InsertText` 保留为明确的文档/公式编辑命令，不承担宿主键盘目标选择。

## 逐键模块

| 目标文件 | 键位 / 状态 | 职责 |
| --- | --- | --- |
| `escape.rs` | Escape；已有处理器 | 命令输入、IME、数学编辑、选区、结构退出顺序 |
| `enter.rs` | Enter、Shift+Enter；已有处理器 | 命令候选、Align 换行、数学退出/拒绝、正文换段或软换行 |
| `tab.rs` | Tab；已有处理器 | 命令补全、数学上下文保留焦点、正文空格 |
| `backslash.rs` | `\` 命令前缀；已接入 | 正文启动命令输入；命令输入/公式焦点由 Session 保持对应输入语义 |
| `dollar.rs` | `$`；已接入 | 正文创建并聚焦空行内公式；公式中作为数学输入，命令查询中作为查询文本。按输入字符识别；`、`、`¥` 和 `￥` 保持普通文本 |
| `backspace.rs` | Backspace；已接入 | 正文删除、命令输入删除、公式内部删除和边界行为 |
| `mod_backspace.rs` | Mod+Backspace；已接入 | IME/命令行、数学编辑、行内环境、块环境顺序 |
| `delete.rs` | Delete；已接入 | 正文及公式上下文中的前向删除；表格单元格行为待 045/046 接入后实现 |
| `left_arrow.rs` / `right_arrow.rs` | Left / Right 与 Shift 变体；已接入 | 命令行光标、数学内部移动、正文视觉候选、公式宿主进入和边界行为；修饰变体随对应实体键管理 |
| `up_arrow.rs` / `down_arrow.rs` | Up / Down、Shift 与结构移动变体；已接入 | 各自 handler 明确选择 composition、命令行、公式和正文分支；结构移动受高优先级状态约束，布局候选提交委托 `keyboards/helpers/vertical_navigation.rs` |
| `home.rs` / `end.rs` | Home / End 与 Shift 变体；已接入 | 命令行/数学文本/正文视觉行边界；修饰变体随对应实体键管理 |
| `page_up.rs` / `page_down.rs` | PageUp / PageDown；后续结构任务 | 同类结构导航，按对应结构任务接入时再实现 |
| `mod_space.rs` | Mod+Space；已接入 | 扩大结构选区；普通 Space 是文本输入，不是编辑器快捷键 |
| `alt_t.rs` | Alt+T；未实现，快捷键表中的表格规划 | 插入表格；仅在 045/046 接入后实现 |
| `mod_c.rs` / `mod_v.rs` | Mod+C / Mod+V；已接入 | 复制/粘贴的会话语义与作用域；系统剪贴板读写由 app 适配层提供 |
| `mod_z.rs` | Mod+Z / Mod+Shift+Z；已接入 | 画布焦点下的 Undo/Redo Session 优先级、公式源码草稿本地历史与文档历史；工具栏仍使用 undo()/redo() 命令 API |

当前逐键入口覆盖[唯一键位清单](keyboard-shortcuts.md)中所有已支持的画布编辑器快捷键；表格等尚未实现的规划键只记录目标文件和依赖，功能本身留在 045/046 等后续任务。未来新增编辑器快捷键也按具体快捷键或独立语义家族加入同一目录，不在 app、QML 或领域模块另建仲裁入口。目标结构是 `keyboards.rs` 只做模块注册和 `run_editor_key` 分派；本键状态优先级与具体动作由逐键 handler 负责。只有同一候选校验/提交确实被多个键以相同不变量调用时，才放进 `keyboards/helpers.rs` 与 `keyboards/helpers/`，辅助模块不得重新决定上下文优先级。app 保有并计算页面布局模型和视觉几何，engine 不复制或重算这些数据。候选无效时必须在任何焦点/文档迁移前拒绝，保证失败原子性。

只改变扩选/修饰状态的变体（如 Shift+方向键）随基础键处理；形成独立语义动作的修饰快捷键（如 Mod+Backspace、Mod+Z）可有独立文件，仍由单一 Session 键盘入口分派。Qt 负责物理键和平台修饰映射；IME 预编辑仍由 Qt 输入适配先处理。

### 当前行为

下表记录当前已实现的行为边界，供实现与回归测试共同遵循。逐键架构仍在收敛；不把统一入口误读为每个键已经以相同方式独立管理仲裁。

| 按键 | 当前行为与例外 |
| --- | --- |
| Escape | MathEdit、命令输入、composition、块环境/普通选区与结构按 handler 的明确优先级处理；普通选区 Escape 只收起选区，再按一次才退出结构。IME 预编辑时 Qt 只转交 Escape 意图，由 Session 确认是否取消。 |
| Enter / Shift+Enter | 命令输入下 Enter 接受候选、Shift+Enter 无操作；公式 Align 中 Enter 插入行分隔，数学文本分支拒绝换行，其他公式 Enter 退出；正文分别插入段落或软换行。 |
| Tab | 命令输入交给 `TabCommandLine`：字体候选确认、普通查询补全、空查询退出并插入字面 `\`；公式中保留公式焦点；普通正文插入两个字符宽空格。表格 Tab 导航尚未实现。 |
| Backspace | 组合输入期间不改文档；命令行优先删除查询；其余情况交给共享 `Command::Delete(Backward)` 事务处理选区、虚拟块边界、MathText/MathTree/Source 与正文删除。逐键 handler 决定焦点优先级，领域事务共享复杂删除规则。 |
| Delete | 组合输入或命令行期间不改文档；其余情况交给共享 `Command::Delete(Forward)` 事务，按公式、结构和正文语义删除。 |
| Mod+Backspace | composition/命令输入中不改正文；数学编辑按公式规则处理；正文优先解除可解除的最内层行内环境，否则尝试解除块环境并保留内容。 |
| Left / Right / Up / Down / Home / End | Qt 把方向和 Shift 扩展映射为几何查询，app 提供带 Session、文档和字体代际的单次候选。每个键 handler 按自己的状态顺序处理 composition、命令行、MathEdit 和正文；结构移动变体在 composition、命令行或数学编辑活动时保持原位。MathTree Up/Down 使用当前公式内布局候选；跨操作数 Shift 扩展因单路径状态不能表示而保持原位。候选代际检查与无损提交由 `keyboards/helpers/navigation.rs` 和 `vertical_navigation.rs` 共享。Qt `commandMode` 不再抢先改写语义。 |
| Mod+C / Mod+V | 对应键 handler 判断 composition、命令输入、数学文本选区及正文选区的允许行为；effect 请求 app 执行系统剪贴板读写。 |
| Mod+Z / Mod+Shift+Z | 画布有焦点时 QML Shortcut 将 `mod-z` / `mod-shift-z` 交给 EditorKey handler，Session 选择数学源码草稿或文档历史；工具栏仍调用 undo()/redo()。 |
| 结构与表格修饰键 | 结构扩选和上一块/下一块通过 `mod_space.rs`、`up_arrow.rs`、`down_arrow.rs` 进入 Session。表格快捷键尚属 045/046 规划，表格节点接入前不启用。 |

以上记录当前实现的行为和边界；各上下文保留既有差异，未实现的键只登记目标文件与依赖，不把键位清单中的未来目标写成已支持行为。

## 状态与优先级

导航键没有强制套用到所有键的统一顺序。Up/Down 当前顺序如下；其他键以逐键处理器和上方行为矩阵为准：

| 顺序 | 目标 | 说明 |
| --- | --- | --- |
| 1 | IME composition | 普通 Up/Down 不改变正文或数学选择 |
| 2 | 命令输入 | Up/Down 移动命令行候选选择，不移动正文 |
| 3 | MathEdit | 按 Text、Tree 或 Source 分支解释数学视觉候选；Shift 跨树位置保持原位 |
| 4 | 正文 | 使用普通布局候选提交视觉选择变化 |
| 特例 | 结构移动变体 | composition、命令行或 MathEdit 活动时无操作，否则执行 MoveStructure |

每个按键文件可以有不同的状态优先级，不应把导航顺序机械套到 Enter、Tab 等按键上。Left/Right/Home/End 当前也在各自文件中仲裁 composition、命令行、MathEdit 和正文候选；共享 helper 只校验 generation、提交候选或完成既定的数学分支候选。`keyboards/helpers/text_input.rs` 是宿主文本输入适配入口，负责命令行目标选择和把 `\\` / `$` 交给对应的具体按键 handler，不代替实体按键 handler 仲裁。

交互字段也不是一概互斥：已确认 MathEdit 可与 MathText composition 共存，也可与 command line 共存；其他组合要逐项确认允许/拒绝条件，不推断为任意组合均合法。每次路由只选择拥有本次按键的处理者，并保留其他合法状态。

方向键的视觉顺序、双向文本亲和性和 preferred x 属于布局几何信息；Session 不应复制 Parley 页面模型。Session 负责判断导航目标、允许/拒绝迁移和提交选择/结构变更。app 负责把同代快照上的几何候选交给 Session，并投影结果。公式内部树路径、文本岛和源码位置由数学领域模块负责。

## 一致性结论

已支持画布实体按键共享 `Session::run_editor_key` 分派入口。公开按键和文本适配入口都经过 Session 事务包装，执行过时代际检查、错误恢复和成功后的修订/锚点同步。逐键状态仲裁由对应 handler 管理；导航候选提交与文本输入适配分别位于 `keyboards/helpers/`。Backspace/Delete 在 handler 中决定 composition/命令行焦点优先级，之后复用 `Command::Delete` 领域事务处理数学、结构与正文删除，避免逐键复制同一套事务规则。`run_editor_key_with_font_choices_lazy` 只把按键 handler 需要的字体目录闭包交给 Enter/Tab handler，只有选择字体候选时才计算目录，不在 app 侧判断命令行优先级。`Command::Undo/Redo` 和直接领域 `Delete` 命令仍保留 Session 级兼容入口。窗口快捷键与控件本地按键仍属于各自焦点作用域；尚未实现的表格键不计入“已支持”。

视觉候选包含 Session、文档和字体代际；Session 验证自身和文档代际，app 在生成候选时验证当前字体代际。扩选锚点由 Session 当前选择构造，app 不能提交任意旧锚点。正文选区锚点与焦点各自保留视觉侧并随撤销/重做恢复；布局和 Qt 只读取会话状态，不再维护第二份可变锚点亲和性。MathText 两端视觉侧仍随其数学文本位置保存。剪贴板系统调用留在 app，复制/粘贴是否允许由逐键模块决定。

## 新增与修改快捷键的规则

1. 在[快捷键表](keyboard-shortcuts.md)写明平台键序列、焦点作用域、用户动作和冲突行为。
2. 先确定最内层焦点所有者；弹窗、命令输入、IME、结构选择和公式编辑不得由更外层处理器抢先改写文档。
3. 纯窗口动作放窗口快捷键；控件本地动作留在控件；画布物理键映射集中在 Qt 键盘入口；编辑状态相关语义交给 Session 命令/意图路由。
4. 视觉导航把同代布局几何作为候选数据传入，Session 不重算像素几何；Session 不因单次路由目标而强制合并可并存的持久状态。
5. 为键位映射和领域行为分别添加测试，覆盖焦点作用域、优先级冲突、事件消费、状态不变性及文档副作用。
6. 不为静态快捷键表增加运行时注册表。只有用户重映射或动态扩展成为需求时，才建立含稳定动作 ID、平台键序列、作用域、冲突优先级与可用条件的描述符；动作仍委托给已有命令入口。
