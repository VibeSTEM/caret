# 文档模型

更新日期：2026-10-29。

> 产品已决定使用 Paragraph/Run 作为文本表示。本文是文档持久化语义、页面层级和结构类别的权威来源；实现任务可补实现细节，但不得另立冲突的产品语义。本文的语义草图不等于当前 Rust DTO/JSON 的逐字段描述，不等于照搬 OOXML，也不承诺 DOCX 兼容。

## 当前实现

当前 `caret-doc` 的实际树形 DTO 是 `Document { body: Vec<Block>, page_sections, global_settings }`；`Block` 当前包含 Paragraph、BlockEnvironment、ExplicitBreak 和 Opaque，`InlineNode` 当前包含 Run、InlineEnvironment、SoftBreak 和 Opaque。块环境与行内环境是不同的 Rust 类型，均没有统一的带 `scope` 字段的 `EnvironmentNode` DTO。Heading 由 Paragraph 的属性识别。Equation、Align、Table、Figure 等是语义模型的后续节点，不代表它们已在当前 `caret-doc` 中实现。

本页下文中的 `EnvironmentNode` 是语义层的统称；树形图是目标模型草图。当前 `.cdoc` 的具体字段以 `crates/caret-doc/src/model/` 中的 DTO 和 codec 为准。文档修订、布局缓存代际、焦点和选区属于运行时状态，不序列化进 `Document`。

相关设计各维护一份权威定义：本文维护持久化树、节点语义、页面角色和通用结构类别；[结构导航](structure-navigation.md)维护索引、导航及几何投影；[多文字体系字体选择](font-selection.md)维护字体候选与 fallback；[文字格式语义](text-formatting.md)维护格式解析和格式命令行为。任务文档只记录实现进度与验收，不重复这些契约。

## 文档树与内容模型

### Block、Inline 与行内内容插入

这三个概念分属不同层次，不能把“行内插入”误认为第三种节点容器：

| 概念 | 含义 | 文档模型 / 操作 |
| --- | --- | --- |
| `Block` | 正文流中的块级内容，参与块顺序、分页和结构导航 | `Paragraph`、块级 `EnvironmentNode`、显式分页；块环境拥有有序 `children`，可嵌套 |
| `Inline` | 一个 `Paragraph` 内按顺序排布的内容，参与同一段逻辑文本、塑形和字素偏移 | `Run`、行内 `EnvironmentNode`、`SoftBreak`、未知行内节点；行内格式环境保存嵌套子节点 |
| 行内内容插入 | 编辑命令在光标或选区处插入/替换行内节点的行为，不是节点类型 | `\\newline` 与 Shift+Enter 都插入 `SoftBreak`，段落 ID 不变；Enter 调用段落拆分，创建新的 `Paragraph` / 块序列位置 |

`Run.props` 和 `Paragraph.props` / `Paragraph.layout` 保存直接属性；Run 与 Paragraph 是属性的承载对象，不是格式环境。粗体、斜体和局部字体使用 inline `EnvironmentNode`。因此格式命令分成两种文档变更：环境格式命令在 Inline 子树上包裹、拆分、合并或解除节点；直接格式命令只更新目标 Run/Paragraph 的属性。两者由同一个 Rust 编辑会话提交原子事务、位置映射和 Undo 记录，QML 不自行修改格式树或决定属性继承。

Rust 文件按数据职责和编辑职责对应：

```text
crates/caret-doc/src/model.rs           文档根 DTO 与公开导出
crates/caret-doc/src/model/block.rs     Block、块环境、显式分页
crates/caret-doc/src/model/paragraph.rs Paragraph 与段落直接布局覆盖
crates/caret-doc/src/model/run.rs       Run 与 Run 直接属性
crates/caret-doc/src/model/inline.rs    InlineNode、行内环境和软换行
crates/caret-engine/src/editing/        命令会话、事务和文档编辑
crates/caret-structure/src/atoms/       段落/标题与块环境等结构原子
crates/caret-structure/src/index.rs     唯一结构索引装配入口
```

## 页面层级与内容块形态

页面组合高于内容块形态。概念上文档按顺序包含同一种 `PageContent`，由 `role` 区分 `BodyPage` 与 `TitlePage`；两者共用 `Vec<Block>` 子内容。`BodyPage` 是连续正文流，`TitlePage` 从新页开始且可继续流到后续物理页。分页器产生的物理页面/片段不是逐页持久化节点。`PageSection` 只保存页面尺寸、方向、边距等版式设置。

页面组合内部使用以下通用内容块形态。它们描述结构，不代表当前视觉行数，也不替代领域节点 kind：

| 通用内容块形态 | 领域结构映射 | 说明 |
| --- | --- | --- |
| `Paragraph` | 普通段落 | 正文段落，可按宽度折行 |
| `SingleLine` | Heading；后续 `Title`、Equation | 一个语义块，不承诺视觉排版只占一行；`Title` 只表达标题，Equation 按单行公式语义处理 |
| `MultiLine` | 块环境；后续 Align | 一个容纳多个子项或显式多行内容的语义块 |
| `Table` | 后续 Table | 独立的非文本结构 |
| `Figure` | 后续 Figure | 独立的非文本结构 |

页面角色与内容块形态是正交层级。页面角色组织页级内容；Paragraph 承载行内内容并决定段落折行，`SingleLine` 只是语义分类，不保证窄页面上只有一条视觉行。

Heading、Section、Custom 等具体领域语义由结构索引保留，快照提供稳定语义 kind，界面按当前 UI 语言翻译；通用类别仅用于布局/导航分类，不替代状态栏的语义标签。`Title` 后续映射为 `SingleLine`；`TitlePage` 是更高层的 `PageContent` 页面角色；Equation 映射为 `SingleLine`，Align 映射为 `MultiLine`；Table/Figure 在对应节点接入索引后映射。`InlineMath` 留在 Paragraph 内，不单独进入块结构栈。索引如何维护这些语义见[结构导航](structure-navigation.md)。

页面组合的语义草案（不是当前 DTO 字段或 `.cdoc` JSON schema）：

```text
Document
├── 页面组合（有序语义序列）
│   └── PageContent { role: BodyPage | TitlePage, children: Vec<Block> }
│       ├── BodyPage：连续正文角色，可跨多个物理页
│       └── TitlePage：从独立页开始的标题页角色，内容可延续到后续页
│           ├── Title { paragraph: Paragraph }           # 可选的单行标题内容块
│           ├── Author                                    # 可选内容；数量/元数据绑定待定
│           ├── Date                                      # 可选内容；数量/元数据绑定待定
│           └── 其他允许的通用内容块
├── globalSettings: DocumentSettings { theme, language, fonts, defaultStyles, formatOverrides, ... }
└── pageSections: Vec<PageSection>                      # 页面尺寸、方向和边距等版式设置
```

当前持久化 DTO 仍是 `Document { global_settings, body: Vec<Block>, page_sections }`；当前没有持久化的 `PageContent` / `BodyPage` / `TitlePage` 节点。是否为未来页面组合新增 DTO 字段或节点，需在实现任务里评估兼容和编辑成本，不由概念图预设。当前 M0 的 `pageSections` 恰有一个元素，覆盖整份 `Document.body`；首版不支持在文档内部切换纸型、方向或边距，多节范围与显式分节节点以后另行设计。

`PageContent` 是统一的页面组合类型；`BodyPage` 与 `TitlePage` 是不同 `role`，不是两套节点类型。`Title` 是可放在任一页面角色中的 `SingleLine` 内容块。标题页可以自然分页，不为塞进单个物理页而强行缩放或裁切。Author/Date 是否允许重复、是否绑定文档元数据，尚未决定。

```text
Block = Paragraph | EnvironmentNode(scope: block) | Title | Equation | Align | Table | ExplicitBreak
InlineNode = Run | EnvironmentNode(scope: inline) | InlineMath | InlineObject | SoftBreak
Run = { text, props, language?: Bcp47 }  # text 保留原始 Unicode 码点序列

EnvironmentNode(scope: block).children: Vec<Block>
EnvironmentNode(scope: block).title: Option<Paragraph>  # 与 children 分开
EnvironmentNode(scope: inline).children: Vec<InlineNode>

Paragraph
├── language?    # BCP 47 override for the document default
├── inlines       # Run、行内 EnvironmentNode 和行内对象组成的有序内容
├── markStyle     # 段落标记/空段落的有效字符格式
├── props         # 段落样式、缩进与间距
└── extra         # 未知可选字段保留；运行时修订代际不属于 Paragraph DTO
```

`children` 按顺序保存环境拥有的正文节点：块级环境存段落、嵌套块环境、表格等 `Block`；行内环境存 Run、嵌套行内环境等 `InlineNode`。块级环境的 `title` 单独保存，不是 `children` 的首项。整体删除块级环境会删除标题和子树。解除环境则保留所有可见内容：有标题时将其转成普通 Paragraph，放在原位置的提升子项之前；Section 的当前自动编号物化为标题段落中的普通文字。提升子项保留原顺序和节点 ID，标题段落保留自身 ID；环境节点及其编号定义被移除。无标题 Section 按编号规则不显示、不占号，解除时只提升子项，不合成标题段落。

文档设置包括主题快照、语言、默认 Run/Paragraph 样式和全局格式覆盖。语言使用 BCP 47 标签，按 Run → Paragraph → `globalSettings.defaultLanguage` → `und` 回退；它与 UI 语言相互独立，自动检测不能静默改写正文或标签。字段、脚本/语言映射、字体回退和字体缓存规则以[多文字体系字体选择](font-selection.md)为准。格式属性的解析层级、环境模板覆盖和直接属性语义以[文字格式语义](text-formatting.md)为准。`PageSection` 承载页面尺寸等分页设置，不与文档主题或环境属性混用。

Run 文本在插入、打开和保存时保留原始 Unicode 码点序列，不隐式转成 NFC 或 NFD。搜索/比较如需规范化，使用独立的规范化视图，不回写或替换文档正文。相邻 Run 的 `language`、`props`、未知可选字段 `extra` 完全相同时，编辑器可在受影响的局部范围合并它们；这些边界不代表光标停靠点。没有文本且无显式属性/语言/未知字段的默认 Run 是无语义占位，可在局部编辑归一化时移除；不能合并格式不同或带未知字段的 Run，也不在打开文档时重写整棵内容树。

块级环境的 `title` 始终是单个 Paragraph。标题中按 Enter 时，光标前文本留在标题，后半段移入新建的正文首段，并插到已有正文内容之前；光标在标题开头或末尾时，允许标题前缀或新正文段为空。标题内的文本选区按“删除选区，再在选区起点拆分”一次提交。普通文本 Selection 只接受同一 `Story` 内的范围；GUI 指针拖选若跨越环境标题/正文或环境内外边界，会在 Rust 校验两个合法位置后提升为整块结构选区，并把焦点放到拖动方向对应的环境边界。无效 UTF-8/字素位置不得改变现有选区。标题 Enter 不会隐式改写跨 Story 文本范围。若正文首项是表格或嵌套环境，也在它之前插入 Paragraph。标题不会被拆成多个标题段。IME composition 尚未提交时 Return 由输入法消费，不触发 Enter 命令。

Section 首个正文 Paragraph 的段首 Backspace 删除标题与正文之间的段落边界：把正文首段的行内内容追加到 Section 标题，移除该正文段，并将插入点放到标题原内容之后；Section 本身保留。若标题与正文段落的段落级属性不兼容，操作原子失败，不静默丢弃属性。该操作必须作为单个撤销单元，并通过位置映射把焦点从正文 `Body` story 转到所属 `EnvironmentTitle` story。

块环境首尾分别提供环境内、外的键盘停靠位置。方向键到达外侧停靠点时不立即改写文档；若外侧已有相邻段落，再按一次同方向进入该段落；若没有相邻段落，则继续保留外侧停靠状态，直到用户输入时才创建外部 Paragraph。此时 Enter 创建一个外部空段落，Shift+Enter 创建一个含软换行的外部段落；之后输入留在该段落。空段落执行 `\section` 时转换为 Section 标题；非空段落执行时保留原段，并在其后创建一个空 Section。普通 Backspace 在折叠光标位于空环境起点，或从环境末尾外侧停靠点向回删除时可移除空环境；在空 Section 后继外部段落的段首按 Backspace 时，将该段内容合并到 Section 标题末尾并移除外部段落。非空环境不会因普通 Backspace 被删除。若用户通过 Shift+方向键或指针拖选选中整个环境，Backspace 删除该环境及标题、全部后代节点，并作为一个撤销单元；删除后光标落到前一段末尾，若没有前段则落到后一段开头，若文档没有其他段落则留下一个空 Paragraph 供继续编辑。段落内部 Backspace 仍按字素删除，不能因结构选区状态或环境导航状态被吞掉。第一次 Escape 收起整个环境选区，第二次才离开当前结构。撤销或重做后清除不再对应当前文档的虚拟环境外侧停靠状态。Mod+Backspace 解除当前环境外壳并保留标题和正文，按一次结构编辑撤销；内部按键意图名为 `mod-backspace`，领域命令 ID 为 `struct.remove_preserving_content`。

环境区分内置语义 kind 和可组合定义。内置 kind 固定 scope、合法子项和结构行为；Custom 是可跨 scope 选择的定义族，具体实例的 scope 必须与其 `definitionId` 指向的定义一致。主题快照提供内置 kind 默认模板、命名变体和自定义环境定义，实例 `props` 保存局部覆盖。`definitionId` 缺省表示使用 Custom 的通用默认模板；存在时必须能在当前文档嵌入的快照中解析到同 scope 定义。模板仅用声明式数据组合格式环境、命名样式和属性值，不能改写结构语义，也不执行脚本或通用宏。首版内置 kind 如下：

自定义环境定义应允许用户选择三种 scope：`Page`、`Block`、`Inline`，并设置可编辑的显示名。定义使用稳定 `definitionId`；改名不改变引用它的实例身份。实例可通过局部 `displayName` 覆盖定义名称。定义一旦被文档实例引用，其 scope 固定；若要换 scope，必须创建新的 `definitionId` 或执行显式文档结构转换，不能静默改定义使现存节点失效。三种 scope 共用声明式定义模型，但映射到各自合法的内容容器，不在 QML 为每个用户定义新增组件分支。scope 顺序固定为 `Page → Block → Inline`：Page 是 PageContent 的角色定义，只能位于文档页面序列，不能嵌套 Page；Block 实例位于块内容并可按定义递归嵌套；Inline 实例必须位于 Paragraph 的行内序列。较窄 scope 不能包含较宽 scope，因此 Block 内不能放 Page，Inline 内不能放 Block/Page。定义和文档实例均由 Rust 校验这些约束。显式更新文档主题时，必须先对候选快照验证所有已引用的 `definitionId` 和 scope；若引用将丢失或变更，拒绝本次更新并保留旧快照，直到用户显式转换相关实例。不得静默回退到 Custom 通用样式或丢失定义。

概念定义与实例关系如下；实际持久化字段以各 scope 的 Rust DTO 为准：

```text
ThemeSnapshot.customEnvironmentDefinitions[definitionId]
└── CustomEnvironmentDefinition { displayName, scope, composition }

Document
└── PageContent {
    ├── role: BodyPage | TitlePage
    ├── definitionId?: PageDefinitionId         # 可选 Page-scope 自定义定义
    └── children: Vec<Block>
        └── Custom(Block, definitionId)
            └── children: Vec<Block>
                └── Paragraph
                    └── inlines: Vec<InlineNode>
                        └── Custom(Inline, definitionId)
                            └── children: Vec<InlineNode>
}
```

Page scope 定义基于 `BodyPage` 或 `TitlePage` 内置角色的 PageContent 模板，不作为可递归嵌套的 Block，也不创造新的页面语义。模板通过独立 `definitionId` 引用，可组合外观与页级约束；Block/Inline 实例只能出现在对应的块序列和 Paragraph 行内序列中。主题文件更新不会自动改变已有文档；显式更新快照时，上述引用校验与原子替换规则同样适用。

当前实现只落地了 Block-scope `Custom` 节点和实例名校验；Page/Inline-scope 自定义实例、定义管理与跨 scope 嵌套校验属于后续实现，不能依据此概念图宣称已可编辑或可序列化。

| kind | scope | 子内容 | 语义 |
| --- | --- | --- | --- |
| `Bold` | inline | `InlineNode` | 粗体作用域；可选 `enabled: true|false` 显式开启/关闭继承的粗体，缺省兼容为 true |
| `Italic` | inline | `InlineNode` | 斜体作用域；可选 `enabled: true|false` 显式开启/关闭继承的斜体，缺省兼容为 true |
| `Font` | inline | `InlineNode` | 指定字体族的局部作用域；内层字体覆盖外层 |
| `Section` | block | `Block` | 可选标题、章节结构和编号 |
| `Custom` | Page / Block / Inline（由定义选择；当前实现仅 Block） | `PageContent` / `Block` / `InlineNode` | 可命名的声明式自定义环境定义；scope 决定合法内容容器 |

每种环境的 `props` 只保存其注册语义允许的实例级覆盖；环境模板与命名样式存于所选主题快照。文件可以定义基于内置 kind 的环境模板，但不能任意发明新的结构 kind。模板引用的 `baseKind` 必须与实例 `kind` 一致。

- `kind` 来自内置注册表；模板不能声明新结构语义。未知 kind 或必需能力若可可靠定位，则保留为不透明节点并进入标注清晰的只读安全预览；否则拒绝预览。未知 kind 不会自动迁移成另一个内置类型。安全预览不能宣称全文语义或排版正确，并禁用保存及会丢失未知内容的整篇复制/导出。注册表和 `requiredFeatures` 能力登记由本文档维护，020 负责其产品契约。

首版 `requiredFeatures` 能力标识使用稳定的 `caret.` 命名空间；仅在文档实际依赖该能力时写入。首版登记表：

| ID | 能力 |
| --- | --- |
| `caret.env.bold` | 行内粗体环境 |
| `caret.env.explicit-format-state` | Bold/Italic 环境中显式的 `enabled` 状态，包括局部关闭及重新开启 |
| `caret.env.font` | 行内字体族环境 |
| `caret.env.italic` | 行内斜体环境 |
| `caret.env.section` | 块级章节、层级计数和生成标签 |
| `caret.env.custom` | 当前已实现的块级可嵌套、可命名自定义容器 |
| `caret.env.templates` | 主题中的声明式环境模板、同 kind 变体和实例选择 |
| `caret.text.language-tags` | BCP 47 文档/段落/Run 语言标签 |
| `caret.theme.snapshot` | `.cdoc` 内嵌文档排版主题快照 |

`requiredFeatures` 只列该文档实际用到的非基础能力，去重并按字节序排序。读取器发现未知能力时只读打开；能力 ID 的语义在发布后不可变，语义不兼容时使用新 ID 或提升 `.cdoc` 主版本。

Page/Inline 自定义环境尚未实现；接入时应新增 scope 专属的必需能力标识，不能静默扩张当前 `caret.env.custom` 的块级语义。

- 行内格式环境仅在 kind、状态、模板、属性和上下文兼容时压平或合并；拆分时原节点 ID 留在文档顺序靠前的片段，其余片段分配新 ID；合并时保留顺序靠前的 ID，撤销恢复原树与 ID。具体 Bold/Italic/Font 操作见[文字格式语义](text-formatting.md)。
- `Section`、`Custom` 是块级 EnvironmentNode，拥有块子项和可选独立标题；`Section` 另有编号规则。`Custom` 是不绑定特定学术/排版语义的可嵌套容器，可通过 `props.displayName` 设置实例显示名，并由 `definitionId` 选择主题模板；不设置时使用通用名称。行内环境没有标题。
- 一个 Section 的语义层级示例：

```text
Section(id, props, definitionId?)
├── heading: Paragraph                 # 独立标题段；模板将其作为 Heading 子结构排版
│   └── inlines: [Run, InlineEnvironment, ...]
└── children: Vec<Block>               # 有序正文块
    ├── Paragraph
    ├── Section                         # 子 Section，形成层级编号
    │   ├── heading: Paragraph
    │   └── children: [Paragraph, ...]
    └── Custom                          # 通用、可命名的自定义容器
        └── children: [Paragraph, ...]
```

这里 `heading` 是 Section 标题的语义角色，不是一个额外持久化 `EnvironmentNode`；当前 DTO 用 `title: Option<Paragraph>` 保存它。编号是 Section 的派生标签，绘制在 heading 段落之前，不作为 Run 或标题文字存储。Heading 的默认视觉样式由 Section 模板提供；单个标题的局部覆盖由其 Paragraph 表达。普通 Paragraph 可带局部 `outlineLevel`，作为不拥有子树的独立大纲标题。
- 当前环境背景高亮是由光标位置派生的视图叠加，不属于持久化模型；行为见[结构导航](structure-navigation.md)。
- 章节编号由 `Section` 持有的编号定义生成，并作为标题 Paragraph 前的虚拟标签绘制。启用编号的 Section 构成编号层级与计数流：顶层共用文档级计数流；子级按最近的已编号 Section 祖先和显示层级计数。关闭编号的 Section 和 `Custom` 对标签路径及计数流都透明，其已编号后代继续使用最近已编号祖先的计数流，因此不同未编号容器下同层后代按文档顺序接续序号。例如 Section `1` 下有未编号容器 A、B，A 内首个编号 Section 为 `1.1`，B 内首个编号 Section 为 `1.2`。关闭编号的节点本身不占序号。模板为编号提供默认显示开关与逐层格式，实例可覆盖；格式至少支持阿拉伯数字、小写/大写罗马数字和中文数字。模板可定制标签显示格式，但不能改变层级和计数关系。`title: None` 的 Section 不显示标签且不占自身序号，作为结构容器对后代编号透明；空标题 Paragraph 则可正常显示标签并占号。编号在视觉上类似行内文本，但不持久化为 Run、不进入标题文本或 UTF-8 偏移，因此插入、移动或删除同一计数流中的 Section 后可重新编号。解除 Section 时，先为整个操作范围快照当前显示标签，再原子提升子树并物化标签，结果不依赖遍历顺序。
- `InlineMath` 是段落内的原子对象；`Equation` 与 `Align` 是块级公式节点，分别表示单行显示公式和多行对齐公式。公式内部位置使用 `MathPos`。自动分页只存在于布局结果。
- `Title` 属于 `SingleLine` 内容块；`TitlePage` 是 `PageContent` 的一种 role，不是物理页或 `PageSection`。TitlePage 与 BodyPage 共用块模型，页级约束由分页布局兑现。模型接入前不得假设这些后续语义已经可编辑或可导航。
- `PageSection` 只表示纸张、方向和边距等分页配置，与用户可见的 `EnvironmentNode` 分开，避免把页面分节和内容环境混为一谈。

块级环境可折叠、可选中和通过结构命令整体删除，默认删除该节点、独立标题及子树；若要保留内容，提供明确的“解除环境”操作，按上述规则把标题和正文移回父级。行内格式环境按文字范围编辑和删除。两者都不能让普通文本删除隐式破坏外层结构。命令事务、失败原子性和后续撤销由 018 衔接。环境种类、scope、编号定义、嵌套约束、标题的位置映射及块移动后的 ID/位置映射按本文规则维护，实现任务补充具体 API 和测试。

普通标题仍可以只是带 `outlineLevel` 的 Paragraph；这是 Paragraph 的局部结构属性，不是与 `globalSettings` 并列的文档节点或设置对象。标题的默认视觉样式属于主题全局规则，必要时才由 Paragraph 局部格式覆盖。需要显式拥有/删除一组子内容时才用块级 EnvironmentNode。环境标题中的自动编号和标题样式属于可独立编辑的属性，不是靠解析标题文本推断。

字符样式必须保留“未指定”和“显式关闭”的区别；具体解析规则见[文字格式语义](text-formatting.md)。

物理分页和页内片段是排版结果，引用原始文档范围；显式分页符才是文档内容。

编辑和缓存必须按影响分类处理：文字/度量变化使受影响布局失效；全局度量变化可触发完整分页；纯绘制属性只刷新绘制；只有影响大纲语义的变化才更新结构索引。异步布局快照携带单调 `documentGeneration`，过期结果不能覆盖新状态。上述是运行时失效契约，不是 `Paragraph` 持久化字段。当前文档 DTO 使用拥有型 `Vec`，没有 `Arc<Block>` 结构共享或逐节点 `contentRev`、`layoutRev`、`structureRev`、`paintRev` 字段；不得据此假设局部编辑不复制子项容器。超长 Paragraph 与宽子项容器的优化应依据 013 基准测量后决定，不预先承诺 rope、分块表示或结构共享。

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
