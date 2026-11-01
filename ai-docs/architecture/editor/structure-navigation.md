# 结构导航

更新日期：2026-10-30。键位见[快捷键](../interface/keyboard-shortcuts.md)，命令入口见[命令](commands.md)。

## 结构语义与界面类别

结构类别、页面角色和节点语义的唯一权威定义见[文档树与结构](document-model/document-structure.md#页面层级与内容块形态)。本文件只规定结构索引、导航和焦点几何如何消费这些定义，不重述页面/内容分类。结构索引保存具体领域语义；界面只消费稳定 ID 和 Rust 投影，不根据标题文字、QML 节点树或具体 kind 再推断一次。

当前 `StructureIndex` 为 Paragraph、由段落大纲属性识别的 Heading、`Section`/`Custom` 块环境以及 Equation/Align 公式块建立条目。Section/Custom 共享可嵌套的标题/子项容器；Section 另有编号语义。二者都投影为 `multiLine`，但不能在 Rust 索引或文档数据中抹掉具体 kind。Custom 的实例名可覆盖通用显示名，模板扩展复用容器模型，不要求新增 QML 类别分支。Equation/Align 条目没有 Paragraph 停靠点，不进入段落导航序列。当前 Paragraph block 的 `ParagraphContent::Text(TextFlow)` 与数学 `MathContent::Text(TextFlow)` 共用文本载体；InlineMath 根流提升后的文本进入 ParagraphContent，固定数学槽位仍在 MathFlow 内。模型及代码边界见[内容节点代码链路](document-model/content-code-path.md)。

行内与块级当前环境的权威适配规则见[行内与块级环境适配](document-model/inline-and-block-environments.md)。行内 `EnvironmentNode`、Run 下划线范围和 `InlineMath` 分别保留自己的载体与编辑入口，不作为块级导航停靠点。

`InlineMath` 是 Paragraph 中的 U+FFFC 原子对象；公式内部编辑当前由 `MathEdit` 通过源码字节游标定位。Equation 与 Align 作为块级公式节点进入结构范围；当前 `MathEdit.target` 标识活动公式，不改变持久化树。Table、Figure 的结构映射在相应文档节点实现并接入索引后启用。

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

044 尚未完成数学焦点到环境几何的适配。完成后，Rust 再用同代布局快照把候选映射为块范围、行内文本选区或公式盒；Qt 消费 `environmentHighlights` 绘制，不推导语义或范围。能力须区分宿主原子边界与公式内部可编辑性。

画布按层级着色：块级为奶油黄，行内级为粉色，两组可以同时绘制且不画轮廓框。颜色来自 QML Theme 的 GUI token，不写入文档或排版主题；滚动和缩放只改变绘制坐标。Qt 只绘制快照给出的矩形，不遍历文档树；光标离开某个环境时只清空对应层级，布局、分页或字体度量变化时改用新快照。分层规则见[画布与交互背景](../interface/canvas-highlighting.md)。

`Mod+Backspace` 解除光标所在的最内层可解除环境：是否可解除由该环境 kind 的能力集决定，不按节点类型逐一判断。

## 光标高度

光标高度只由行内文本度量决定。行内含原子对象（如行内公式）时，光标的顶部与高度取同一行去掉对象后的文本度量，并对齐到本行基线；因此公式旁、公式内外与普通文本处的光标高度一致，行高仍按对象撑开。`LineMetrics` 的 `caret_top_pt` / `caret_height_pt` 携带这一结果，交互快照与完整光标快照使用同一来源。

## 块环境边界

块环境边界支持四个编辑停靠位置：环境前的外部位置、环境内部起点、环境内部终点、环境后的外部位置。方向键第一次越过环境首尾时，必须先停在环境外侧并清除环境高亮，即使外侧已有相邻段落也不直接跳过该停靠点；再次沿同方向移动才进入相邻段落。外侧输入按需在环境前/后创建段落，现有相邻段落不被改写。外侧光标使用环境边界几何，不复用环境内文本的光标 x 坐标；交互快照和完整光标快照必须保持一致。Rust Session 维护位置、环境范围、编辑和撤销，Qt 仅传递方向键意图并绘制新布局。

环境删除、保留内容解除及其光标落点规则以[环境模型](document-model/document-environments.md)为准。状态栏快照只提供稳定的语义 `kind` 与必要层级，不携带本地化标签；QML 使用当前 UI 语言将 `paragraph`、`heading`、`section`、`custom` 翻译并组合。不能把 Section 显示成底层 `MultiLine` 排版类别，也不能通过标题文字或 QML 类型推断语义。
