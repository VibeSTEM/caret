# 文档模型

更新日期：2026-10-25。

> 产品已决定使用 Paragraph/Run 作为文本表示。020 继续细化 Caret 的语义块、位置、编号、删除和持久化契约；下列字段仍是草案，不等于照搬 OOXML，也不承诺 DOCX 兼容。

## 树

语义骨架草案：

```text
Document
├── globalSettings: DocumentSettings { theme, defaultFontFamily?: Family, defaultStyles, formatOverrides, ... }
├── styles: StyleSheet { generation: StyleGeneration }
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

M0 的 `pageSections` 恰有一个元素，覆盖整份 `Document.body`。首版不支持在文档内部切换纸型、方向或边距；多节范围与显式分节节点以后另行设计，不从数组顺序推断内容范围。

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
├── contentRev    # 文档语义值变化时递增
├── layoutRev     # 文字/度量变化时递增，作为布局缓存键
├── structureRev  # 文本范围/结构/大纲语义变化时递增
└── paintRev      # 仅绘制属性变化时递增，不进入布局缓存键
```

`children` 按顺序保存环境拥有的正文节点：块级环境存段落、嵌套块环境、表格等 `Block`；行内环境存 Run、嵌套行内环境等 `InlineNode`。块级环境的 `title` 单独保存，不是 `children` 的首项。删除整个块环境会删除标题和子树；提升子项只移动其正文 `children`。

文档有独立的全局设置层，用于选择文档排版主题、默认 Run/Paragraph 样式和整份文档的格式覆盖值。新建文档的 `defaultFontFamily` 未设置，默认字体命令才写入显式的文档级字体覆盖；未设置时先解析主题命名样式，再回退到系统默认文本字体。需要局部指定字体时通过 `Font` EnvironmentNode 表达，Run 不直接保存字体族。格式按属性逐项解析，优先级为 Run/Paragraph 局部显式属性（字体族除外）→ EnvironmentNode 设置 → 文档全局覆盖 → 主题中的命名样式 → 应用默认值；这里的局部显式属性是直接存于具体 Run/Paragraph 的值（如字号、字色、缩进、段后距），不是命名样式引用。粗体和斜体只由 EnvironmentNode 表示。Run/Paragraph 可引用可复用的命名样式。命名样式定义属于文档排版主题，不是应用界面主题（如 GUI 颜色和控件外观）。`.cdoc` 保存所选主题的当前格式快照，可记录外部来源 ID；文档不依赖主题源文件持续存在。主题只维护一个当前格式，不并行支持多版本运行时；加载旧主题或旧快照时立即迁移到当前结构。外部主题更新不会自动改变已有文档；显式更新时替换快照并按受影响范围推进 `StyleGeneration`，使相应格式与布局缓存失效。页面尺寸等分页设置仍属于 `PageSection`，环境标题和编号由环境类型管理，不混入格式全局设置。

块级环境的 `title` 始终是单个 Paragraph。标题中按 Enter 时，光标前文本留在标题，后半段移入新建的正文首段，并插到已有正文内容之前；后缀为空时该段为空。若正文首项是表格或嵌套环境，也在它之前插入 Paragraph。标题不会被拆成多个标题段。

已决定使用 Paragraph/Run 作为文本表示：Paragraph 是段落边界和段落级属性的载体，Run 是连续文字及字符级格式的叶子节点。首版内置环境类型如下；注册表是封闭集合，后续新类型需先更新模型规范与能力定义：

| kind | scope | 子内容 | 语义 |
| --- | --- | --- | --- |
| `Bold` | inline | `InlineNode` | 粗体作用域 |
| `Italic` | inline | `InlineNode` | 斜体作用域 |
| `Font` | inline | `InlineNode` | 指定字体族的局部作用域；内层字体覆盖外层 |
| `Section` | block | `Block` | 可选标题、章节结构和编号 |
| `Showcase` | block | `Block` | 可选标题、可嵌套的展示容器 |

每种环境的 `props` 只保存其注册语义允许的设置；不支持文件任意定义新 `kind`。

- `kind` 来自文档模型定义的内置类型注册表；文件不能自行声明任意环境类型。每种类型注册自己的 scope、合法子节点和语义。未知类型按未知必需能力处理，只读打开并禁用保存。注册表及 `requiredFeatures` 能力标识的唯一维护位置是本文档，由 020 维护。

- 粗体和斜体只能由行内 EnvironmentNode 表示，Run 不携带粗体或斜体直接覆盖；粗体和斜体可以互相嵌套，同类粗斜体环境不嵌套且相邻作用范围规范化合并。如粗体内嵌斜体，格式按父子作用域组合。Font 环境可以用不同字体族嵌套，内层覆盖外层；相同字体族的冗余嵌套压平，相邻同字体范围合并。其他语义环境不因 kind 相同就自动合并。拆分时原 ID 留在左侧，后续片段分配新 ID；合并时保留文档顺序靠前的 ID；撤销恢复原节点及 ID。
- `Section`、`Showcase` 是块级 EnvironmentNode，拥有块子项和可选独立标题；`Section` 另有编号规则，`Showcase` 是可嵌套的展示容器。行内环境没有标题。
- 当前环境背景高亮是由光标位置派生的视图叠加，不属于持久化模型；行为见[结构导航](structure-navigation.md)。
- 章节编号 `1.` 由编号定义生成，不烘焙进标题 Run，所以插入、移动或删除同级章节环境时可重新编号。
- `Table`、公式对象和显式分页仍是有类型的语义块/对象；自动分页只存在于布局结果。
- `PageSection` 只表示纸张、方向和边距等分页配置，与用户可见的 `EnvironmentNode` 分开，避免把页面分节和内容环境混为一谈。

块级环境可折叠、可选中和通过结构命令整体删除，默认删除该节点、独立标题及子树；若要保留子内容，提供明确的“解除环境/提升子项”操作。行内格式环境按文字范围编辑和删除。两者都不能让普通文本删除隐式破坏外层结构。命令事务、失败原子性和后续撤销由 018 衔接。环境种类、scope、编号定义、嵌套约束、标题的位置映射及块移动后的 ID/位置映射由 task 020 定稿。

普通标题仍可以只是带 `outline_level` 的 Paragraph；需要显式拥有/删除一组子内容时才用块级 EnvironmentNode。环境标题中的自动编号和标题样式属于可独立编辑的属性，不是靠解析标题文本推断。

字符样式候选值需要区分未指定、显式关闭和具体值，避免把“继承”压成 false。样式解析优先级及主题引用见[格式语义](text-formatting.md)。

页属于排版结果，不拆成互相独立的文档片段。分页片段引用原始块和文字范围；显式分页符/分节符才属于文档内容。纸型和边距归 `PageSection`，不归 `EnvironmentNode`。

修改段落时复制这一块并增加 `contentRev`。未改节点继续共享原来的 `Arc`；`layoutRev` 只在文字或度量变化时推进，`structureRev` 只在影响结构范围/导航时推进，`paintRev` 只在绘制属性变化时推进。布局键只消费 `layoutRev`；纯字色变化不会因通用内容修订而重排。样式、全局设置和主题变化按影响分类：度量变化使受影响布局失效，全局度量变化可触发完整分页；纯绘制属性只刷新绘制，来源等元数据不触发布局；只有影响大纲语义的样式变化才更新结构索引。相应的 `StyleGeneration` 分别跟踪布局度量、大纲语义和绘制属性；来源元数据变化不增加这些代际。所有更新保持局部，不复制整棵内容树。

## 位置

`Pos` 由 story、块路径和 UTF-8 字节偏移组成。story 标识文档根正文；块路径使用稳定节点 ID，到 Paragraph 或 Table cell 内的 Paragraph 为止，也能定位独立存放在块级 EnvironmentNode 上的标题 Paragraph。偏移按目标段落逻辑文本的 UTF-8 字节计数。节点移动保留其 ID 和目标位置；节点删除或替换时，由文档事务返回旧位置到新位置的映射。Run 边界和行内 EnvironmentNode 嵌套不改变偏移单位；给定偏移所属的行内环境链从内容树派生。块级 EnvironmentNode 与表格 cell 通过块路径寻址。Rust 内以 UTF-8 字节作为规范存储单位，键盘左右移动按 Unicode 字素簇，不允许落在字素簇内部；Qt/C++ UTF-16 索引只在 FFI 边界映射。story 的稳定身份、路径段的强类型表示、行内环境边界亲和性，以及块移动时的位置映射由 task 020 定稿。

选区记住锚点和焦点。公式内部另有 `MathPos`：节点路径加节点内偏移。它不属于 `Path`，避免和表格下标混用。

## 上限

公式树深度 64。表格最多 32767 行、63 列、嵌套 24 层。超过就拒绝这次命令，文档保持原样。

## `.cdoc`

原生文件是 UTF-8 JSON。

```json
{
  "format": "caret",
  "version": 1,
  "requiredFeatures": [],
  "media": [],
  "document": {}
}
```

- `format` 必须是 `caret`，否则拒绝。
- `version` 是正整数。只有不兼容变化才加一：删字段、改语义、增加旧程序无法跳过的枚举。新增可选字段不加版本。
- 版本更高：拒绝打开，提示更新程序，不猜测内容。
- 版本更低：在内存里逐级迁移，保存时写成当前版本。
- `requiredFeatures` 是可选的顶层字符串数组，缺省为空。当前程序不认识其中任一项时允许只读打开，但必须禁用保存，避免旧程序破坏依赖未知语义的内容；能力标识及其语义的唯一登记表维护在本文档，由 020 建立。
- 同一版本里，可选字段必须有默认值。读取器保留未知可选字段，并在往返保存时原样保留。该保留规则覆盖信封和 `document` 内部。
- 媒体放在 `media` 数组里，用 id 引用，内容是 base64。单个媒体超过 32 MiB 就拒绝插入。更大的外链不在第一版里。

保存先写同目录临时文件，fsync，再替换原文件。中途失败时原来的文件还在。畸形 JSON 返回错误，不 panic。

每个发布过的版本留一份夹具：读、写、再读必须相等；更高版本必须报错。
