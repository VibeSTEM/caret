# 文档树与结构

更新日期：2026-10-30。本文是当前 DTO、文档树和页面层级的权威定义。语义草图不等于当前 Rust DTO/JSON 的逐字段描述，不照搬 OOXML，也不承诺 DOCX 兼容；内置与自定义环境的详细规则见[环境模型](document-environments.md)。位置与上限见[文档位置](document-position.md)，文件格式与写入边界见[`.cdoc` 格式](document-storage.md)。

## 当前实现

当前 `caret-doc` 的实际树形 DTO 是 `Document { body: Vec<Block>, page_sections, global_settings }`；`Block` 当前包含 Paragraph、BlockEnvironment、ExplicitBreak 和 Opaque，`InlineNode` 当前包含 Run、InlineEnvironment、SoftBreak 和 Opaque。块环境与行内环境是不同的 Rust 类型，均没有统一的带 `scope` 字段的 `EnvironmentNode` DTO。Heading 由 Paragraph 的属性识别。Equation、Align、Table、Figure 等是语义模型的后续节点，不代表它们已在当前 `caret-doc` 中实现。

本页下文中的 `EnvironmentNode` 是语义层的统称；树形图是目标模型草图。当前 `.cdoc` 的具体字段以 `crates/caret-doc/src/model/` 中的 DTO 和 codec 为准。文档修订、布局缓存代际、焦点和选区属于运行时状态，不序列化进 `Document`。

相关设计各维护一份权威定义：本文维护持久化树、节点语义、页面角色和通用结构类别；[结构导航](../structure-navigation.md)维护索引、导航及几何投影；[多文字体系字体选择](../font-selection.md)维护字体候选与 fallback；[文字格式语义](../text-formatting.md)维护格式解析和格式命令行为。任务文档只记录实现进度与验收，不重复这些契约。

## 文档树与内容模型

### Block 与 Inline 节点

`Block` 和 `InlineNode` 是文档树的两类内容节点，归属不同的容器：

| 节点层级 | 所属容器 | 当前及规划中的内容 |
| --- | --- | --- |
| `Block` | 页面内容的有序块流 | `Paragraph`、块级 `EnvironmentNode`、显式分页；块环境通过有序 `children` 持有可嵌套块内容 |
| `InlineNode` | `Paragraph.inlines` 的有序行内流 | `Run`、行内 `EnvironmentNode`、`SoftBreak`、未知行内节点；行内格式环境通过子节点保存作用范围 |

页面角色组织块流；`Paragraph` 承载行内流并负责段落级排版。行内节点共享段落逻辑文本坐标，Run 边界和行内环境边界不改变 UTF-8 偏移或跨节点字素分段。实际 DTO 以本节开头列出的 `caret-doc` 类型为准。

### 编辑命令与内容插入

“行内内容插入”描述的是编辑操作，不是第三类节点或容器。普通文本输入在光标/选区处更新 Paragraph 的行内树；`\newline` 与 Shift+Enter 插入 `SoftBreak`，保留 Paragraph；Enter 与 `\newparagraph` 执行段落拆分，在所属块流中创建新的 Paragraph。命令模式、按键入口和补全规则由[命令输入](../command-input.md)定义；节点本身及其持久化语义仍由本文定义。

`Run.props` 和 `Paragraph.props` / `Paragraph.layout` 保存直接属性；Run 与 Paragraph 是属性的承载对象，不是格式环境。粗体、斜体和局部字体使用 inline `EnvironmentNode`；下划线等字符装饰是 Run 直接属性。各属性的语义和编辑规则见[文字格式语义](../text-formatting.md)。环境 kind、稳定存储名称、必需能力及模型节点集中定义于 `caret-doc/src/model/inline_environment.rs`；`caret-layout/src/shaping.rs` 将环境和 Run 直接属性投影为排版样式，文档模型不依赖 Parley，也不负责选字体或塑形。格式树及直接属性只能由 Rust 编辑会话事务修改，QML 不维护副本。

Rust 文件按数据职责和编辑职责对应：

```text
crates/caret-doc/src/model.rs           文档根 DTO 与公开导出
crates/caret-doc/src/model/block.rs     Block、块环境、显式分页
crates/caret-doc/src/model/paragraph.rs Paragraph 与段落直接布局覆盖
crates/caret-doc/src/model/run.rs       Run 与 Run 直接属性
crates/caret-doc/src/model/inline.rs    InlineNode 与软换行
crates/caret-doc/src/model/inline_environment.rs 行内环境类型、kind 注册及存储能力
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

`Title` 属于 `SingleLine` 内容块；`TitlePage` 是 `PageContent` 的一种 role，不是物理页或 `PageSection`。TitlePage 与 BodyPage 共用块模型，页级约束由分页布局兑现；标题页与显式分节能力尚未接入时，不应据此宣称已经可编辑或可导航。`PageSection` 只表示纸张、方向和边距等分页配置，与用户可见的环境分开。

Heading、Section、Custom 等具体领域语义由结构索引保留，快照提供稳定语义 kind，界面按当前 UI 语言翻译；通用类别仅用于布局/导航分类，不替代状态栏的语义标签。`Title` 后续映射为 `SingleLine`；`TitlePage` 是更高层的 `PageContent` 页面角色；Equation 映射为 `SingleLine`，Align 映射为 `MultiLine`；Table/Figure 在对应节点接入索引后映射。`InlineMath` 留在 Paragraph 内，不单独进入块结构栈。索引如何维护这些语义见[结构导航](../structure-navigation.md)。

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


详细的块级/行内环境、格式能力和编辑语义见[环境模型](document-environments.md)。

## 文本与实现边界

Run 文本在插入、打开和保存时保留原始 Unicode 码点序列，不隐式转成 NFC 或 NFD。搜索/比较如需规范化，使用独立的规范化视图，不回写或替换文档正文。相邻 Run 的 `language`、`props`、未知可选字段 `extra` 完全相同时，编辑器可在受影响的局部范围合并它们；这些边界不代表光标停靠点。没有文本且无显式属性/语言/未知字段的默认 Run 是无语义占位，可在局部编辑归一化时移除；不能合并格式不同或带未知字段的 Run，也不在打开文档时重写整棵内容树。

物理分页和页内片段是排版结果，引用原始文档范围；显式分页符才是文档内容。文字或度量变化使受影响布局失效，全局度量变化可触发完整分页，纯绘制属性只刷新绘制，只有影响大纲语义的变化才更新结构索引。异步布局快照携带单调 `documentGeneration`，过期结果不能覆盖新状态。布局失效与缓存策略见[增量排版](../incremental-layout.md)。当前 DTO 使用拥有型 `Vec`，没有 `Arc<Block>` 结构共享或逐节点 `contentRev`、`layoutRev`、`structureRev`、`paintRev` 字段。局部编辑不保证只复制受影响节点；超长 Paragraph 与宽子项容器的优化应依据 013 基准测量后决定，不预先承诺 rope、分块表示或结构共享。
