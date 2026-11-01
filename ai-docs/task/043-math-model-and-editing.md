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
- 每个公式持有 `MathTree { schemaVersion, source, root, extra }`。`source` 是与当前树对应的 LaTeX，由 `caret-math` 序列化器在树变化后重写。
- 节点是有序 `MathNode { id, kind, text, children, … }`，`MathNodeKind` 覆盖行、符号、分式、根式、上下标、定界、矩阵、大运算符、重音、文本、颜色、空白、占位符等。
- 上限：源码 1 MiB、深度 32、节点 65 536、可变载荷 8 MiB；未知字段保存在 `extra` 并原样写回。
- `defaultMathFontFamily` 是文档全局设置，未设置时使用 STIX Two Math。

编辑（`caret-engine`）：

- 行内公式在段落逻辑文本中占一个 U+FFFC（3 字节），作为原子对象参与插入、删除和光标移动。
- `Command::InsertInlineMath(source)`（`insert.inline_math`）按 `MathInputKind::Inline` 解析，解析失败返回 `edit.invalid_math` 且文档不变；成功时替换选区并把插入点放在公式之后。
- 公式是统一环境模型中的 `InlineMath`（行内层级、原子、不可解包），Mod+Backspace 跳过它解包外层环境。

非目标：自动编号、`\label`/`\ref`、编号版 `equation`/`align`（解析器报不支持并保留输入）、表格行列编辑。

## 前置条件与待决策

块级插入通过 `DocumentPatch::BlockRange` 进入撤销历史。表格单元格内公式随 045 表格模型接入。

## 实施步骤

1. 定义数学树 schema、上限与校验，实现 `.cdoc` 编解码与未知字段保留。
2. 实现解析器与序列化器（042），保证往返结构一致。
3. 接入行内公式原子对象、插入命令与失败原子性。
4. 接入块级公式插入、公式内部 `MathPos` 编辑、占位符跳转和进出公式导航。
5. 撤销/重做、保存重开与表格单元格内公式的模型覆盖；独立评审。

## 预计改动

`crates/caret-doc` 数学模型与校验，`crates/caret-math` 解析与序列化，`crates/caret-engine` 编辑命令、原子对象与环境模型。

## 验收标准

- [x] 三种公式在 `.cdoc` 中为 Caret 自有 schema，未知字段保留，保存重开稳定。
- [x] 超深、超长、节点过多的输入安全拒绝，正文不变。
- [x] 行内公式作为一个原子对象插入、删除和移动光标；无效输入不改文档。
- [x] 块级 `Equation`/`Align` 可通过命令插入并进入编辑。
- [x] 公式内部插入点移动、占位符停靠、Esc 离开公式。
- [ ] 表格单元格内公式由稳定块结构表示。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-01 | Linux：`cargo test -p caret-doc -p caret-math` | schema、上限、往返测试通过 | 通过 |
| 2026-11-01 | Linux：`cargo test -p caret-engine` | 行内公式原子插入、删除、无效输入与环境栈测试通过 | 通过 |

| 2026-11-01 | Linux：`cargo test -p caret-engine math_edit`、`cargo test -p caret-app math` | `\math`/`\equation`/`\align` → 回车 → 公式内输入与 `\` 子命令 → 退出、撤销的端到端测试通过 | 通过 |

## 风险与回退

schema 发布后靠 `schemaVersion` 与 `extra` 保持兼容。解析器只覆盖 LaTeX 数学子集，不支持的命令报错并保留输入。

## 决策与工作记录

- 2026-11-01：解析器直接产出 `caret-doc` 数学树，不保留中间 AST；`source` 始终由序列化器描述当前树。
- 2026-11-01：行内公式以 U+FFFC 原子对象进入段落文本，与格式边界分开记录。

- 2026-11-01：公式内编辑以源码为真值：`MathEdit` 保存目标公式、LaTeX 源码与源码字节偏移；每次修改重新解析，失败则拒绝且文档不变。插入点只停在花括号组内与 token 边界，控制词按整体删除。

## 完成摘要

未完成。模型、解析、序列化、三种公式的命令插入与公式内编辑已完成；表格单元格内公式待 045。
