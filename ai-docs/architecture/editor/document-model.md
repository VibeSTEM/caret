# 文档模型

更新日期：2026-10-25。

> 产品已决定使用 Paragraph/Run 作为文本表示。020 继续细化 Caret 的语义块、位置、编号、删除和持久化契约；下列字段仍是草案，不等于照搬 OOXML，也不承诺 DOCX 兼容。

## 树

语义骨架草案：

```text
Document
├── styles: StyleSheet { generation }
├── body: Vec<Arc<Block>>
│   ├── EnvironmentNode { id, kind, scope, props, title?: Paragraph, children }
│   │   ├── inline scope: Run | nested inline EnvironmentNode | inline object
│   │   └── block scope: Paragraph | nested block EnvironmentNode | Table | ExplicitBreak
│   ├── Paragraph { props, inlines: Vec<InlineNode> }
│   ├── Table(Table → rows → cells → body)
│   └── ExplicitBreak
└── pageSections: Vec<PageSection>
    └── props: PageSectionProps { pageSize, orientation, margins, ... }
```

```text
Block = Paragraph | EnvironmentNode(scope: block) | Table | ExplicitBreak
InlineNode = Run | EnvironmentNode(scope: inline) | InlineObject | SoftBreak

EnvironmentNode(scope: block).children: Vec<Block>
EnvironmentNode(scope: block).title: Option<Paragraph>  # 与 children 分开
EnvironmentNode(scope: inline).children: Vec<InlineNode>

Paragraph
├── inlines       # Run、行内 EnvironmentNode 和行内对象组成的有序内容
├── markStyle     # 段落标记/空段落的有效字符格式
├── props         # 段落样式、缩进与间距
└── rev           # 全局递增，不参与相等，作为布局缓存键
```

`children` 按顺序保存环境拥有的正文节点：块级环境存段落、嵌套块环境、表格等 `Block`；行内环境存 Run、嵌套行内环境等 `InlineNode`。块级环境的 `title` 单独保存，不是 `children` 的首项。删除整个块环境会删除标题和子树；提升子项只移动其正文 `children`。

已决定使用 Paragraph/Run 作为文本表示：Paragraph 是段落边界和段落级属性的载体，Run 是连续文字及字符级格式的叶子节点。EnvironmentNode 可以是行内或块级作用域，由环境 kind 声明允许的子节点：

- 粗体和斜体只能由行内 EnvironmentNode 表示，Run 不携带粗体或斜体直接覆盖；如粗体内嵌斜体，格式按父子作用域组合。EnvironmentNode 的继承和显式关闭规则由 020 定稿。
- Showcase、章节等可以是块级 EnvironmentNode，拥有块子项、可选标题和编号规则；行内环境没有标题。
- 当前环境背景高亮是由光标位置派生的视图叠加，不属于持久化模型；行为见[结构导航](structure-navigation.md)。
- 章节编号 `1.` 由编号定义生成，不烘焙进标题 Run，所以插入、移动或删除同级章节环境时可重新编号。
- `Table`、公式对象和显式分页仍是有类型的语义块/对象；自动分页只存在于布局结果。
- `PageSection` 只表示纸张、方向和边距等分页配置，与用户可见的 `EnvironmentNode` 分开，避免把页面分节和内容环境混为一谈。

块级环境可折叠、可选中和通过结构命令整体删除，默认删除该节点、独立标题及子树；若要保留子内容，提供明确的“解除环境/提升子项”操作。行内格式环境按文字范围编辑和删除。两者都不能让普通文本删除隐式破坏外层结构。命令事务、失败原子性和后续撤销由 018 衔接。环境种类、scope、编号定义、嵌套约束、标题的位置映射及块移动后的 ID/位置映射由 task 020 定稿。

普通标题仍可以只是带 `outline_level` 的 Paragraph；需要显式拥有/删除一组子内容时才用块级 EnvironmentNode。环境标题中的自动编号和标题样式属于可独立编辑的属性，不是靠解析标题文本推断。

字符样式候选值需要区分未指定、显式关闭和具体值，避免把“继承”压成 false。样式解析优先级及主题引用见[格式语义](text-formatting.md)。

页属于排版结果，不拆成互相独立的文档片段。分页片段引用原始块和文字范围；显式分页符/分节符才属于文档内容。纸型和边距归 `PageSection`，不归 `EnvironmentNode`。

修改段落时复制这一块并增加 `rev`。没改的块继续共享原来的 `Arc`。

## 位置

`Pos` 由 story、块路径和 UTF-8 字节偏移组成。候选规则是 story 标识文档根正文；块路径到 Paragraph 或 Table cell 内的 Paragraph 为止，偏移按该段落逻辑文本的 UTF-8 字节计数。Run 边界和行内 EnvironmentNode 嵌套不改变偏移单位；给定偏移所属的行内环境链从内容树派生。块级 EnvironmentNode 与表格 cell 通过块路径寻址。story 的稳定身份、路径段是否使用有类型的枚举、行内环境边界亲和性，以及块移动时的位置映射由 task 020 决定；这里仍是候选草案。

选区记住锚点和焦点。公式内部另有 `MathPos`：节点路径加节点内偏移。它不属于 `Path`，避免和表格下标混用。

## 上限

公式树深度 64。表格最多 32767 行、63 列、嵌套 24 层。超过就拒绝这次命令，文档保持原样。

## `.cdoc`

原生文件是 UTF-8 JSON。

```json
{
  "format": "caret",
  "version": 1,
  "media": [],
  "document": {}
}
```

- `format` 必须是 `caret`，否则拒绝。
- `version` 是正整数。只有不兼容变化才加一：删字段、改语义、增加旧程序无法跳过的枚举。新增可选字段不加版本。
- 版本更高：拒绝打开，提示更新程序，不猜测内容。
- 版本更低：在内存里逐级迁移，保存时写成当前版本。
- 同一版本里，可选字段必须有默认值。读取器保留未知可选字段，并在往返保存时原样保留；遇到未知必需能力时可以只读查看，但拒绝写入，避免旧程序破坏无法理解的内容。该保留规则覆盖信封和 `document` 内部。必需能力的具体声明格式由 020 定稿。
- 媒体放在 `media` 数组里，用 id 引用，内容是 base64。单个媒体超过 32 MiB 就拒绝插入。更大的外链不在第一版里。

保存先写同目录临时文件，fsync，再替换原文件。中途失败时原来的文件还在。畸形 JSON 返回错误，不 panic。

每个发布过的版本留一份夹具：读、写、再读必须相等；更高版本必须报错。
