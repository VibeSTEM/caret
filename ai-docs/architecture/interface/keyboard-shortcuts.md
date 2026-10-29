# 快捷键

更新日期：2026-10-29。正文文本输入、Enter 拆分段落、Shift+Enter 插入软换行、Backspace 删除/段落合并、方向键/Home/End 导航、复制粘贴、撤销/重做和上一块/下一块/跳出结构已接入。结构内移动、结构首尾和表格快捷键仍是后续目标。命令 id 见[命令](../editor/commands.md)。

## 路由

窗口、控件和画布分别处理自己焦点作用域内的按键；命令输入、IME、选区和结构之间的优先级由 Rust Session 单点裁决。实现位置、扩展规则和 Qt/Rust 职责见[键盘路由](keyboard-routing.md)。

正文编辑中，Enter 开始新段落，Shift+Enter 在当前段落插入软换行，Backspace 删除前一个字素簇。方向键按布局视觉位置移动，段落边界可跨段；Ctrl/Command+C 复制选中文本，Ctrl/Command+V 粘贴并替换选区。撤销/重做分别使用 Windows/Linux 的 Ctrl+Z、Ctrl+Shift+Z，以及 macOS 的 Command+Z、Command+Shift+Z。IME 与选区细节见[030](../../task/030-ime-navigation-selection.md)。

macOS 上复制、保存、撤销用 Command。结构键的修饰键按下表。Linux 和 Windows 上，结构内移动用 Ctrl+Alt。不用 Win。

Esc 和 Shift+Esc 执行同一上下文动作：先关闭有焦点的 UI 弹层；否则由 Rust 依次取消命令输入、IME 预编辑、收起选区、跳出结构。收起选区不会同时退出环境。详见[键盘路由](keyboard-routing.md)与[结构导航](../editor/structure-navigation.md)。

## 正文与结构

| 动作 | Linux / Windows | macOS |
| --- | --- | --- |
| 复制 / 粘贴 | Ctrl+C / Ctrl+V | Command+C / Command+V |
| 打开 `.cdoc` | Ctrl+O | Command+O |
| 保存 | Ctrl+S | Command+S |
| 另存为 | Ctrl+Shift+S | Command+Shift+S |
| 放大 / 缩小画布 | Ctrl+Plus / Ctrl+Minus | Command+Plus / Command+Minus |
| 撤销 | Ctrl+Z | Command+Z |
| 重做 | Ctrl+Shift+Z | Command+Shift+Z |
| 左右 / 上下移动插入点 | 方向键 | 方向键 |
| 扩展选区 | Shift+方向键 | Shift+方向键 |
| 移至视觉行首 / 行尾 | Home / End | Home / End |
| 上一块 / 下一块 | Ctrl+↑ / ↓ | Option+↑ / ↓ |
| 上一个 / 下一个同类结构 | Ctrl+PgUp / PgDn | Control+PgUp / PgDn |
| 结构内移动 | Ctrl+Alt+方向键 | Command+Option+方向键 |
| 结构的首 / 尾 | Ctrl+Alt+Home / End | Command+Option+Home / End |
| 编辑器 Escape 仲裁 | Esc / Shift+Esc | 同左 |
| 逐级扩大选区 | Ctrl+Space | Control+Space |
| 插入行或列 | Alt+方向键 | Control+Option+Shift+方向键 |
| 删除列 | Alt+Backspace / Delete | Option+Backspace / Delete |

表格里，上一块 / 下一块（Linux / Windows 的 Ctrl+↑ / ↓、macOS 的 Option+↑ / ↓）先按行移动，到首行或末行才离开表格。单元格之间的移动用结构内移动那一组。

插入行列和删除列只在表格内可用。插入方向以当前单元格为基准：↑ / ↓ 在前 / 后插入行，← / → 在前 / 后插入列。macOS 的 Option+↑ / ↓ 始终用于导航，不插入行。

## 命令与公式

交互细节见[命令输入](../editor/command-input.md)和[公式与表格](../editor/math-and-table.md)。

| 动作 | 键 |
| --- | --- |
| 进入命令模式 | `\` |
| 执行候选 / 补全别名 | Enter 执行当前候选；查询非空时 Tab 补全到查询 |
| 查询为空时插入 `\` | Tab |
| 在命令查询中移动插入点 | ← / →（只在查询文本内移动） |
| 空格 | 无效果 |
| 取消并清除已输入内容 | Esc |
| 字面 `\` | `\\` |
| 行内公式 / 显示公式 | `$` / `$$` |
| 字面 `$` | `$` 然后 Tab |
| 插入 2×2 表格 | Alt+T |
| 下一格 / 上一格 | Tab / Shift+Tab |

中文标点下用 `、` 或 `¥` 进入命令和公式，是设置里的开关，默认关。见[命令输入](../editor/command-input.md)。
