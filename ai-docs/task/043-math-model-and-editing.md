# 043 — M3：公式文档模型与编辑

- 状态：in-progress
- 依赖：010、042
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-01

## 目标与背景

为可编辑公式建立 Caret 自有的版本化数学树，并实现公式插入、内部编辑、位置与键盘导航。总体验收由 [011 M3 公式](011-impl-math.md)管理。

## 必读

- [公式与表格](../architecture/editor/math-and-table.md)、[文档模型](../architecture/editor/document-model.md)、[结构导航](../architecture/editor/structure-navigation.md)
- [命令输入](../architecture/editor/command-input.md)、[存储格式](../architecture/editor/document-model/document-storage.md)
- [测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

模型（`caret-doc`）：

- `InlineMath` 是段落行内节点；`Equation` 与 `Align` 是正文块，`Align` 由行与单元格组成。
- 每个公式持有 `MathTree { schemaVersion, source, root, extra }`。编辑会话以 LaTeX `source` 为输入权威，每次修改源码后重新解析得到当前 `root`；解析失败时不提交源码或树。树到 LaTeX 的序列化器用于规范化输出和往返验证，不假定所有树编辑都会调用它。持久化时校验 source 与 root 的一致性，避免保存不匹配的数据。
- 节点是有序 `MathNode { id, kind, text, children, … }`，`MathNodeKind` 覆盖行、符号、分式、根式、上下标、定界、矩阵、大运算符、重音、文本、颜色、空白、占位符等。
- 上限：源码 1 MiB、深度 32、节点 65 536、可变载荷 8 MiB；未知字段保存在 `extra` 并原样写回。
- `defaultMathFontFamily` 是文档全局设置，未设置时使用 STIX Two Math。

编辑（`caret-engine`）：

- 行内公式在段落逻辑文本中占一个 U+FFFC（3 字节），作为原子对象参与插入、删除和光标移动。
- `Command::InsertInlineMath(source)`（`insert.inline_math`）按 `MathInputKind::Inline` 解析，解析失败返回 `edit.invalid_math` 且文档不变；成功时替换选区并把插入点放在公式之后。
- 公式是统一环境模型中的 `InlineMath`（行内层级、原子、不可解包），Mod+Backspace 跳过它解包外层环境。

非目标：自动编号、`\label`/`\ref`、编号版 `equation`/`align`（解析器报不支持并保留输入）、表格行列编辑。

## 前置条件与待决策

块级插入通过 `DocumentPatch::BlockRange` 进入撤销历史。表格单元格内公式的模型编辑归 045，布局与画布归 046。

## 实施步骤

1. 定义数学树 schema、上限与校验，实现 `.cdoc` 编解码与未知字段保留。
2. 实现解析器与序列化器（042），保证往返结构一致。
3. 接入行内公式原子对象、插入命令与失败原子性。
4. 接入块级公式插入、公式内部源码游标编辑、占位符跳转和进出公式导航。
5. 撤销/重做、保存重开、source/root 一致性和失败原子性；独立评审。

## 预计改动

`crates/caret-doc` 数学模型与校验，`crates/caret-math` 解析与序列化，`crates/caret-engine` 编辑命令、原子对象与环境模型。

## 验收标准

- [x] 三种公式在 `.cdoc` 中为 Caret 自有 schema，未知字段保留，保存重开稳定。
- [x] 畸形或不匹配的 source/root 数据在进入可编辑会话与应用保存入口时安全拒绝；合法编辑往返保持一致。
- [x] 超深、超长、节点过多的输入安全拒绝，正文不变。
- [x] 行内公式作为一个原子对象插入、删除和移动光标；无效输入不改文档。
- [x] 块级 `Equation`/`Align` 可通过命令插入并进入编辑。
- [x] 公式内部插入点移动、占位符停靠、Esc 离开公式。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-01 | Linux：`cargo test -p caret-doc -p caret-math` | schema、上限、往返测试通过 | 通过 |
| 2026-11-01 | Linux：`cargo test -p caret-engine` | 行内公式原子插入、删除、无效输入与环境栈测试通过 | 通过 |
| 2026-11-01 | Linux：`cargo test -p caret-engine math_edit`、`cargo test -p caret-app math` | `\math`/`\equation`/`\align` → 回车 → 公式内输入与 `\` 子命令 → 退出、撤销的端到端测试通过 | 通过 |
| 2026-11-01 | macOS：`cargo test --locked --workspace` | source/root 语义校验、公式状态迁移、模型和布局回归通过 | 通过；包含 native CTest 8/8 |
| 2026-11-01 | macOS：分别运行 `cargo test -p caret-engine --locked align_cell_mapping`、`cargo test -p caret-engine --locked empty_equation_keeps_a_visible_math_caret`、`cargo test -p caret-engine --locked empty_align_keeps_a_visible_math_caret`、`cargo test -p caret-engine --locked math_history_estimate_counts_nodes_and_extension_payloads`、`cargo test -p caret-math --locked empty_align_keeps_an_editable_placeholder_cell` | 嵌套环境分隔符、空公式插入点、历史内存估算和空对齐单元格通过 | 通过 |

## 风险与回退

schema 发布后靠 `schemaVersion` 与 `extra` 保持兼容。解析器只覆盖 LaTeX 数学子集，不支持的命令报错并保留输入。

## 决策与工作记录

- 2026-11-01：解析器直接产出 `caret-doc` 数学树，不保留中间 AST；编辑以 source 为真值并在成功解析后同步更新 root。
- 2026-11-01：行内公式以 U+FFFC 原子对象进入段落文本，与格式边界分开记录。

- 2026-11-01：公式编辑以 LaTeX 源码和 UTF-8 焦点偏移为输入状态；公式树适配器负责把同一焦点映射到分子、分母等结构节点。控制词对应的可见符号作为一个停靠点，普通内容按字素移动；分式相邻参数间的 `}{` 是一步结构导航。空参数退格删除其空结构，多余 `}` 输入拒绝且不改变文档。`InlineMath` 是段落行内载体，`\frac` 是其 `MathTree` 内部的 `MathNodeKind::Fraction`，不另建行内环境。
- 2026-11-01：`caret-doc` 只负责稳定 schema 与结构约束；依赖解析器的语义校验放在 `caret-engine::Session::new` 和 app 存储边界，避免 `caret-doc` 反向依赖 `caret-math`。
- 2026-11-01：编辑重解析时保留数学树及可唯一映射的未修改节点 ID 与 `extra`；公式历史估算遍历节点、扩展字段及对齐行/单元格，不再按源码长度乘常数估算。
- 2026-11-01：嵌套 LaTeX 环境中的 `&` 与 `\\` 不作为外层 `aligned` 单元格/行分隔符；空 `aligned` 持有一个占位单元格，确保 MathEdit 有有效插入点。

## 完成摘要

未完成。模型、解析、序列化、三种公式的命令插入与公式内编辑、可编辑会话和保存边界的 source/root 语义校验已接通。公式输入法预编辑仍没有公式专用状态与预览路径；公式内 Undo/Redo 尚未恢复公式焦点和源码偏移。实现前需覆盖替换、取消、失败恢复与撤销契约；表格单元格公式模型、编辑目标和导航归 045。
