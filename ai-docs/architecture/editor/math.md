# 数学公式

更新日期：2026-11-01。本文是 Caret 数学文档模型、LaTeX 子集、公式编辑语义和字体选择的唯一权威。任务进度见[011 总体验收](../../task/011-impl-math.md)及其拆分任务；公式与表格之间的工作边界见[专题索引](math-and-table.md)。

## 公式节点与 LaTeX 语义

公式是可编辑的语义树，不是普通正文中的 LaTeX 源码。Caret 持有稳定、版本化的文档节点与数学树；解析器直接产出这棵树，没有中间 AST。042 已实现数学树、解析/序列化和独立数学帧 API。`MathEdit`、页面布局、命中测试、光标几何与 Qt 绘制由 043/044 接入；能力完成前，含数学特性的文档以只读安全预览方式打开。

首版采用 LaTeX 对应名称：

| Caret 节点 | LaTeX 术语 / 写法 | 位置与语义 |
| --- | --- | --- |
| `InlineMath` | inline math：`$...$`、`\(...\)` | Paragraph 行内原子项；公式内部位置由 Math 编辑适配器维护 |
| `Equation` | `equation*` / `\[...\]` / `$$...$$` | 独占一行的无编号显示公式块；不是可自动折成普通段落的文本行 |
| `Align` | `align*` | 多行无编号显示公式；每行有对齐单元格，`&` 是对齐点，`\\` 是公式行分隔 |

命令面板中的可读别名与公式源码分隔符是两套输入语法，不混用：完整接入后，`\math` 创建行内公式，`\equation` 创建单行显示公式，`\align` 创建多行对齐公式。`$...$`、`\(...\)`、`\[...\]` 等是公式源码导入语法，不作为命令候选。当前行内公式的持久化与绘制/光标投影、显示公式分页布局尚未接入，因此三个命令候选保持不可执行；底层 Session 能力不代表桌面功能已开放。

首版只支持无编号显示公式：不实现自动编号、`\label`、`\ref`；编号版 `equation` / `align` 不得静默丢弃编号语义，首版应明确报不支持并保留输入。公式源文本可保留用于错误位置和导出；解析或规范化后不保证与原始 LaTeX 字符串相同。未支持的命令必须报错并保留原内容。正文 Shift+Enter 的 `SoftBreak` 与 `Align` 行分隔是不同结构。

MathTree 是已提交文档内容的权威。源码模式编辑使用临时 source buffer；接受时解析并与原树对齐，随后原子提交新树与规范化 source。TextFlow 模式直接改写目标文本流，并由同一事务更新所属源片段。能由首版数学源码表达的 TextFlow 格式必须有规范表示；源码无法表达的属性与未知字段沿稳定节点/源范围映射保留。MathFlow、MathNode、MathContent 中的 TextFlow 与 InlineEnvironment 使用相对 `MathTree.source` 的半开 UTF-8 `sourceRange`。若本次源码编辑会丢失无法安全映射的字段，拒绝提交并保留原树、有效焦点和错误信息。解析、序列化、范围或事务失败不得改写现有公式。

数学 `TextFlow.sourceMap` 由解析器生成，逐段映射拼接 Run 文本中的 UTF-8 字节区间到 `MathTree.source`。转义符号、`\textchar` 和折叠空白会让显示长度与源码长度不同；这类区间只能在映射项边界处编辑，不能按字节长度直接换算。空 `\text{}` 使用零长度文本区间和源码锚点保留编辑入口。正文 TextFlow 不允许 sourceRange 或 sourceMap；数学 source mapping 由 `caret.math.text-flow-source-map.v1` 能力声明。

### 文档树中的位置

段落中的公式是 `ParagraphContent::Math(InlineMath)`；显示公式 `Equation` 与 `Align` 是块节点。段落与行内树的结构字段由[文档树与结构](document-model/document-structure.md)定义，公式与文本的适配规则由[行内与块级环境适配](document-model/inline-and-block-environments.md)定义。

```text
Document.body: Vec<Block>
├── Paragraph
│   └── inlines: Vec<ParagraphContent>
│       ├── Text(TextFlow)
│       ├── Math(InlineMath { id, math_tree })
│       └── Environment(InlineEnvironment<ParagraphContent>)
├── Equation { id, math_tree }
└── Align { id, source, rows: Vec<MathAlignRow { cells: Vec<MathTree> }> }
```

Caret 数学树保留 TeX 原子与符号顺序，不解析成代数关系或运算语义。`MathTree { schemaVersion, source, root, extra }` 的字段与验证由本页定义：`root` 是 `MathFlow`，`MathContent` 按源码顺序交替保存 `Math(MathNode)`、`Text(TextFlow)` 和行内格式环境。分数、根式、脚本等结构节点的每个语义槽位各持有一个有序 `MathFlow`；流内切分不得增加、删除或重排这些固定槽位。行内环境是 `InlineEnvironment<Child>` 包装节点，`ParagraphContent::Environment` 使用 `InlineEnvironment<ParagraphContent>`，`MathContent::Environment` 使用 `InlineEnvironment<MathContent>`；共享包装语义与树操作，子项类型由宿主流约束。`source` 与树在成功提交后保持规范表示一致。文档级能力登记和未知能力行为见[文档存储](document-model/document-storage.md)。schema 尚未发布前，不为临时字符串 `TextContent` 格式提供兼容或迁移路径。

### 数学树限制

每个节点只保存自身的原子文本和有序子节点；单棵树受深度 32、节点 65,536、可变载荷 8 MiB 与源码 1 MiB 的上限约束。行内公式在段落逻辑文本中占一个 U+FFFC（3 字节），作为原子对象参与编辑和布局。若测量显示大量公式文档的内存或编辑成本成为热点，再评估连续 arena 等紧凑表示，不改变 `.cdoc` 编码。

## 解析、排版与字体

公式后端是工作区内的 `caret-math` crate，选型依据见[研究 010](../../research/010-typst-math-layout.md)：

- **解析**：自写 LaTeX 数学子集解析器直接产出 `caret-doc` 的 `MathTree` / `Align`；序列化器把树写回 LaTeX。纯源码往返比较解析器可表示的结构投影（不含稳定 ID、源范围、未知字段及其他非源码元数据）；从现有树开始编辑源码时，还必须把解析结果与原树对齐并恢复可映射元数据，否则按第 21 行的规则拒绝提交。
- **排版**：`layout/` 移植自 Typst v0.15.1 的数学排版，度量来自 OpenType MATH 表，经 `read-fonts` / `skrifa` 读取。移植文件的来源与修改记录在文件头，见[源码署名](../../standards/source-attribution.md)。
- **输出**：042 的 `MathLayout` 独立产出 Glyph、Rule、Line、Rectangle 和 MathNodeBox 项及其几何；044 将这些项适配到 Caret 页面快照，并让绘制、命中和光标共用几何。SVG/PNG 不是文档模型，也不是交互路径。
- **缓存**：042 的 `MathLayoutEngine` 按公式内容、字号、样式与字体缓存排版结果；044 的布局服务负责组合和复用。

M3 首版内置 STIX Two Math（OFL），三平台不依赖本机字体。文档 `defaultMathFontFamily` 使用统一 `<default>` 字段值；当前 `MathLayoutEngine` 仍固定使用内置 STIX，尚不读取该设置或为显式其他值生成诊断。044 接通数学字体选择后再实现候选解析和回退诊断。字体角色默认标记的定义见[字体选择规范](font-selection.md)。未来扩大支持列表时，应另行处理字体度量、缺字回退、缓存与 `.cdoc` 重现性。

## 公式中的普通文本

`\text{...}` 是公式中的 Text 编辑分支，与 Math 源码编辑模式平级。Paragraph 的行内流以 `Vec<ParagraphContent>` 表达；枚举分支包含 `Text(TextFlow)`、`Math(InlineMath)`、`Environment(InlineEnvironment<ParagraphContent>)`，并为未来的 `Code(CodeSpan)` 保留扩展位置。043 从现有 `Paragraph.inlines` 及其编辑操作抽取共享 `TextFlow`，不另写文本编辑器。TextFlow 只保存文本 Run；Math 和 InlineEnvironment 是同一父序列的其他分支。

`TextFlow` 复用正文 Run、Run 属性、未知字段、字素边界、选区、输入法、格式事务和撤销逻辑；其节点边界以[文档树与结构](document-model/document-structure.md)为准。`InlineEnvironment` 通过通用混合树操作复用格式范围、嵌套顺序与稳定 ID 规则。适配器携带宿主路径，直接在所属内容树调用这些操作；数学源码节点仍由 Math 编辑逻辑处理。公式树与正文共用文本编辑实现，但各自保留文档节点、位置身份、事务和失败边界。文本岛编辑直接修改共享树分支，不拍平或复制第二份可变文本；同步 source 与数学树的成本计入同一原子事务。

IME 只在活动分支为 TextFlow 时启用。数学源码及数学命令查询不接受预编辑、替换或组字预览；布局接入后 Qt 的 `ImEnabled` 状态由 Rust 活动分支投影，TextFlow 的替换事务仍复用正文实现。

当前 043 仅接入数学源码分支，因此公式焦点下的正文格式命令会拒绝执行，避免修改宿主段落的 pending style；TextFlow 编辑分支接入后，这些命令才由宿主路径路由到文本流。

### 宿主边界与拆分

普通正文执行 `\math` 命令时，Paragraph 流插入一个 `InlineMath` 原子。该 InlineMath 根流含有 TextFlow 时，必须按顺序将根流切成相邻的 `ParagraphContent::Math` 与 `ParagraphContent::Text` 项，让普通文本回到正文编辑器。

若根流子项被 Bold、Italic、Font 等 `InlineEnvironment` 包装，转换其 children 中的 Math/Text 项，但保留一个对应的 `InlineEnvironment<ParagraphContent>` 包装及原有范围、顺序和 ID。例如 `\math(\bold{a\text{x}b})` 规范化为一个 Bold 包装，其 children 为 Math、Text、Math；不拆成三个相邻 Bold。不得因包装环境改变提升结果。

分数、根式、上下标等内部槽位的 `\text{...}` 始终留在该槽位自己的 MathFlow 中。在这类文本岛内输入 `\math` 时，只结束当前 TextFlow，并在同一槽位插入 MathNode 兄弟项，后续文本成为新的 TextFlow，绝不提升到 Paragraph，以免改变分式操作数、脚本作用域和数学排版。Equation/Align 没有 Paragraph 宿主，其根流始终保留数学混合内容。

### 可复用的 split/merge

统一 split/merge 操作接收强类型有序流和宿主路径，由宿主适配器访问节点与位置。ParagraphContent 与 MathContent 共用同一操作策略，但不抹平成无类型数据；Bold、Italic、Font 只声明包装语义，不各自实现拼接算法。根流提升时，`InlineEnvironment<MathContent>` 转为 `InlineEnvironment<ParagraphContent>`，每个输入包装仍对应一个输出包装，不因子项类型转换而复制节点。只有其他编辑操作确实把同一包装拆为多个非连续范围时，才创建多个包装节点；原稳定 ID 保留在首个存活的同 kind/同语义包装上，其他节点分配新 ID。InlineMath 宿主 ID 保留在首个仍存在的 Math 输出项；若输出中没有 Math 项，则该 ID 从树中移除。MathNode 内部稳定 ID 原样保留。无 ID 的节点不凭空生成身份。

Run 与环境的未知字段按实际归属保留；只有能证明安全复制或归属唯一时才迁移到新片段，否则整笔编辑原子拒绝。合并只在未知字段、属性和身份可无损处理时发生。位置映射同步更新光标、选区与环境范围；Underline 由独立 Run 属性适配器处理。一次切换是一个 Rust 文档事务。

文本岛的布局复用正文 shaping 的字体回退、语言和 Run 样式，但不获得段落块布局、段落前后距或分页语义。数学源码支持范围如下；标准 LaTeX 导出遇到无法表示且会影响外观或语义的属性时返回明确损失错误，不静默降级。

| 内容 | 数学源码表示 | 编辑/导出规则 |
| --- | --- | --- |
| TextFlow 文本与首版支持的 InlineEnvironment | `\text{...}` 及明确支持的格式命令 | 规范序列化；非规范源解析后可规范化 |
| Run 的 language、underline 与未知字段；Font 环境的字体覆盖 | 普通 LaTeX 子集通常不能完整表达 | language、underline 和未知字段保存在 Run 元数据中；字体选择保存在 `InlineEnvironment<MathContent>` 的 Font 属性中。源码重编辑须通过稳定节点/源范围映射恢复，无法唯一对齐时拒绝；标准 LaTeX 导出遇到影响外观或语义的元数据时返回明确损失错误。Font 环境的任意覆盖与 `fontFamily` 源命令语义仍需无损映射设计，当前不承诺支持任意覆盖 |
| 编辑器命令 `\math` | 非 LaTeX 源码语法 | 正文和公式 TextFlow 编辑模式都可用的结构命令；`.cdoc` source 保存解析后的数学内容，不保存命令面板别名 |

不得用重复文本的模糊搜索匹配节点身份。源码表示与树元数据的对齐必须使用解析结构和稳定源范围；无法唯一映射时拒绝提交，保留原树和焦点。布局契约与验证由[044](../../task/044-math-layout-and-rendering.md)维护。

共享 TextFlow 消除重复策略，但本身不证明性能提升。现阶段没有目标模型的性能或体积实测，也不能从当前分支克隆 Paragraph 的编辑路径推断已经实现局部更新。评估时固定机器、工具链和构建配置，用长纯文本、密集短 Run、数千短行内公式、中日韩与组合字符文本岛、深层嵌套及接近节点/源码限制的大公式，记录解析/序列化与打开/保存耗时、输入/拆分/撤销延迟分布、分配次数、峰值内存、枚举尺寸和 `.cdoc` 字节数；对照当前实现保存可复现基线，再判断是否需要改用间接存储或局部撤销快照。

## 导航与编辑

本节定义用户行为；具体实现状态和测试证据见[043](../../task/043-math-model-and-editing.md)。

- 完整产品路径接入后，`\math`、`\equation`、`\align` 分别插入行内公式、单行显示公式、多行对齐公式，并进入公式编辑。当前三个命令在桌面命令面板中保持不可执行，避免创建无法保存或呈现的文档内容。
- 公式编辑状态记录目标公式、当前编辑分支和该分支的位置。源码模式使用 UTF-8 字节位置；TextFlow 模式使用正文文本位置语义。切换模式时，适配器用公式树位置映射恢复焦点；失败时保留原文档与有效位置。
- 数学 TextFlow 的 `SetMathTextSelection` 以当前 TextFlow 内 UTF-8 字节偏移设置 anchor/focus；两端必须落在同一流内的 grapheme 边界。输入和退格/删除替换该选区，左右键将选区折叠到对应边界。文本到源码的映射若不连续或无法无损覆盖，操作原子拒绝。跨 TextFlow/InlineEnvironment 的选区等待槽位级物理路径映射，不用单岛路径伪装支持；此命令提供 engine 位置入口，不代表 044 的命中与选区绘制已接入。
- 左右键在花括号组、token 边界间移动；控制词整体跳过与删除。到达边界再按一次离开公式；方向键、鼠标、选区命令、Esc 与 Enter 可退出公式。
- `_` 与 `^` 插入带空组的下标与上标，`{` 自动补 `}`，`}` 跳出组。
- 公式编辑中 `\` 打开数学命令，不打开正文命令；`Align` 中 Enter 插入 `\\` 结束一行，`&` 为对齐点。
- 空公式中退格删除整个公式对象。只移动插入点的离开操作不产生撤销步；修改文档树的操作才记账。
- 当前 MathEdit 暂不定义 Tab 槽位导航；Tab 保留公式目标，不向宿主段落插入空格。
