# 公式与表格

更新日期：2026-10-30。

## 公式节点和 LaTeX 语义

公式是可编辑的语义树，不是普通正文中的 LaTeX 源码。Caret 持有稳定、版本化的文档节点、占位符和 `MathPos`；第三方解析器的 AST 只存在于适配边界，不作为 `.cdoc` 持久化 schema。公式布局从同一数学树产生绘制、命中测试与光标几何。

首版采用 LaTeX 对应名称：

| Caret 节点 | LaTeX 术语 / 写法 | 位置与语义 |
| --- | --- | --- |
| `InlineMath` | inline math：`$...$`、`\(...\)` | 段落内的行内对象，内部位置用 `MathPos` |
| `Equation` | `\[...\]` / `$$...$$` | 独占一行的无编号显示公式块；不是可自动折成普通段落的文本行 |
| `Align` | `\[\begin{aligned}...\end{aligned}\]` | 多行无编号显示公式；每行有对齐单元格，`&` 是对齐点，`\\` 是公式行分隔 |

命令面板中的可读别名与公式源码分隔符是两套输入语法，不混用：`\math` 创建行内公式，`\equation` 创建单行显示公式，`\align` 创建多行对齐公式。`$...$`、`\(...\)`、`\[...\]` 等是公式源码导入语法，不作为命令候选；候选表达插入的结构，而不是源码边界标点。

首版只支持无编号的显示公式：不实现自动编号、`\label`、`\ref`；编号版 `equation` / `align` 不得静默丢弃编号语义，首版应明确报不支持并保留输入。公式源文本可保留用于错误位置和导出；解析或规范化后不保证与原始 LaTeX 字符串相同。未支持的命令必须报错并保留原内容。正文 Shift+Enter 的 `SoftBreak` 与 `Align` 行分隔是不同结构。

### 正文结构示意

下面的 JSON 展示公式节点在文档正文中的位置：行内公式属于段落的 `inlines`，显示公式和多行公式是正文块。字段名称与树形仅用于表达文档模型，不是已定稿的 `.cdoc` 序列化 schema；具体必需字段、ID、版本兼容和校验规则由 [043 模型与编辑](../../task/043-math-model-and-editing.md) 定义。Caret 数学树保留 TeX 原子与符号顺序，不解析成代数关系或运算语义。

```json
{
  "body": [
    {
      "kind": "Paragraph",
      "inlines": [
        { "kind": "Run", "text": "质能关系：" },
        { "kind": "InlineMath", "mathTree": "<compact math tree>" }
      ]
    },
    { "kind": "Equation", "mathTree": "<compact math tree>" },
    {
      "kind": "Align",
      "rows": [
        { "cells": ["<compact math tree>", "<compact math tree>"] },
        { "cells": ["<compact math tree>", "<compact math tree>"] }
      ]
    }
  ]
}
```

此示意只表达正文层级：行内公式位于段落的 `inlines`，`Equation` 和 `Align` 是正文块，`Align` 单元格包含公式树。`<compact math tree>` 是说明性占位符，不是可保存的 JSON 字符串，也不代表最终字段名或序列化格式。

### 数学树的紧凑内存表示

文档模型保留语义结构，但不为每个字符分配一个独立字符串和堆对象。实现优先评估连续 arena：节点使用紧凑 kind 与子节点范围/索引，符号使用枚举或紧凑数值，标识符与数字共享公式级文本缓冲区并以 UTF-8 范围引用；临时索引不承担跨编辑稳定 ID。这样保留脚本、分组、运算符和关系符的结构，同时避免深层 `Box` 与每符号 `String` 的碎片化。

这只是实现约束，不先锁定具体位宽或二进制 schema。043 应以典型公式、深层公式和大量公式文档测量节点数、堆分配数、驻留内存及编辑成本，再确定树节点布局和持久化编码；可读 JSON 如用于调试/交换，不要求直接作为运行时内存结构。

## 解析、数学排版和字体

优先复用 [`latex-rust`](https://docs.rs/latex-rust/latest/latex_rust/) 做 LaTeX 数学解析和 TeX 数学盒排版，评估公开 `MathNode` / `MathBox` 到 Caret 文档与画布快照的适配。Caret 不另造已有 parser/layout 功能，也不将 crate AST 持久化。公式快照应能表达 glyph、rule、基线和盒尺寸；SVG/PNG 是输出格式，不是文档模型。若公开 API 不足，先做最小 adapter 或向上游补能力，避免将 egui renderer 与 Qt 画布混接。

042 验证 crates.io `latex-rust 3.0.1` 的 MathBox 可用于 Caret 数学布局适配。继续复用它的 TeX parser/layout，通过独立低层 `caret-math` crate 隔离版本和兼容补丁，不自行实现公式 parser/layout。Caret 尚无既定 LaTeX 输入约定，因此 `Equation` 使用 `\[...\]` / `$$...$$`，`Align` 使用 `\[\begin{aligned}...\end{aligned}\]`，无需支持 `equation*` / `align*`。043 在 parser 边界补充源码字节范围和预处理偏移映射；044 提供正文 shaping provider 与布局盒到稳定数学节点的 provenance。`caret-doc` 保持持久化 schema 权威，`caret-layout` 组合 Caret 数学树、shaping 和布局快照，第三方 AST 不持久化。workspace MSRV 已提升为 Rust 1.99，与 3.0.1 元数据一致。版本、依赖校验和、原型和 patch 边界见[研究报告 010](../../research/010-latex-rust.md)。

LaTeX 没有唯一公式字体。M3 首版统一使用 `latex-rust` 内嵌的 STIX Two Math，使三平台不依赖本机字体安装，并使用与其 MATH 表布局一致的字形度量。文档保留 `defaultMathFontFamily` 作为稳定扩展入口；未设置时默认解析为 `STIX Two Math`。M3 当前只支持该字体，不提供多字体切换 UI；其他值保持原样，渲染回退到 STIX Two Math 时提供诊断。未来扩大支持列表时，应另行处理字体度量、缺字回退、缓存与 `.cdoc` 重现性。

公式中的 `\text{...}` 属于正文文本，不应强制由 STIX Two Math 覆盖。适配原型需验证将其内容交给 Caret 的文字 shaping（含文档语言与字体回退）并作为数学文本原子放入 MathBox 的可行性；不支持的文本内容必须保留源文本并显示可定位错误，不能静默换成缺字方框。

解析输入和手建树都受明确的最大嵌套深度约束；依赖版本不得包含已知深层嵌套导致栈溢出的版本。布局失败不能伪装成成功，也不得破坏现有公式源内容。

## 导航和编辑

- `\math` 插入行内公式，`\equation` 插入单行显示公式，`\align` 插入多行对齐公式；三者均进入公式编辑。`$...$`、`\(...\)`、`\[...\]` 是公式源码解析/导入语法，不是正文命令入口。
- 结构命令可显式插入 `Align`；`aligned` 输入中的 `&` 与 `\\` 映射为对齐点与行节点。
- 方向键在公式边界进出。Esc 离开公式，插入点落到公式之后。
- 公式内部可以上下左右移动，也可以跳到下一个占位符；`_` 和 `^` 插入下标与上标节点。
- 正文命令模式不会在公式内部打开。公式有自己的命令解析；其中的 `\\` 仅在 `Align` 内结束一行。

离开公式只移动插入点，不产生撤销步。改树的命令才记账。

## 表格

表格是块。单元格里是普通的块序列，所以单元格里可以有多段，也可以再放公式。第一版不允许单元格合并。

- 表格单元格导航的目标行为：Tab 到下一格，最后一格再按 Tab 增加一行；Shift+Tab 到上一格。该行为尚未接入当前 `EditorKey::Tab` 路由；实现时由 Rust 先处理表格上下文，再回退到正文空格行为。
- 单元格里按 Enter 是新起一段，不是在表外插入行。
- 方向上的结构键在单元格之间移动，到边缘再离开表格。
- 插入默认 2×2。增删行列是独立命令。

大表格只重排 `layoutRev` 变了的单元格内容。整表重排的开销在 M4 用一份宽表测量，测量前不设毫秒门禁。

## M3 集成验收

[011 总体验收](../../task/011-impl-math.md)由 [042 后端验证](../../task/042-latex-rust-math-backend.md)、[043 模型与编辑](../../task/043-math-model-and-editing.md)及 [044 布局、渲染与集成](../../task/044-math-layout-and-rendering.md)拆分推进。042 可先独立进行，043 依赖 010 与 042，044 依赖 012 表格和 027 画布。[012 表格总体验收](../../task/012-impl-table.md)由 [045 模型与交互](../../task/045-table-model-and-editing.md)及 [046 布局与画布](../../task/046-table-layout-and-canvas.md)拆分；表格模型只持有通用 Caret 块序列，不依赖 `latex-rust`。044 负责接通单元格内公式的插入、编辑、导航、绘制、命中、撤销和保存往返。M3 只有在 011 汇总验收和单元格集成证据齐全后才算完成，013 也只有在该证据存在时才能开始。
