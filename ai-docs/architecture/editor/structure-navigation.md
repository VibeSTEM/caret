# 结构导航

更新日期：2026-11-09。键位见[快捷键](../interface/keyboard-shortcuts.md)，命令入口见[命令](commands.md)。

## 结构语义与界面类别

结构类别、页面角色和节点语义的唯一权威定义见[文档树与结构](document-model/document-structure.md#页面层级与内容块形态)。本文件只规定结构索引、导航和焦点几何如何消费这些定义，不重述页面/内容分类。结构索引保存具体领域语义；界面只消费稳定 ID 和 Rust 投影，不根据标题文字、QML 节点树或具体 kind 再推断一次。

当前 `StructureIndex` 为 Paragraph、由段落大纲属性识别的 Heading、`Section`/`Custom` 块环境以及 Equation/Align 公式块建立条目。Section/Custom 共享可嵌套的标题/子项容器；Section 另有编号语义。二者都投影为 `multiLine`，但不能在 Rust 索引或文档数据中抹掉具体 kind。Custom 的实例名可覆盖通用显示名，模板扩展复用容器模型，不要求新增 QML 类别分支。Equation/Align 条目没有 Paragraph 停靠点，不进入段落导航序列。Paragraph block 的 `ParagraphContent::Text(TextFlow)` 与数学 `MathContent::Text(TextFlow)` 共用文本载体；InlineMath 创建与源码编辑提交时会把根层文本提升到 ParagraphContent，固定数学槽位仍在 MathFlow 内。规则见[数学公式架构](math.md#宿主边界与拆分)。模型及代码边界见[内容节点代码链路](document-model/content-code-path.md)。

行内与块级当前环境的权威适配规则见[行内与块级环境适配](document-model/inline-and-block-environments.md)。行内 `EnvironmentNode`、Run 下划线范围和 `InlineMath` 分别保留自己的载体与编辑入口，不作为块级导航停靠点。

`InlineMath` 是 Paragraph 中的 U+FFFC 原子对象；公式内部编辑由 `MathEdit` 标识宿主与活动分支：Source 分支使用源码字节游标，Text 分支使用 MathFlow 中 TextFlow 的路径和显示偏移。Equation 与 Align 作为块级公式节点进入结构范围；当前 `MathEdit.target` 标识活动公式，不改变持久化树。Table、Figure 的结构映射在相应文档节点实现并接入索引后启用。

当前 `StructureClass` 只有 `Paragraph`、`SingleLine`、`MultiLine`、`Table` 和 `Figure` 内容块类别；页面组合层不是该枚举的同级成员。未来页面组合角色是否进入导航索引，按文档模型的层级语义另行接入。

当前文档模型尚无列表项节点/编号引用；列表节点、嵌套/多段列表项语义和导航由 [050](../../task/050-document-code-links-quotes.md) 定义，布局几何由 051 承接。列表连续范围是派生索引，不复制正文节点。

## 边界规则

| 单元 | 边界与行为 |
| --- | --- |
| Paragraph | 一个段落是一个结构停靠范围；内部 Run、行内环境和软换行仍属于该段 |
| Heading | 由 Paragraph 的样式/outline level 识别；自身是结构节点。可推导到下一个同级或更高级标题之前的大纲范围，但该范围不是可删除容器 |
| 块环境 | 可嵌套的语义容器；可选 title 是独立 Paragraph，不属于 `children`；结构删除作用于明确的环境子树，保留内容须使用单独的提升操作 |
| 列表项 | 计划中的块语义；列表项可包含一个或多个块，连续同列表引用形成派生列表范围 |
| PageSection | 页面几何配置，不是内容结构 |

删除普通 Heading 只影响该标题段落，不隐式删除标题范围内后续块。删除块环境才按该环境的明确子树边界处理。当前环境背景高亮、字符选区是不同的视图叠加；它们不写入文档格式、不打印，也不修改结构修订。当前环境背景高亮的视觉规则由[031](../../task/031-environment-highlight.md)定义。

## 索引与更新

`caret-structure` 只读 `Document`，不依赖编辑引擎。索引是结构派生数据，不写入文档、不作为布局缓存键。布局几何来自同一代 `DocumentLayoutSnapshot`，避免 Qt 自行估算结构边界。

- 节点按文档顺序排列，用稳定节点 ID 表示身份和父链；保存 kind、层级与段落序号范围，不复制正文。
- `at(pos)` 沿稳定 ID 查出光标所在的外到内结构栈，供状态栏与画布使用。
- Heading 的跨块大纲范围和列表连续范围是派生数据。当前结构事务提交后从文档树完整重建轻量结构索引；撤销/重做恢复文档后也会重建索引。标题插入、删除、级别变化后的范围因此由同一构建逻辑重新计算。暂不承诺局部增量更新，只有代表性文档的基准表明完整重建成为热点时才设计增量方案。
- 块环境子树范围来自文档树，不从 Heading 层级推断。单纯绘制、分页几何或主题来源变化不重建结构索引；文档历史恢复会重建，以确保派生索引与恢复后的文档一致。索引变化与布局失效分别处理。
- 每页的块环境范围由该页实际布局片段聚合，包含稳定环境 ID 和页面局部 pt 几何。跨页环境由多个页内范围表示，不能用一个跨页矩形近似。
- 每次结构变更后的索引都由完整文档树构建，并以稳定 ID 与事务位置映射维护拆分、合并、移动和撤销后的结构归属。若未来引入增量更新，必须针对相同文档与完整重建结果做等价性验证。

## 键盘作用域

结构导航按文档内部的最深可编辑对象优先；公式、表格行为在对应节点接入后逐步加入：

```text
公式内部 → 表格单元格/行 → 列表项 → 段落与块环境 → 正文
```

命令输入、IME 预编辑、选区和 UI 弹层是键盘交互上下文，不是文档结构层级。事件先由最内层焦点所有者接收；进入 Rust 的结构/编辑意图再由 Session 仲裁，详见[键盘路由](../interface/keyboard-routing.md)。例如表格内“上一块/下一块”先按行移动，越过首/末行时再离开表格；没有表格节点前不启用这项行为。

## 实现文件边界

```text
crates/caret-structure/src/
├── index.rs                       # 唯一索引装配入口与结构查询
├── index/tests.rs                 # 索引外部可观察结果
├── atoms.rs                       # 结构原子声明与内部导出
└── atoms/
    ├── paragraph.rs               # Paragraph / Heading 语义
    ├── block_environment.rs        # 块环境容器语义
    └── list_item.rs                # 列表实现后新增

crates/caret-engine/src/editing/session/
├── structure.rs                    # 结构命令入口与事务边界
└── structure/                      # 多职责增长后再拆分导航、环境操作

crates/caret-app/src/document_view/  # 生成当前结构与同代布局几何的只读投影
native/viewmodels/document/         # 校验、保存和发布 Rust 快照
native/canvas/document_canvas/      # 绘制结构快照范围，不保存结构真相
qml/Canvas/                         # 画布组合与 Theme 绑定
```

索引按语义拆分 atoms，但保持单一装配入口；不要为每个 kind 建平行索引。Session 命令在有独立职责和足够体量时再拆子目录，不提前创建空模块。Qt/C++ 只映射快捷键、校验快照并绘制 Rust 提供的页内范围；QML 只组合画布和绑定 Theme token。

## 当前环境高亮

当前环境采用语义视图设计，不要求每项都对应同类持久化节点。目标适配器按 Inline 与 Block 分层，具体来源和操作见[行内与块级环境适配](document-model/inline-and-block-environments.md)。目标快照保留来源类型与逻辑范围，真实节点使用持久化 ID，Run 属性范围不生成 ID。

数学焦点与普通环境焦点都由 Rust 依据当前 Session 和同代布局快照投影几何，Qt 校验并把同视觉层的领域来源合并后交给画布，不推导语义或范围。Equation/Align 活动焦点与 TextFlow 高亮的几何、图层及复用规则以[画布与交互背景](../interface/canvas-highlighting.md#活动公式焦点)为准；能力区分宿主原子边界与公式内部可编辑性。

画布按层级着色：块级为奶油黄，inline 级为粉色，两组可以同时绘制且不画轮廓框。颜色来自 QML Theme 的 GUI token，不写入文档或排版主题；滚动和缩放只改变绘制坐标。Qt 只绘制快照给出的矩形，不遍历文档树；焦点变化时替换对应层几何，布局、分页或字体度量变化时改用新快照。统一规则见[画布与交互背景](../interface/canvas-highlighting.md)。

`Mod+Backspace` 解除光标所在的最内层可解除环境：是否可解除由该环境 kind 的能力集决定，不按节点类型逐一判断。

## 光标高度

数学宿主沿用与普通文本一致的基础行框、行距和光标高度策略；普通数学字形不应仅因字形轮廓不同而使光标或宿主行框跳变。上标、下标、分数、根式等结构超出正文基准时，宿主行框及必要的光标范围按正文行距规则处理：可扩展的行距模式容纳所需数学内容；固定行距保持固定推进，并按行距契约处理溢出。光标的水平位置、基线和结构内垂直位置仍由数学布局决定；统一高度策略不得改变脚本、分数槽位或 Align 行/单元格定位。

## 块边界停靠

所有可编辑块容器共享四个停靠状态：块前外侧、块内起点、块内终点、块后外侧。外侧状态由 Session 持有目标块 ID 与方向，不在文档里预先插入空段落。方向键第一次越过块首尾时停在对应外侧；再次沿同方向移动才越过外侧状态进入相邻内容，反方向则进入块内首/末位置。左右键遵循相同的状态转换；上下键从相邻视觉行进入块首/末行，并在块顶/底离开到相应外侧状态。MathTree/MathText 的结构候选不得跳过该外侧状态。

在块前/后外侧按 Enter、输入文字或提交输入法时，事务在目标块同级插入段落，再把焦点映射到新段落；不改写已有相邻段落。段落插入、焦点恢复、外侧停靠状态和撤销补丁属于一个原子事务。外侧 caret 几何由块适配器投影到共享边界查询：Section 使用容器范围；居中 Equation/Align 使用所在页面内容行的行首/行尾，Align 的垂直位置对应首行/末行。绘制、指针命中和交互快照消费同一几何。

公式块后的空段落段首按 Backspace 时，删除该空段落并进入公式末端；撤销恢复空段落与原焦点，重做再次进入公式。该规则不删除非空公式内容。

拥有自身逻辑行/单元格模型的多行块共享行首退格契约：整行为空时 Backspace 删除该行；非空行在行首按 Backspace 不改内容、不合并行、不移动焦点。这里的“行”指块内部的逻辑结构行，不包括段落自动折行或行内软换行。共享策略统一分类空行删除、非空行首无操作和其他位置交给 cell 编辑；各模型适配器只负责行定位、整行判空、安全检查和焦点恢复。当前 Align 是唯一支持 Enter 创建结构行的块，后续表格等行模型应复用该策略。

Section 虽可跨多段正文并在结构索引中归类为 `MultiLine`，但它是标题加 `Vec<Block>` 子块的容器，没有内部行/cell 模型；其中 Paragraph 仍按段落编辑规则处理。尤其 Section 首个正文 Paragraph 行首 Backspace 有专门的标题合并语义，不能套用逻辑表格行的 no-op 规则。是否视觉上占多行或结构索引的 `MultiLine` 分类本身不足以决定退格行为。

块类型只提供稳定 ID、父级路径、块边界几何和同级插入适配；外侧状态、键盘仲裁、段落物化、输入法预编辑和历史行为共用单一实现。Section 环境、Equation、Align 与后续块类型不得各自复制一套边界状态或插入逻辑。Rust Session 持有权威状态，Qt 传递导航意图并绘制新布局。

环境删除、保留内容解除及其光标落点规则以[环境模型](document-model/document-environments.md)为准。状态栏快照只提供稳定的语义 `kind` 与必要层级，不携带本地化标签；QML 使用当前 UI 语言将 `paragraph`、`heading`、`section`、`custom` 翻译并组合。不能把 Section 显示成底层 `MultiLine` 排版类别，也不能通过标题文字或 QML 类型推断语义。
