# 命令

更新日期：2026-10-25。执行规则见[运行时](../core/runtime.md)。`\` 的交互见[命令输入](command-input.md)。

## 注册表

每条命令有稳定的 id、给人看的英文 label、是否纯命令、以及当前是否可用。快捷键、补全和 `caret-cli` 都查这张表，不各自维护一份行为。

id 用小写点分段，段内使用 snake_case：`text.insert`、`para.heading`、`cmdline.accept`。别名不是第二条命令，只是名字到 id 和默认参数的映射。

命令定义表单独维护别名、稳定 ID、显示名和可用状态；命令查询状态机只负责查询、匹配、高亮与接受，不内嵌注册表数据。补全按前缀匹配，优先级为别名、命令 ID、显示名；同一优先级保留注册表顺序，匹配忽略 ASCII 大小写。空格不属于查询字符；M1 不做模糊搜索或隐式排序。Rust 返回完整候选列表和高亮索引，QML 不重复过滤或维护选中状态。

命令要么成功并给出 `Effect`，要么返回错误且不改文档。不使用 panic 表示「没有这个命令」或「参数不对」。

## 第一批 id

| id | 作用 | 撤销 |
| --- | --- | --- |
| `text.insert` / `text.delete` | 在插入点输入或删除 | 打字合并成一步 |
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
| `cmdline.begin` / `cmdline.type` / `cmdline.backspace` / `cmdline.move_selection` / `cmdline.select_candidate` / `cmdline.tab` / `cmdline.accept` / `cmdline.cancel` | 命令模式 | 查询和选择是纯命令；`tab`、`accept` 与重复前缀键由 Rust 按当前查询状态决定插入字面字符或执行候选 |
| `file.save` | 把当前文档写成 `.cdoc` | 不改文档，失败不丢内存里的稿 |

结构命令的完整行为和键位见[结构导航](structure-navigation.md)和[快捷键](../interface/keyboard-shortcuts.md)。

新增命令时同时写：它在公式内、表格内、命令模式里是否可用；它会不会改变 `rev`。没写明的命令在公式内先退出公式再执行。`struct.*` 和 `cmdline.*` 必须自己声明，不能靠这个默认。

M0 的 `clipboard.copy` 只导出选区纯文本；多段选区用换行分隔。`clipboard.paste` 读取系统剪贴板后由 Rust 负责规范换行、替换当前选区并提交一次事务。当前不支持的结构或未知字段必须在写入前拒绝，保留原文与选区。撤销系统接入后，整次粘贴应作为一个撤销单元。
