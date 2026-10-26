# 文档模型

更新日期：2026-10-25。

> 产品已决定使用 Paragraph/Run 作为文本表示。020 定稿 Caret 的语义块、位置、编号、删除和持久化契约；实现任务可补实现细节，但不得另立冲突的产品语义。后续澄清只补充环境样式优先级、边界亲和性和相应测试，不改变模型，也不撤销单一 PageSection、原始 Unicode 保留、BCP 47 回退或标题 Enter 等既有规则。下列字段仍是设计草案，不等于照搬 OOXML，也不承诺 DOCX 兼容。

## 树

语义骨架草案：

```text
Document
├── globalSettings: DocumentSettings { theme, defaultLanguage?: Bcp47, defaultFontFamily?: Family, defaultCjkFontFamily?: Family, scriptFontFamilies?: Map<ISO15924, Family>, defaultMathFontFamily?: Family, defaultStyles, formatOverrides, ... }
│   └── theme snapshot: { formatVersion, named styles, composable environment templates }
├── styles: StyleSheet { generation: StyleGeneration }
├── body: Vec<Arc<Block>>
│   ├── EnvironmentNode { id, kind, scope, definitionId?, props, title?: Paragraph, children }
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
Run = { text, props, language?: Bcp47 }  # text 保留原始 Unicode 码点序列

EnvironmentNode(scope: block).children: Vec<Block>
EnvironmentNode(scope: block).title: Option<Paragraph>  # 与 children 分开
EnvironmentNode(scope: inline).children: Vec<InlineNode>

Paragraph
├── language?    # BCP 47 override for the document default
├── inlines       # Run、行内 EnvironmentNode 和行内对象组成的有序内容
├── markStyle     # 段落标记/空段落的有效字符格式
├── props         # 段落样式、缩进与间距
├── contentRev    # 文档语义值变化时递增
├── layoutRev     # 文字/度量变化时递增，作为布局缓存键
├── structureRev  # 文本范围/结构/大纲语义变化时递增
└── paintRev      # 仅绘制属性变化时递增，不进入布局缓存键
```

`children` 按顺序保存环境拥有的正文节点：块级环境存段落、嵌套块环境、表格等 `Block`；行内环境存 Run、嵌套行内环境等 `InlineNode`。块级环境的 `title` 单独保存，不是 `children` 的首项。整体删除块级环境会删除标题和子树。解除环境则保留所有可见内容：有标题时将其转成普通 Paragraph，放在原位置的提升子项之前；Section 的当前自动编号物化为标题段落中的普通文字。提升子项保留原顺序和节点 ID，标题段落保留自身 ID；环境节点及其编号定义被移除。无标题 Section 按编号规则不显示、不占号，解除时只提升子项，不合成标题段落。

文档有独立的全局设置层，用于选择文档排版主题、默认语言、默认 Run/Paragraph 样式和整份文档的格式覆盖值。字体选择契约见[多文字体系字体选择](font-selection.md)。语言使用 BCP 47 标签，按 Run → Paragraph → `globalSettings.defaultLanguage` → `und` 回退，用于断行词典等语言相关处理；它与应用界面语言相互独立。语言自动检测只能作为建议，不能静默修改正文或持久化标签。全局字体按 Unicode 文字体系选择，而不按英语、俄语等语言分别建字段：`defaultFontFamily` 是默认拉丁字体（英语及其他拉丁文字共用），`defaultCjkFontFamily` 用于 CJK 文字体系，`scriptFontFamilies` 可按 Unicode `Script` 短标签覆盖个别脚本（如 `Hani`、`Cyrl`、`Grek`、`Arab`），`languageScriptFontFamilies` 可在具体脚本内再按 BCP 47 标签覆盖，公式上下文单独使用 `defaultMathFontFamily`。未配置的文字体系回退到默认拉丁字体，再由字体引擎进行系统字体回退；缺字时必须检查实际 glyph fallback，不能假设单一字体覆盖全部字符。CJK 默认字体覆盖 Han（`Hani`）、Hiragana（`Hira`）、Katakana（`Kana`）、Hangul（`Hang`）和 Bopomofo（`Bopo`）；`Hani` 本身只表示 Han 脚本，不等于整个 CJK。语言标签（例如 `ja`、`ko`、`zh-Hant`）仍参与 shaping 和本地字形形式选择。公式、文字体系和局部 `Font` 环境选择的是不同上下文，局部环境内的显式字体优先于文档默认字体。Run 不直接保存字体族；新建文档的各默认字体均未设置。格式按属性逐项解析，优先级为 Run/Paragraph 局部显式属性（字体族除外）→ EnvironmentNode 有效设置 → 文档全局覆盖 → 主题中的命名样式 → 应用默认值。主题 kind 默认模板、环境命名变体及实例 `props` 一旦作为某个环境的样式来源，解析结果都属于 `EnvironmentNode` 有效设置层，不再作为较低的主题命名样式层参与回退；节点内部优先级为 kind 默认模板 → 所选变体覆盖默认模板 → 实例 `props` 覆盖变体，之后最近的内层环境值覆盖外层环境值。因此全局字号为 14 pt、Section 默认模板字号为 18 pt 且实例无覆盖时，Section 内最终字号为 18 pt；实例字号可再覆盖它。环境未提供某属性时才继续回退到全局覆盖、主题命名样式和应用默认值。Run/Paragraph 局部显式属性是直接存于具体 Run/Paragraph 的值（如字号、字色、缩进、段后距），不是命名样式引用。粗体和斜体只由 EnvironmentNode 表示。Run/Paragraph 可引用可复用的命名样式。命名样式定义属于文档排版主题，不是应用界面主题（如 GUI 颜色和控件外观）。`.cdoc` 保存所选主题的当前格式快照，可记录外部来源 ID；文档不依赖主题源文件持续存在。主题只维护一个当前格式，不并行支持多版本运行时；加载旧主题或旧快照时立即迁移到当前结构。外部主题更新不会自动改变已有文档；显式更新时替换快照并按受影响范围推进 `StyleGeneration`，使相应格式与布局缓存失效。页面尺寸等分页设置仍属于 `PageSection`，环境标题和编号由环境类型管理，不混入格式全局设置。

Run 文本在插入、打开和保存时保留原始 Unicode 码点序列，不隐式转成 NFC 或 NFD。搜索/比较如需规范化，使用独立的规范化视图，不回写或替换文档正文。

块级环境的 `title` 始终是单个 Paragraph。标题中按 Enter 时，光标前文本留在标题，后半段移入新建的正文首段，并插到已有正文内容之前；光标在标题开头或末尾时，允许标题前缀或新正文段为空。标题内的选区按“删除选区，再在选区起点拆分”一次提交。跨越标题与正文的选区不由标题 Enter 隐式改写；当前命令拒绝且保留文档、选区和 IME 状态。若正文首项是表格或嵌套环境，也在它之前插入 Paragraph。标题不会被拆成多个标题段。IME composition 尚未提交时 Return 由输入法消费，不触发 Enter 命令。

已决定使用 Paragraph/Run 作为文本表示：Paragraph 是段落边界和段落级属性的载体，Run 是连续文字及字符级格式的叶子节点。首版环境有内置的语义 kind 和可组合的环境定义两层。`kind` 决定节点 scope、合法子项和结构行为；主题快照为每种内置 kind 提供默认模板，并可保存用户创建或覆盖的命名变体。`definitionId` 选择与节点 `kind` 相符的模板；未指定时使用该 kind 的主题默认模板，节点 `props` 再作局部覆盖。模板可组合已有格式环境和样式值，自定义呈现并覆盖内置默认值，但不能改写结构 kind 的子项约束、编号关系或编辑语义；新结构语义仍需登记为新能力。文档保存语义环境实例，不把模板展开并复制成普通内容节点。首版模板采用受限声明式数据，可组合内置格式环境、命名样式和覆盖值，不执行用户脚本或通用宏语言。首版内置 kind 如下：

| kind | scope | 子内容 | 语义 |
| --- | --- | --- | --- |
| `Bold` | inline | `InlineNode` | 粗体作用域；可选 `enabled: true|false` 显式开启/关闭继承的粗体，缺省兼容为 true |
| `Italic` | inline | `InlineNode` | 斜体作用域；可选 `enabled: true|false` 显式开启/关闭继承的斜体，缺省兼容为 true |
| `Font` | inline | `InlineNode` | 指定字体族的局部作用域；内层字体覆盖外层 |
| `Section` | block | `Block` | 可选标题、章节结构和编号 |
| `Showcase` | block | `Block` | 可选标题、可嵌套的展示容器 |

每种环境的 `props` 只保存其注册语义允许的实例级覆盖；环境模板与命名样式存于所选主题快照。文件可以定义基于内置 kind 的环境模板，但不能任意发明新的结构 kind。模板引用的 `baseKind` 必须与实例 `kind` 一致。

- `kind` 来自文档模型定义的内置类型注册表；每种类型注册自己的 scope、合法子节点和语义。环境定义可以在主题中基于这些 kind 组合样式、覆盖呈现，但不能声明新的结构语义。遇到未知 kind 或必需能力时进入只读安全预览：保留未知数据为不透明节点，在原位置显示占位并列明不支持能力；只对可解释部分作尽力呈现，不宣称全文含义或排版正确。不能安全定位未知内容边界时拒绝预览。禁止保存和会丢弃未知内容的整篇复制/导出；局部复制仅在目标完全由已知节点组成时允许。注册表及 `requiredFeatures` 能力标识的唯一维护位置是本文档，由 020 维护。

首版 `requiredFeatures` 能力标识使用稳定的 `caret.` 命名空间；仅在文档实际依赖该能力时写入。首版登记表：

| ID | 能力 |
| --- | --- |
| `caret.env.bold` | 行内粗体环境 |
| `caret.env.explicit-format-state` | Bold/Italic 环境中显式的 `enabled` 状态，包括局部关闭及重新开启 |
| `caret.env.font` | 行内字体族环境 |
| `caret.env.italic` | 行内斜体环境 |
| `caret.env.section` | 块级章节、层级计数和生成标签 |
| `caret.env.showcase` | 块级可嵌套展示容器 |
| `caret.env.templates` | 主题中的声明式环境模板、同 kind 变体和实例选择 |
| `caret.text.language-tags` | BCP 47 文档/段落/Run 语言标签 |
| `caret.theme.snapshot` | `.cdoc` 内嵌文档排版主题快照 |

`requiredFeatures` 只列该文档实际用到的非基础能力，去重并按字节序排序。读取器发现未知能力时只读打开；能力 ID 的语义在发布后不可变，语义不兼容时使用新 ID 或提升 `.cdoc` 主版本。

- 粗体和斜体只能由行内 EnvironmentNode 表示，Run 不携带粗体或斜体直接覆盖；缺少对应环境表示继承，`enabled: true|false` 显式开启/关闭，缺省 `enabled` 兼容为 true。若文档序列化了显式 `enabled` 值，需在 `requiredFeatures` 声明 `caret.env.explicit-format-state`。粗体与斜体可以互相嵌套；同 kind、同状态、同 `definitionId`、同 `props` 且上下文相同的嵌套为冗余，可压平；不同状态的同 kind 环境有效且可嵌套，如 `on → off → on`。相邻范围仅在这些语义字段兼容且不跨其他语义节点时合并。如粗体内嵌斜体，格式按父子作用域组合。Font 环境可以用不同字体族嵌套，内层覆盖外层；相同字体族且其余语义字段兼容的冗余嵌套压平，相邻同字体范围合并。其他语义环境不因 kind 相同就自动合并。拆分时原 ID 留在左侧，后续片段分配新 ID；合并时保留文档顺序靠前的 ID；撤销恢复原节点及 ID。
- `Section`、`Showcase` 是块级 EnvironmentNode，拥有块子项和可选独立标题；`Section` 另有编号规则，`Showcase` 是可嵌套的展示容器。行内环境没有标题。
- 当前环境背景高亮是由光标位置派生的视图叠加，不属于持久化模型；行为见[结构导航](structure-navigation.md)。
- 章节编号由 `Section` 持有的编号定义生成，并作为标题 Paragraph 前的虚拟标签绘制。启用编号的 Section 构成编号层级与计数流：顶层共用文档级计数流；子级按最近的已编号 Section 祖先和显示层级计数。关闭编号的 Section 和 `Showcase` 对标签路径及计数流都透明，其已编号后代继续使用最近已编号祖先的计数流，因此不同未编号容器下同层后代按文档顺序接续序号。例如 Section `1` 下有未编号容器 A、B，A 内首个编号 Section 为 `1.1`，B 内首个编号 Section 为 `1.2`。关闭编号的节点本身不占序号。模板为编号提供默认显示开关与逐层格式，实例可覆盖；格式至少支持阿拉伯数字、小写/大写罗马数字和中文数字。模板可定制标签显示格式，但不能改变层级和计数关系。`title: None` 的 Section 不显示标签且不占自身序号，作为结构容器对后代编号透明；空标题 Paragraph 则可正常显示标签并占号。编号在视觉上类似行内文本，但不持久化为 Run、不进入标题文本或 UTF-8 偏移，因此插入、移动或删除同一计数流中的 Section 后可重新编号。解除 Section 时，先为整个操作范围快照当前显示标签，再原子提升子树并物化标签，结果不依赖遍历顺序。
- `Table`、公式对象和显式分页仍是有类型的语义块/对象；自动分页只存在于布局结果。
- `PageSection` 只表示纸张、方向和边距等分页配置，与用户可见的 `EnvironmentNode` 分开，避免把页面分节和内容环境混为一谈。

块级环境可折叠、可选中和通过结构命令整体删除，默认删除该节点、独立标题及子树；若要保留内容，提供明确的“解除环境”操作，按上述规则把标题和正文移回父级。行内格式环境按文字范围编辑和删除。两者都不能让普通文本删除隐式破坏外层结构。命令事务、失败原子性和后续撤销由 018 衔接。环境种类、scope、编号定义、嵌套约束、标题的位置映射及块移动后的 ID/位置映射按本文规则维护，实现任务补充具体 API 和测试。

普通标题仍可以只是带 `outline_level` 的 Paragraph；需要显式拥有/删除一组子内容时才用块级 EnvironmentNode。环境标题中的自动编号和标题样式属于可独立编辑的属性，不是靠解析标题文本推断。

字符样式候选值需要区分未指定、显式关闭和具体值，避免把“继承”压成 false。样式解析优先级及主题引用见[格式语义](text-formatting.md)。

页属于排版结果，不拆成互相独立的文档片段。分页片段引用原始块和文字范围；显式分页符/分节符才属于文档内容。纸型和边距归 `PageSection`，不归 `EnvironmentNode`。

修改段落时复制这一块并分配新的 `contentRev`。未改节点继续共享原来的 `Arc`；共享节点不保证窄范围编辑成本与文本长度无关，宽子项 `Vec` 或超长 Paragraph 仍可能复制较大容器，数据结构须由 013 基准决定，不在没有测量前承诺 rope 或分块表示。`contentRev`、`layoutRev`、`structureRev` 和 `paintRev` 是不可复用的修订令牌：每次新编辑分配新的会话内唯一代际；Undo 可恢复旧节点及其旧令牌，之后的新编辑不能复用已分配令牌。缓存同时包含文档身份，异步布局快照携带单调 `documentGeneration`，过期结果不能覆盖新状态。`layoutRev` 只在文字或度量变化时推进，`structureRev` 只在影响结构范围/导航时推进，`paintRev` 只在绘制属性变化时推进。布局键只消费 `layoutRev`；纯字色变化不会因通用内容修订而重排。样式、全局设置和主题变化按影响分类：度量变化使受影响布局失效，全局度量变化可触发完整分页；纯绘制属性只刷新绘制，来源等元数据不触发布局；只有影响大纲语义的样式变化才更新结构索引。相应的 `StyleGeneration` 分别跟踪布局度量、大纲语义和绘制属性；来源元数据变化不增加这些代际。所有更新保持局部，不复制整棵内容树。

## 位置

`Pos` 的身份由 story ID、终止节点稳定 ID、UTF-8 字节偏移和逻辑边界亲和性组成；有类型祖先路径（`EnvironmentBody(environmentId)`、`TableCell(tableId, cellId)`、`Paragraph(paragraphId)` 或 `EnvironmentTitle(environmentId, paragraphId)`）可作为可重建的定位与校验投影，不使用数组下标。节点 ID 在文档内唯一，索引可从 ID 重建当前路径；解析器验证路径段类型和父子关系。插入、删除、替换、移动、拆分、合并和解除环境都通过事务返回位置映射；跨父移动保留节点 ID，但旧祖先路径不再作为目标路径。偏移按目标段落逻辑文本的 UTF-8 字节计数。Run 边界和行内 EnvironmentNode 嵌套不改变偏移单位；给定偏移所属的行内环境链从内容树派生。Rust 内以 UTF-8 字节作为规范存储单位，键盘左右移动按同一 Unicode 版本的 extended grapheme cluster 移动，不允许落在字素簇内部；Qt/C++ UTF-16 索引只在 FFI 边界映射。

当同一逻辑段落偏移恰好位于行内环境边界时，`Affinity::Upstream` 表示光标关联逻辑文档顺序的前侧，`Affinity::Downstream` 表示关联后侧；它决定输入继承的环境链，不代表屏幕左侧或右侧。例如普通文本 A、环境内容 B、普通文本 C 的状态应为“无环境｜有环境｜无环境”：环境起点的 upstream 在 A、downstream 在 B；环境终点的 upstream 在 B、downstream 在 C。光标仍关联环境内侧时，继续输入沿用该环境；环境末端移动到逻辑外侧后，下次输入不再继承该环境。一个视觉光标停靠点可以携带不同逻辑亲和性，但亲和性本身通常不是额外停靠点。对于 LTR 的单一行内环境，若方向键沿当前环境向外离开，在同一 UTF-8 偏移切换为外侧 affinity，并立即更新当前环境高亮/格式状态；这是有明确视图反馈的边界状态转换，不移动正文偏移，也不构成无反馈的空按键。继续反向移入环境时，按相邻字素簇移动文本偏移，并将到达位置设为环境内侧 affinity，不在同一偏移单独恢复 affinity。具体转移见下表，其中 `p0/p1/p2/p3` 分别是 A 前、环境起点、环境终点和 C 后的位置：

| 当前位置 | 按键 | 下一位置 | 效果 |
|---|---|---|---|
| `p1 / Upstream`（环境外） | Right | 环境首个字素簇之后的边界，`Upstream`（环境内） | 向前跨过首个字素簇；若环境只有一个字素簇，该位置就是 `p2 / Upstream` |
| `p2 / Upstream`（环境内） | Right | `p2 / Downstream`（环境外） | 偏移不变；退出环境并更新高亮/格式状态 |
| `p2 / Downstream`（环境外） | Left | 环境末个字素簇之前的边界，`Downstream`（环境内） | 向后跨过末个字素簇；若环境只有一个字素簇，该位置就是 `p1 / Downstream` |
| `p1 / Downstream`（环境内） | Left | `p1 / Upstream`（环境外） | 偏移不变；退出环境并更新高亮/格式状态 |

其他简单 LTR 移动按相邻字素边界进行。鼠标命中按布局给出的相邻字素簇边缘选择亲和性；若点击精确落在无法区分两侧的边界，确定性地选择逻辑 `Downstream`。工具栏格式命令保留原光标亲和性；选区命令保留锚点和焦点的完整 Pos，并在事务改写后通过位置映射恢复两侧。对于 bidi 或换行处视觉位置不同的同逻辑 Pos，方向键按视觉布局直接移动到相邻视觉 caret 候选，并以目标候选的字素边缘确定逻辑 affinity；不先执行上表的同位置状态转换。混合方向文本不能按屏幕左/右推断逻辑 upstream/downstream。鼠标命中测试和结构命令也必须明确提交逻辑侧。选区锚点和焦点各自携带完整 `Pos`，因此选区方向及两端的边界语义都能保留。

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
- 每个主题快照另有主题格式版本；仅对已知旧版本且有完整迁移器的快照执行迁移。未知未来主题版本或缺失迁移器进入只读安全预览，不猜测解释。迁移在临时副本上完成并验证所有引用，成功后才替换当前内存快照；失败保留原文档和快照。
- `requiredFeatures` 是可选的顶层字符串数组，缺省为空。当前程序不认识其中任一项时不得宣称文档已被正确打开：若能可靠定位未知数据，则进入标注清晰的只读安全预览，未知内容原样保留并占位；若未知能力可能改变解析边界且无法可靠定位，则拒绝预览。安全预览禁用保存和会遗漏不透明内容的复制/导出。能力标识及其语义的唯一登记表维护在本文档，由 020 建立。
- 同一版本里，可选字段必须有默认值。读取器保留未知可选字段，并在往返保存时原样保留。该保留规则覆盖信封和 `document` 内部。
- 媒体放在 `media` 数组里，用 id 引用，内容是 base64。单个媒体超过 32 MiB 就拒绝插入。更大的外链不在第一版里。

保存先写同目录临时文件，fsync，再替换原文件。中途失败时原来的文件还在。畸形 JSON 返回错误，不 panic。

每个发布过的版本留一份夹具：读、写、再读必须相等；更高版本必须报错。
