# 命令

更新日期：2026-10-30。执行规则见[运行时](../core/runtime.md)。`\` 的交互见[命令输入](command-input.md)。

## 注册表

每条命令有稳定的 id、给人看的英文 label、是否纯命令、以及当前是否可用。快捷键、补全和 `caret-cli` 都查这张表，不各自维护一份行为。

id 用小写点分段，段内使用 snake_case：`text.insert`、`para.heading`、`cmdline.accept`。别名不是第二条命令，只是名字到 id 和默认参数的映射。

命令定义表单独维护别名、稳定 ID、显示名和可用状态；命令查询状态机负责查询、匹配、高亮与接受，不内嵌注册表数据。补全按前缀匹配，优先级为别名、命令 ID、显示名；同一优先级保留注册表顺序，匹配忽略 ASCII 大小写。空格不属于查询字符；M1 不做模糊搜索或隐式排序。Rust 按[候选可用性规则](command-input.md#候选可用性)返回当前列表和高亮索引，QML 不重复过滤或维护选中状态。

命令要么成功并给出 `Effect`，要么返回错误且不改文档。不使用 panic 表示「没有这个命令」或「参数不对」。

## 第一批 id

| id | 作用 | 撤销 |
| --- | --- | --- |
| `text.insert` / `text.insert.one_character_width_space` / `text.insert.two_character_width_space` / `text.delete` | 在插入点输入、插入空格或删除；Tab 与 `\doublecharspace` 共用插入两个字符宽空格动作；`\singlecharspace` / `\doublecharspace` 表示字符宽度，不表示 U+0020 空格个数；Backspace 在完整块环境结构选区上删除环境子树 | 打字合并成一步；结构删除一步 |
| `text.soft_break` / `text.split_paragraph` | 插入软换行 / 拆分或合并段落 | 一步 |
| `caret.move` / `selection.set` | 移动插入点 / 设置选区 | 纯命令 |
| `clipboard.copy` / `clipboard.paste` | 复制所选纯文本 / 以单一 Rust 事务粘贴系统剪贴板文本 | 复制纯命令；粘贴一次文档变更 |
| `ime.update` / `ime.commit` / `ime.cancel` | 更新、提交或取消输入法组字 | 仅提交正文时一步 |
| `page.geometry.set` | 设置整份文档的纸型、方向和边距 | 一步 |
| `para.heading` | 设大纲级别 | 一步 |
| `para.bullets` / `para.numbering` | 列表 | 一步 |
| `insert.equation` | 插入公式对象 | 一步 |
| `equation.move` / `equation.exit` | 公式内部导航、离开 | 纯命令 / 离开时若只动光标则纯命令 |
| `insert.table` | 插入表格 | 一步 |
| `table.insert_row` / `table.delete_row` / `table.insert_col` / `table.delete_col` | 改表格形状 | 一步 |
| `struct.next` / `struct.prev` / `struct.exit` | 结构跳转 | 纯命令 |
| `struct.section.create` | 把当前 Paragraph block 转为 Section 标题，保留标题段落属性与行内内容项 | 一步，可撤销 |
| `text.format.bold` / `text.format.italic` / `text.format.font_family` | 应用粗体、斜体或所选字体族行内环境 | 一步，可撤销；空选区的待输入样式亦可撤销 |
| `paragraph.first_line_indent.set` | 将当前段落首行缩进设为 2 个字符宽度 | 一步，可撤销；由 `\firstlineindent` 调用，与 Tab 插入空格无关 |
| `interaction.escape` / `interaction.enter` / `interaction.shift_enter` / `interaction.tab` / `interaction.mod_backspace` | 编辑器按键意图；由 Rust Session 根据活动交互上下文裁决，优先级见[键盘路由](../interface/keyboard-routing.md) | 视分支而定；文档变更各作为一个编辑撤销 |
| `struct.remove_preserving_content` | 将当前块环境标题与正文提升为父级内容并移除环境外壳 | 一步，可撤销 |
| `text.format.remove_environment_preserving_content` | 移除指定行内环境外壳并保留子节点；由 Mod+Backspace 的 Rust 仲裁调用 | 一步，可撤销 |
| `cmdline.begin` / `cmdline.type` / `cmdline.backspace` / `cmdline.move_selection` / `cmdline.select_candidate` / `cmdline.tab` / `cmdline.accept` / `cmdline.cancel` | 命令模式 | 查询和选择是纯命令；`tab`、`accept` 与重复前缀键由 Rust 按当前查询状态决定插入字面字符或执行候选 |
| `file.save` | 把当前文档写成 `.cdoc` | 不改文档，失败不丢内存里的稿 |

结构命令的完整行为和键位见[结构导航](structure-navigation.md)和[快捷键](../interface/keyboard-shortcuts.md)。

注册表条目不代表命令已经可执行。当前 `struct.next`、`struct.prev`、`struct.exit` 与 `struct.section.create` 已进入用户交互；Custom 容器创建目前仅有 Rust 领域 API，不是命令候选，用户入口留待 Customize 设计。标题级别命令和列表仍由后续任务实现，未接入前候选必须标记不可用。结构选区上的 Backspace 复用 `text.delete` 按键意图，不新增独立候选。每次实现后同步更新可用状态和行为表，不让候选显示与 Rust 实际分发脱节。

新增命令时同时写：它在公式内、表格内、命令模式里是否可用；它会不会改变 `rev`。没写明的命令在公式内先退出公式再执行。`struct.*` 和 `cmdline.*` 必须自己声明，不能靠这个默认。

编辑器按键从 Qt 以 `EditorKey` 意图进入 Rust Session；Session 是命令输入、IME、选区和文档结构状态的唯一权威。按键分发目录、行为优先级、Qt 焦点作用域和扩展规则见[键盘路由](../interface/keyboard-routing.md)。

M0 的 `clipboard.copy` 只导出选区纯文本；多段选区用换行分隔。`clipboard.paste` 读取系统剪贴板后由 Rust 负责规范换行、替换当前选区并提交一次事务。当前不支持的结构或未知字段必须在写入前拒绝，保留原文与选区。撤销系统接入后，整次粘贴应作为一个撤销单元。
