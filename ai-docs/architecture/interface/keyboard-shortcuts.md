# 快捷键

更新日期：2026-10-24。

键位的语义借鉴 Mogan：结构遍历、结构内移动、结构内插入是三组不同的修饰。Mogan 是 GPL。Caret 只采用「哪个键做什么」，绑定表写在自己的命令注册表里，不复制它的代码、键表或注释。

macOS 上 Command 用于复制、保存这类通用编辑键。结构键用 Control 和 Option，避免和系统键抢。Linux 和 Windows 上没有单独的 Super 键位：结构内移动用 Ctrl+Alt，不用 Win。

Esc 不是粘滞修饰键。它按下面的顺序用掉一次：退出命令模式，离开公式，有选区时收到插入点，再否则跳出当前结构。

## 正文与结构

| 动作 | Linux / Windows | macOS |
| --- | --- | --- |
| 上一块 / 下一块 | Ctrl+↑ / ↓ | Option+↑ / ↓ |
| 上一个 / 下一个同类结构 | Ctrl+PgUp / PgDn | Control+PgUp / PgDn |
| 结构内移动 | Ctrl+Alt+方向键 | Command+Option+方向键 |
| 结构的首 / 尾 | Ctrl+Alt+Home / End | Command+Option+Home / End |
| 跳出结构 | Esc / Shift+Esc | 同左 |
| 逐级扩大选区 | Ctrl+Space | Control+Space |
| 插入行或列 | Alt+方向键 | Option+方向键 |
| 删除列 | Alt+Backspace / Delete | Option+Backspace / Delete |

表格里，Ctrl+↑ / ↓ 先按行移动，到首行或末行才离开表格。单元格之间的移动用结构内移动那一组。

## 命令与公式

| 动作 | 键 |
| --- | --- |
| 进入命令模式 | `\` |
| 执行候选 | Enter；查询非空时 Tab 也可以 |
| 查询为空时插入 `\` | Tab |
| 空格 | 无效果 |
| 取消并清除已输入内容 | Esc |
| 字面 `\` | `\\` |
| 行内公式 / 显示公式 | `$` / `$$` |
| 字面 `$` | `$` 然后 Tab |
| 插入 2×2 表格 | Alt+T |
| 下一格 / 上一格 | Tab / Shift+Tab |

中文标点下用 `、` 或 `¥` 进入命令和公式，是设置里的开关，默认关。见[命令输入](../editor/command-input.md)。
