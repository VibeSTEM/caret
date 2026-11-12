# 089 — InlineEnvironment 普通 Backspace 清理语义

- 状态：implemented
- 依赖：031、044、075
- 优先级：P1
- 创建 / 更新：2026-11-12 / 2026-11-12

## 目标与背景

桌面测试发现：`\bold` 等 InlineEnvironment 只有 Mod+Backspace 能删除，普通 Backspace 无法清理零宽空环境。根因是 `handle_backspace`（`crates/caret-engine/src/editing/session/keyboards/backspace.rs`）除 IME/命令行外一律 `Delete(Backward)`，不检查光标处的空 InlineEnvironment；空环境对 TextMap 没有可删文本，普通退格只会删前一个字符或合并段落，环境永久残留。Mod+Backspace 走 `RemoveInlineEnvironmentPreservingContent`，是唯一移除路径。

`\math` 的既有语义是：内部先删内容，空宿主再删对象。本任务让 InlineEnvironment 与之一致，并保留 Mod+Backspace 的“解除环境、保留内容”区别。

## 必读

- [行内格式与文本](../../architecture/editor/text-formatting.md)、[数学公式](../../architecture/editor/math.md)
- [验证与评审](../../standards/validation-and-review.md)、[测试规范](../../standards/testing.md)
- [031](031-environment-highlight.md)、[044](044-math-layout-and-rendering.md)、[075](075-equation-align-active-highlight.md)

## 范围与非目标

- 普通 Backspace：光标在空 InlineEnvironment 内部或右侧边界时，清理最内层空环境（一次一层）；非空环境先删文字，删空后下一次退格清理；左外侧边界保持删除前一个字符，不清理。
- 保持优先级：IME > 命令输入 > 公式编辑 > 结构选区 > 块边界停靠 > 空环境清理 > 普通文本删除。
- 非目标：不改变 Mod+Backspace 的保留内容语义；不做正向 Delete 的对称清理；不改结构范围删除路径。

## 前置条件与待决策

- 已确认 `TextMap` 提供 `empty_inline_environment_stack_at` / `empty_inline_environment_stack_for_host`，普通 Backspace 未使用；`inline_environment_stack_at` 只包含非零长度环境，空环境不可见。
- 无显式 host 状态时按“就近空环境优先”兜底；代价是正文 `ab` + 空 Bold、光标在环境右侧但无 host 状态时，第一次退格清理环境而非删除 `b`。有显式左外侧 host（Before）时仍删字符。

## 实施步骤

1. `handle_backspace` 在 composition/命令行/公式/结构选区/块边界之后增加空环境判定，命中则复用 `RemoveInlineEnvironmentPreservingContent`。
2. 新建 `keyboards/tests.rs` 覆盖 Bold/Font/Underline、嵌套逐层、IME/命令行、结构选区优先、Undo/Redo、与 Mod+Backspace 区分。
3. app `tests/keyboard.rs` 增加真实 `handle_editor_key_json("backspace")` 路径回归。
4. 独立评审并同步文档。

## 预计改动

`crates/caret-engine/src/editing/session/keyboards/backspace.rs`、`keyboards.rs`（测试模块接线）、`keyboards/tests.rs`（新）、`crates/caret-app/src/document_view/tests/keyboard.rs`。

## 验收标准

- [x] 空 InlineEnvironment 内部/右侧边界普通退格清理环境，一次一层，单步历史与 Undo/Redo 正确。
- [x] 非空环境先删文字，删空后再清理；左外侧边界与 Mod+Backspace 语义保持。
- [x] IME/命令行/公式编辑/结构选区/块边界优先级不变；Underline 等 RunProperty 不受影响。
- [x] 格式、lint、engine/app 测试与独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-12 | `cargo test --locked -p caret-engine editing::session::keyboards` | 新增 11 条退格回归通过 | 通过：11 passed / 0 failed（含 SetSelection 清 host 后左外侧兜底用例） |
| 2026-11-12 | `cargo test --locked -p caret-engine`、`-p caret-app` | 全量无回归 | 通过：engine 867、app 325，0 failed |
| 2026-11-12 | workspace clippy `-D warnings`、fmt、workspace 测试 | 无警告/回归 | 通过 |
| — | Qt 桌面手工退格观感 | 与 `\math` 一致 | 未执行（本环境无窗口） |

## 风险与回退

- 无 host 状态的兜底清理可能早于删除左侧字符，属语义取舍；若产品希望先删字符，可收紧为仅在 host 状态命中时清理（回退到 Mod+Backspace 行为）。
- 旧 API `delete_backward_json` 直接发 `Command::Delete`，语义不变；QML 走 `EditorKey::Backspace` 路径。

## 决策与工作记录

- 2026-11-12：从桌面测试发现中拆出并实现，任务归属 031/044（行内环境删除契约），不在 083 的跨宿主选区范围。
