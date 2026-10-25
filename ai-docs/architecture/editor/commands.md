# 命令

更新日期：2026-10-25。执行规则见[运行时](../core/runtime.md)。`\` 的交互见[命令输入](command-input.md)。

## 注册表

每条命令有稳定的 id、给人看的英文 label、是否纯命令、以及当前是否可用。快捷键、补全和 `caret-cli` 都查这张表，不各自维护一份行为。

id 用小写点分段，段内使用 snake_case：`text.insert`、`para.heading`、`cmdline.accept`。别名不是第二条命令，只是名字到 id 和默认参数的映射。

命令要么成功并给出 `Effect`，要么返回错误且不改文档。不使用 panic 表示「没有这个命令」或「参数不对」。

## 第一批 id

| id | 作用 | 撤销 |
| --- | --- | --- |
| `text.insert` / `text.delete` | 在插入点输入或删除 | 打字合并成一步 |
| `text.soft_break` / `text.split_paragraph` | 插入软换行 / 拆分或合并段落 | 一步 |
| `caret.move` / `selection.set` | 移动插入点 / 设置选区 | 纯命令 |
| `ime.update` / `ime.commit` / `ime.cancel` | 更新、提交或取消输入法组字 | 仅提交正文时一步 |
| `page.geometry.set` | 设置整份文档的纸型、方向和边距 | 一步 |
| `para.heading` | 设大纲级别 | 一步 |
| `para.bullets` / `para.numbering` | 列表 | 一步 |
| `insert.equation` | 插入公式对象 | 一步 |
| `equation.move` / `equation.exit` | 公式内部导航、离开 | 纯命令 / 离开时若只动光标则纯命令 |
| `insert.table` | 插入表格 | 一步 |
| `table.insert_row` / `table.delete_row` / `table.insert_col` / `table.delete_col` | 改表格形状 | 一步 |
| `struct.next` / `struct.prev` / `struct.exit` | 结构跳转 | 纯命令 |
| `cmdline.begin` / `cmdline.type` / `cmdline.accept` / `cmdline.cancel` | 命令模式 | begin、type、cancel 是纯命令；`accept` 执行目标命令，由目标命令记账 |
| `file.save` | 把当前文档写成 `.cdoc` | 不改文档，失败不丢内存里的稿 |

结构命令的完整行为和键位见[结构导航](structure-navigation.md)和[快捷键](../interface/keyboard-shortcuts.md)。

新增命令时同时写：它在公式内、表格内、命令模式里是否可用；它会不会改变 `rev`。没写明的命令在公式内先退出公式再执行。`struct.*` 和 `cmdline.*` 必须自己声明，不能靠这个默认。
