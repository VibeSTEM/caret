# 环境模型

更新日期：2026-10-30。本文是内置环境、自定义环境、标题、编号及环境编辑语义的权威定义。基础 DTO 与页面结构见[文档树与结构](document-structure.md)，位置映射见[文档位置](document-position.md)，格式解析见[文字格式语义](../text-formatting.md)。

`children` 按顺序保存环境拥有的正文节点：块级环境存段落、嵌套块环境、表格等 `Block`；行内环境存 Run、嵌套行内环境等 `InlineNode`。块级环境的 `title` 单独保存，不是 `children` 的首项。整体删除块级环境会删除标题和子树。解除环境则保留所有可见内容：有标题时将其转成普通 Paragraph，放在原位置的提升子项之前；Section 的当前自动编号物化为标题段落中的普通文字。提升子项保留原顺序和节点 ID，标题段落保留自身 ID；环境节点及其编号定义被移除。无标题 Section 按编号规则不显示、不占号，解除时只提升子项，不合成标题段落。

文档级语言字段与语言回退见[多文字体系字体选择](../font-selection.md)；格式优先级与环境模板覆盖见[文字格式语义](../text-formatting.md)。页面几何由 `PageSection` 承载，模型见[文档树与结构](document-structure.md)。
Run 的 Unicode 码点保留、未知字段参与局部合并等规则见[文档树与结构](document-structure.md)。

块级环境的 `title` 始终是单个 Paragraph。标题中按 Enter 时，光标前文本留在标题，后半段移入新建的正文首段，并插到已有正文内容之前；光标在标题开头或末尾时，允许标题前缀或新正文段为空。标题内的文本选区按“删除选区，再在选区起点拆分”一次提交。普通文本 Selection 只接受同一 `Story` 内的范围；GUI 指针拖选若跨越环境标题/正文或环境内外边界，会在 Rust 校验两个合法位置后提升为整块结构选区，并把焦点放到拖动方向对应的环境边界。无效 UTF-8/字素位置不得改变现有选区。标题 Enter 不会隐式改写跨 Story 文本范围。若正文首项是表格或嵌套环境，也在它之前插入 Paragraph。标题不会被拆成多个标题段。IME composition 尚未提交时 Return 由输入法消费，不触发 Enter 命令。

Section 首个正文 Paragraph 的段首 Backspace 删除标题与正文之间的段落边界：把正文首段的行内内容追加到 Section 标题，移除该正文段，并将插入点放到标题原内容之后；Section 本身保留。若标题与正文段落的段落级属性不兼容，操作原子失败，不静默丢弃属性。该操作必须作为单个撤销单元，并通过位置映射把焦点从正文 `Body` story 转到所属 `EnvironmentTitle` story。

块环境首尾分别提供环境内、外的键盘停靠位置。方向键到达外侧停靠点时不立即改写文档；若外侧已有相邻段落，再按一次同方向进入该段落；若没有相邻段落，则保留外侧停靠状态，直到用户输入时才创建外部 Paragraph。此时 Enter 创建一个外部空段落，Shift+Enter 创建一个含软换行的外部段落；之后输入留在该段落。空段落执行 `\section` 时转换为 Section 标题；非空段落执行时保留原段，并在其后创建一个空 Section。

普通 Backspace 在折叠光标位于空环境起点，或从环境末尾外侧停靠点向回删除时可移除空环境；在空 Section 后继外部段落的段首按 Backspace 时，将该段内容合并到 Section 标题末尾并移除外部段落。非空环境不会因普通 Backspace 被删除。若用户通过 Shift+方向键或指针拖选选中整个环境，Backspace 删除该环境及标题、全部后代节点，并作为一个撤销单元；删除后光标落到前一段末尾，若没有前段则落到后一段开头，若文档没有其他段落则留下一个空 Paragraph 供继续编辑。段落内部 Backspace 仍按字素删除，不能因结构选区状态或环境导航状态被吞掉。第一次 Escape 收起整个环境选区，第二次才离开当前结构。撤销或重做后清除不再对应当前文档的虚拟环境外侧停靠状态。

Mod+Backspace 在折叠光标位于行内环境时优先解除最内层行内环境并保留子内容；否则解除当前块环境并保留标题和正文。两种解除均为单次编辑撤销；Rust 根据当前编辑上下文仲裁行内与块环境动作。上述结构编辑由 Rust 事务维护位置映射与撤销状态；索引和导航几何的消费方式见[结构导航](../structure-navigation.md)。

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

- `kind` 来自内置注册表，模板不能声明新的结构语义。未知 kind 和未知能力的打开/保存策略以[`.cdoc` 存储规范](document-storage.md)为准。



- 行内格式环境仅在 kind、状态、模板、属性和上下文兼容时压平或合并；拆分时原节点 ID 留在文档顺序靠前的片段，其余片段分配新 ID；合并时保留顺序靠前的 ID，撤销恢复原树与 ID。具体 Bold/Italic/Font 操作见[文字格式语义](../text-formatting.md)。
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
- 章节编号由 `Section` 持有的编号定义生成，并作为标题 Paragraph 前的虚拟标签绘制。启用编号的 Section 构成编号层级与计数流：顶层共用文档级计数流；子级按最近的已编号 Section 祖先和显示层级计数。关闭编号的 Section 和 `Custom` 对标签路径及计数流都透明，其已编号后代继续使用最近已编号祖先的计数流，因此不同未编号容器下同层后代按文档顺序接续序号。例如 Section `1` 下有未编号容器 A、B，A 内首个编号 Section 为 `1.1`，B 内首个编号 Section 为 `1.2`。关闭编号的节点本身不占序号。模板为编号提供默认显示开关与逐层格式，实例可覆盖；格式至少支持阿拉伯数字、小写/大写罗马数字和中文数字。模板可定制标签显示格式，但不能改变层级和计数关系。`title: None` 的 Section 不显示标签且不占自身序号，作为结构容器对后代编号透明；空标题 Paragraph 则可正常显示标签并占号。编号在视觉上类似行内文本，但不持久化为 Run、不进入标题文本或 UTF-8 偏移，因此插入、移动或删除同一计数流中的 Section 后可重新编号。解除 Section 时，先为整个操作范围快照当前显示标签，再原子提升子树并物化标签，结果不依赖遍历顺序。

块级环境可折叠、可选中和通过结构命令整体删除，默认删除该节点、独立标题及子树；若要保留内容，提供明确的“解除环境”操作，按上述规则把标题和正文移回父级。行内格式环境按文字范围编辑和删除。两者都不能让普通文本删除隐式破坏外层结构。命令事务、失败原子性和后续撤销由 018 衔接。环境种类、scope、编号定义、嵌套约束、标题的位置映射及块移动后的 ID/位置映射按本文规则维护，实现任务补充具体 API 和测试。

普通标题仍可以只是带 `outlineLevel` 的 Paragraph；这是 Paragraph 的局部结构属性，不是与 `globalSettings` 并列的文档节点或设置对象。标题的默认视觉样式属于主题全局规则，必要时才由 Paragraph 局部格式覆盖。需要显式拥有/删除一组子内容时才用块级 EnvironmentNode。环境标题中的自动编号和标题样式属于可独立编辑的属性，不是靠解析标题文本推断。
