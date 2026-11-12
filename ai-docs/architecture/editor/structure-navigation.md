# 结构导航

更新日期：2026-11-11。键位见[快捷键](../interface/keyboard-shortcuts.md)，命令入口见[命令](commands.md)。

## 结构语义与界面类别

结构类别、页面角色和节点语义的唯一权威定义见[文档树与结构](document-model/document-structure.md#页面层级与内容块形态)。本文件只规定结构索引、导航和焦点几何如何消费这些定义，不重述页面/内容分类。结构索引保存具体领域语义；界面只消费稳定 ID 和 Rust 投影，不根据标题文字、QML 节点树或具体 kind 再推断一次。

结构索引为 Paragraph、由段落大纲属性识别的 Heading、`Section`/`Custom` 块环境以及 Equation/Align 公式块保留稳定条目和父链。Section/Custom 共享可嵌套的标题/子项容器；Section 另有编号语义。二者都投影为 `multiLine`，但不能在 Rust 索引或文档数据中抹掉具体 kind。Custom 的实例名可覆盖通用显示名，模板扩展复用容器模型，不要求新增 QML 类别分支。Equation/Align 不提供 Paragraph 停靠点；全树文档顺序必须由块身份、父链和同级次序表达，Paragraph ordinal 仅是段落导航的派生序号。Paragraph block 的 `ParagraphContent::Text(TextFlow)` 与数学 `MathContent::Text(TextFlow)` 共用文本载体；InlineMath 创建与源码编辑提交时会把根层文本提升到 ParagraphContent，固定数学槽位仍在 MathFlow 内。规则见[数学公式架构](math.md#宿主边界与拆分)。模型及代码边界见[内容节点代码链路](document-model/content-code-path.md)。

Block 是统一块级宿主类别，Paragraph 是结构最简单且编辑规则最完整的 Block。共享交互协议以 Paragraph 已验证的行为作为提取基准，但不复用其 paragraph_id/UTF-8 坐标作为所有 Block 的位置类型。Paragraph 适配器提供 TextMap/TextFlow 位置；Section/Custom/Equation/Align 适配器提供块身份、父级顺序、必要的块边界及领域内部位置。未来 Table、Figure 必须复用该协议，并通过窄领域适配器暴露单元格或图形内部位置；当前提及扩展契约不表示对应节点已实现。Inline 是 Paragraph 和显式提供行内流的 Block 内部内容层，不是任意块序列的直接子项。

行内与块级当前环境的权威适配规则见[环境视图](document-model/environment-views.md)。行内 `EnvironmentNode`、Run 下划线范围和 `InlineMath` 分别保留自己的载体与编辑入口，不作为块级导航停靠点。

`InlineMath` 是 Paragraph 中的 U+FFFC 原子对象；公式内部编辑由 `MathEdit` 标识宿主与活动分支：Source 分支使用源码字节游标，Text 分支使用 MathFlow 中 TextFlow 的路径和显示偏移。Equation 与 Align 作为块级公式节点进入结构范围；当前 `MathEdit.target` 标识活动公式，不改变持久化树。Table、Figure 的结构映射在相应文档节点实现并接入索引后启用。

`StructureClass` 表达 `Paragraph`、`SingleLine`、`MultiLine`、`Table` 和 `Figure` 内容块类别；页面组合层不是该枚举的同级成员。页面组合角色若参与导航，按文档树父子层级表达，不折叠为内容块类别。

列表项和编号引用作为 Block 层级扩展建模；相应节点接入后，连续列表范围是派生索引，不复制正文节点。列表的持久化与布局语义由其领域架构定义。

## 边界规则

| 单元 | 边界与行为 |
| --- | --- |
| Paragraph | 一个段落是一个结构停靠范围；内部 Run、行内环境和软换行仍属于该段 |
| Heading | 由 Paragraph 的样式/outline level 识别；自身是结构节点。可推导到下一个同级或更高级标题之前的大纲范围，但该范围不是可删除容器 |
| 块环境 | 可嵌套的语义容器；可选 title 是独立 Paragraph，不属于 `children`；结构删除作用于明确的环境子树，保留内容须使用单独的提升操作 |
| 列表项 | 计划中的块语义；列表项可包含一个或多个块，连续同列表引用形成派生列表范围 |
| PageSection | 页面几何配置，不是内容结构 |

删除普通 Heading 只影响该标题段落，不隐式删除标题范围内后续块。删除块环境才按该环境的明确子树边界处理。当前环境背景高亮、字符选区是不同的视图叠加；它们不写入文档格式、不打印，也不修改结构修订。高亮颜色、来源和绘制层以[画布与交互背景](../interface/canvas-highlighting.md)为唯一权威。

## 索引与更新

`caret-structure` 只读 `Document`，不依赖编辑引擎。索引是结构派生数据，不写入文档、不作为布局缓存键。布局几何来自同一代 `DocumentLayoutSnapshot`，避免 Qt 自行估算结构边界。

- 全树节点按父级路径和同级文档顺序排列，用稳定节点 ID 表示身份与父链；保存 kind、层级和宿主范围，不复制正文。Paragraph ordinal 是该索引提供的段落导航派生序号，不作为全树排序键。
- 按任意有效文档位置或稳定宿主 ID 查询其结构祖先链，供状态栏、导航和画布使用；不依赖伪造 Paragraph 光标恢复非 Paragraph 宿主的父链。
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
├── range_extension.rs              # Shift 结构范围单元扩选
├── document_position/range.rs      # 结构范围端点比较与覆盖派生
├── structure/
│   ├── environment_mutations.rs     # 块环境删除、解除等结构变更
│   ├── selection.rs                 # 结构范围删除、替换与落点
│   └── tests.rs                     # Session 结构行为测试

crates/caret-app/src/document_view/  # 生成当前结构与同代布局几何的只读投影
native/viewmodels/document/         # 校验、保存和发布 Rust 快照
native/canvas/document_canvas/      # 绘制结构快照范围，不保存结构真相
qml/Canvas/                         # 画布组合与 Theme 绑定
```

索引按语义拆分 atoms，但保持单一装配入口；不要为每个 kind 建平行索引。Session 命令在有独立职责和足够体量时再拆子目录，不提前创建空模块。Qt/C++ 只映射快捷键、校验快照并绘制 Rust 提供的页内范围；QML 只组合画布和绑定 Theme token。

## 当前环境高亮

当前环境采用语义视图设计，不要求每项都对应同类持久化节点。目标适配器按 Inline 与 Block 分层，具体来源和操作见[环境视图](document-model/environment-views.md)。目标快照保留来源类型与逻辑范围，真实节点使用持久化 ID，Run 属性范围不生成 ID。

数学焦点与普通环境焦点都由 Rust 依据当前 Session 和同代布局快照投影几何，Qt 校验并把同视觉层的领域来源合并后交给画布，不推导语义或范围。Equation/Align 活动焦点与 TextFlow 高亮的几何、图层及复用规则以[画布与交互背景](../interface/canvas-highlighting.md#活动公式焦点)为准；能力区分宿主原子边界与公式内部可编辑性。

画布按层级着色：块级为奶油黄，inline 级为粉色，两组可以同时绘制且不画轮廓框。颜色来自 QML Theme 的 GUI token，不写入文档或排版主题；滚动和缩放只改变绘制坐标。Qt 只绘制快照给出的矩形，不遍历文档树；焦点变化时替换对应层几何，布局、分页或字体度量变化时改用新快照。统一规则见[画布与交互背景](../interface/canvas-highlighting.md)。

`Mod+Backspace` 解除光标所在的最内层可解除环境：是否可解除由该环境 kind 的能力集决定，不按节点类型逐一判断。

## 光标高度

数学宿主沿用与普通文本一致的基础行框、行距和光标高度策略；普通数学字形不应仅因字形轮廓不同而使光标或宿主行框跳变。上标、下标、分数、根式等结构超出正文基准时，宿主行框及必要的光标范围按正文行距规则处理：可扩展的行距模式容纳所需数学内容；固定行距保持固定推进，并按行距契约处理溢出。光标的水平位置、基线和结构内垂直位置仍由数学布局决定；统一高度策略不得改变脚本、分数槽位或 Align 行/单元格定位。

## 复杂 Block 边界停靠

具有独立块外边界语义的复杂 Block 通过共享协议使用外前、内部(宿主局部位置)、外后三态。内部局部位置可到达起点、终点及其间合法位置，不为首尾另造状态类型。外侧状态由稳定 Block ID 与侧向识别，不在文档里预先插入空段落。水平键先停在可到达的块外侧，再进入内部或越过到相邻内容；反向移动遵循相反转换。上下键从相邻视觉行进入块首/末行，并在块顶/底离开到对应外侧。MathTree/MathText 候选不得跳过适用的块边界状态。Section/Custom 与 Equation/Align 通过各自内容适配接入；Paragraph 仍使用其真实 TextMap 首尾位置，不强制额外的虚拟停靠状态。InlineMath 是 Paragraph 中的 inline 原子，其外部两侧由 TextMap 的 U+FFFC 位置表达，内部位置由数学编辑器提供。

## Inline 与 Block 的边界转移

Inline 与具有独立外侧停靠语义的复杂 Block 都按三类位置处理：`外前｜内部(宿主局部位置)｜外后`。这是三种位置类别；内部承载宿主适配器提供的合法局部位置。单个空 InlineEnvironment 或单个空编辑槽只有一个内部插入点；复合 Block 即使内容为空，也可有多个合法空槽或子宿主位置。普通 Paragraph 使用真实 TextMap 首尾，不增加虚拟外侧位置。载体身份和局部坐标由适配器提供；`Affinity` 不替代三态位置身份。

下表中的 Right/Left 表示简单 LTR 情况下的水平移动。若 bidi 或折行使同一逻辑位置存在不同视觉候选，先按布局选择相邻视觉候选，再映射到逻辑宿主位置；不得把屏幕左右直接等同于文档前后。

### Inline 转移

| 起点 | 按键 | 目标 | 规则 |
| --- | --- | --- | --- |
| InlineEnvironment 外前 | Right | 内部位置：起点 | 边界合法时在同一逻辑位置进入；否则按下方字素映射规则 |
| InlineEnvironment 内部位置：起点 | Left | 外前 | 边界合法时在同一逻辑位置退出；否则按下方字素映射规则 |
| InlineEnvironment 内部位置：非终点 | Right | 下一个内部字素边界 | 按 TextFlow 字素规则移动 |
| InlineEnvironment 内部位置：非起点 | Left | 上一个内部字素边界 | 按 TextFlow 字素规则移动 |
| InlineEnvironment 内部位置：终点 | Right | 外后 | 边界合法时在同一逻辑位置退出并更新格式视图；否则按下方字素映射规则 |
| InlineEnvironment 外后 | Left | 内部位置：终点 | 边界合法时在同一逻辑位置进入；否则按下方字素映射规则 |
| 空 InlineEnvironment 外前 | Right | 唯一内部位置 | 同一合法字素偏移进入，激活环境链和粉色槽位 |
| 空 InlineEnvironment 内部位置 | Left | 外前 | 同一偏移退出 |
| 空 InlineEnvironment 内部位置 | Right | 外后 | 同一偏移退出并清除该环境的活动状态 |
| 空 InlineEnvironment 外后 | Left | 唯一内部位置 | 同一合法字素偏移进入，激活环境链和粉色槽位 |
| InlineMath 外前 | Right | MathEdit 内部起点 | 进入公式内部；不创建 Block 停靠点 |
| MathEdit 内部起点 | Left | InlineMath 外前 | 按 MathEdit 局部边界退出到 U+FFFC 前侧 |
| MathEdit 内部终点 | Right | InlineMath 外后 | 按 MathEdit 局部边界退出到 U+FFFC 后侧 |
| InlineMath 外后 | Left | MathEdit 内部终点 | 进入公式内部；不跳过末端位置 |
| MathEdit 内部顶行边界 | Up | InlineMath 外前 | 无内部垂直候选时退出到 U+FFFC 前侧；同一次按键继续文档纵向导航 |
| MathEdit 内部底行边界 | Down | InlineMath 外后 | 无内部垂直候选时退出到 U+FFFC 后侧；同一次按键继续文档纵向导航 |
| InlineMath 相邻视觉行 | Up/Down | 相邻视觉文本 caret | 垂直移动不进入 InlineMath |
| `Run.props.underline` 范围外前 | Right | 范围内部起点 | 在合法字素偏移进入内部归属，不跳过首个字素 |
| Run 属性范围内部起点 | Left | 范围外前 | 在同一合法字素偏移更新 `Affinity` |
| Run 属性范围内部位置：非终点 | Right | 下一个内部字素边界 | 按 TextFlow 字素规则移动 |
| Run 属性范围内部位置：非起点 | Left | 上一个内部字素边界 | 按 TextFlow 字素规则移动 |
| Run 属性范围内部终点 | Right | 范围外后 | 在同一合法字素偏移更新 `Affinity` |
| `Run.props.underline` 范围外后 | Left | 范围内部终点 | 在合法字素偏移进入内部归属，不跳过末个字素 |
| 空光标的 `PendingInlineStyle` | Left/Right | 上/下一个普通文本位置 | 移动时清除待输入格式与粉色槽位；关闭格式时在原位置清除。它不创建零长度范围或额外 caret 步数 |

相邻或嵌套的 InlineEnvironment 若边界落在同一合法字素偏移，不为每个环境额外生成同坐标 caret。单次进入/退出该重合边界时，位置携带并更新完整有序环境链；插入格式按该链解析。边界偏移不同的环境仍分别转换。空嵌套环境继续合成为唯一内部 stop。Run 属性范围不提供宿主身份；非空范围由 `Affinity` 表达边界归属，空光标由 `PendingInlineStyle` 表达待输入格式。空光标启用格式时，在当前位置设置 `PendingInlineStyle` 并显示粉色槽位，不增加 caret 位置或移动步数；关闭格式或移动光标时清除待输入格式与槽位。Bold 与 Underline 在同一边界的进入、退出步数一致。

若 InlineEnvironment 的结构边界落在跨 Run/环境的扩展字素内部，该结构边界不是 caret。进入时映射到环境内方向最近的合法字素边界，退出时对称地映射到环境外方向最近的合法字素边界；映射可越过整个相关字素簇，不得落在簇内。若映射不唯一，拒绝转换并保留文档、焦点、选区与格式上下文。空 InlineEnvironment 只有位于合法字素边界时才有内部插入点。

InlineMath 是行内原子，Up/Down 维持段落视觉行之间的导航，不进入该原子的内部；复杂 Block 有独立块级视觉行，故有效的上下候选可进入其内部首/末位置。在 InlineMath 编辑中，Up/Down 先使用公式内部垂直候选；到达公式顶/底边界后按局部边界退出到 U+FFFC 前/后侧，并由 app 在同一次按键内继续文档纵向导航；没有布局候选时保留退出后的位置，不伪造字素步长。

### 复杂 Block 转移

以下规则适用于具有独立外侧停靠语义的 Section/Custom/Equation/Align 等 Block。外侧停靠身份由稳定 Block ID 与前/后侧表示；内部由各自适配器提供。方向键不能跳过适用的外侧停靠。

Block 的位置类别只有 **外前、内部、外后**。表中的“内部位置：起点/终点”是内部位置携带的局部坐标，不是新增状态；“前方文档末端/后方文档起点”是相邻正文位置，也不属于 Block 状态。单个空编辑槽的 Block 按表中唯一内部位置转换；多个空 cell/子宿主各保留其 adapter 提供的内部位置，不合并成一个点。若 Block 不提供合法内部位置，则不伪造内部 caret。

| 起点 | 按键 | 目标 | 规则 |
| --- | --- | --- | --- |
| 前方文档末端 | Right | Block 外前 | 到达可独立停留的块前停靠 |
| Block 外前 | Left | 前方文档末端 | 离开块前停靠 |
| Block 外前 | Right | 内部位置：起点 | 进入首个合法内部位置 |
| Block 内部位置：起点 | Left | Block 外前 | 先停在块前；再次向左才进入前方内容 |
| Block 内部位置：终点 | Right | Block 外后 | 先停在块后；再次向右才进入后方内容 |
| Block 外后 | Right | 后方文档起点 | 离开块后停靠 |
| Block 外后 | Left | 内部位置：终点 | 进入最后一个合法内部位置 |
| 后方文档起点 | Left | Block 外后 | 到达可独立停留的块后停靠 |
| 空单槽 Block 外前 | Right | 唯一内部位置 | 进入空槽位 |
| 空单槽 Block 内部位置 | Left | 外前 | 同一位置向前退出 |
| 空单槽 Block 内部位置 | Right | 外后 | 同一位置向后退出 |
| 空单槽 Block 外后 | Left | 唯一内部位置 | 进入空槽位 |
| Block 外侧相邻视觉行 | Up/Down | 内部位置：首行/末行 | 有效视觉候选直接进入块内 |
| Block 内部位置：顶/底边界 | Up/Down | Block 外前/外后 | 仅在没有块内视觉候选时，转到对应外侧停靠 |
| Block 外前/外后停靠 | Up/Down | 相邻视觉行的最近合法位置 | 先取共享视觉行候选；候选属于相邻 Paragraph/Block 时进入该位置，不凭键向猜测文档顺序 |

Block 的邻接外侧停靠只在相邻内容实际存在且属于同一有效文档顺序时到达。文档首尾没有前方/后方内容时，水平键在对应 Block 外侧停靠保持原位；Section 子项边缘按宿主父链寻找最近可达的父级边界。不得合成空 Paragraph 或跳到不相邻的正文兜底。

### 嵌套与相邻宿主的重合边界

多个宿主边界若对应同一可编辑文档间隙，且没有不同的插入目标，则解析为一个规范位置，不因宿主层数或相邻宿主数增加重复 caret。移动方向决定从该间隙进入左侧/右侧宿主；位置携带解析所需的宿主链和局部坐标。

- Equation/Align 是 Section 最后一个子项时，子 Block 的外后与 Section 的内部终点是同一规范边界；再向右才进入独立的 Section 外后。首子项的外前与 Section 内部起点对称处理。
- 同级 Equation/Align 相邻时，前一块外后与后一块外前合并为两块之间的单一规范间隙；向左进入前块，向右进入后块。Section/Custom 等容器边界按同样的父链规则解析。
- 非空 InlineEnvironment 的嵌套边界若在同一字素偏移重合，单次边界转换同时更新完整环境链，不逐层添加 caret 步数；只有偏移不同的真实编辑边界才有独立停靠。格式继承仍使用转换后的活动环境链。

规范边界保留足以恢复宿主及方向的稳定身份，不依赖 Paragraph ordinal 或 ID 字典序。若两个宿主边界在位置相同但插入/编辑目标不同，则它们不是同一规范间隙，适配器必须给出可区分的局部位置。

Escape 离开当前活动的复杂 Block 时进入该 Block 的外后停靠；重复 Escape 保持在同一停靠，不越过相邻 Block。结构选区存在时，Escape 先折叠该范围并保留当前光标与块外侧停靠，不进入相邻宿主。需要继续浏览或进入相邻宿主时使用方向键，由共享文档顺序仲裁；宿主适配器不搜索远处 Paragraph 作为 Escape 的替代落点。结构选区存在时，不带 Shift 的 Left/Right 先折叠到文档顺序的起点/终点并消费该次按键，块外侧边界端点复用块外前/外后停靠；规则见[块宿主交互](document-model/block-interaction.md#跨-block-结构选区与历史)。

Paragraph 继续使用其真实 TextMap 首尾，不套用复杂 Block 的虚拟停靠。Shift 扩选跨越宿主时使用共享文档顺序和稳定宿主范围，不从普通移动次数推导选区边界；位置比较、选区范围和删除事务协议见[块宿主交互](document-model/block-interaction.md)。

Shift+水平键扩展结构范围时按文档单元前进：段落内部按字素边界，Section/Custom 与 Equation/Align 作为原子对象一次进入范围，相邻公式块逐个纳入而不跳过。范围与块环境边界相交时扩展到整棵环境，与鼠标跨越环境边界一致；该扩展仍由 Session 仲裁，布局候选只决定普通文本的视觉邻接。公式宿主内部的扩选先按 MathTree/MathText 局部语义移动：MathTree 扩选不进入嵌套结构，离开锚点所在序列（含 Align 当前 cell 边界）时转换为覆盖整个公式对象的结构范围；MathText 扩选仍在文本岛内移动；同方向没有其它文本岛时（即使后面仍有非文本数学内容）转换为整个公式对象。Shift+垂直键需要布局给出的相邻视觉行候选；没有候选时只在相邻块是显示公式时纳入该对象，不用字素步长伪造视觉行移动。InlineMath 编辑中的 Shift+Up/Down 先按上述边界规则退出公式并以 U+FFFC 侧位置作为扩展锚点，再按同一文档纵向候选规则扩展普通文档选区。

结构索引从任意有效宿主 ID 解析稳定父链和同级顺序。只读查询与可变内容事务使用各自经过校验的定位入口，并共享同一身份、父链、顺序与文档代际语义。关于身份、顺序、代际与无法比较时的处理见[文档位置](document-model/document-position.md#跨宿主位置与顺序)。

Paragraph 与复杂 Block 共用同一个位置仲裁、选区端点和删除事务协议；它们的端点表示并不相同。结构索引提供稳定顺序和宿主关系，布局适配器提供同代几何，Session 负责转换 anchor/focus 和动作目标。普通 Paragraph 不要求块级活动背景；需要背景的环境/公式通过可选几何能力提供。添加新 Block kind 时只实现其内容/几何适配，不复制状态机或快捷键仲裁。InlineMath 与行内格式环境仍由 Paragraph/内部 TextFlow 的 Inline 底座管理。共享协议详见[块宿主交互](document-model/block-interaction.md)。

在块前/后外侧按 Enter、输入文字或提交输入法时，事务在目标块同级插入段落，再把焦点映射到新段落；不改写已有相邻段落。段落插入、焦点恢复、外侧停靠状态和撤销补丁属于一个原子事务。外侧 caret 几何由块适配器投影到共享边界查询：Section 使用容器范围；居中 Equation/Align 使用所在页面内容行的行首/行尾，Align 的垂直位置对应首行/末行。绘制、指针命中和交互快照消费同一几何。

公式块后的空段落段首按 Backspace 时，删除该空段落并进入公式末端；撤销恢复空段落与原焦点，重做再次进入公式。该规则不删除非空公式内容。

拥有自身逻辑行/单元格模型的多行块共享行首退格契约：整行为空时 Backspace 删除该行；非空行在行首按 Backspace 不改内容、不合并行、不移动焦点。这里的“行”指块内部的逻辑结构行，不包括段落自动折行或行内软换行。共享策略统一分类空行删除、非空行首无操作和其他位置交给 cell 编辑；各模型适配器只负责行定位、整行判空、安全检查和焦点恢复。当前 Align 是唯一支持 Enter 创建结构行的块，后续表格等行模型应复用该策略。

Section 虽可跨多段正文并在结构索引中归类为 `MultiLine`，但它是标题加 `Vec<Block>` 子块的容器，没有内部行/cell 模型；其中 Paragraph 仍按段落编辑规则处理。尤其 Section 首个正文 Paragraph 行首 Backspace 有专门的标题合并语义，不能套用逻辑表格行的 no-op 规则。是否视觉上占多行或结构索引的 `MultiLine` 分类本身不足以决定退格行为。

块类型只提供稳定 ID、父级路径、块边界几何和同级插入适配；外侧状态、键盘仲裁、段落物化、输入法预编辑和历史行为共用单一实现。Section 环境、Equation、Align 与后续块类型不得各自复制一套边界状态或插入逻辑。Rust Session 持有权威状态，Qt 传递导航意图并绘制新布局。

环境删除、保留内容解除及其光标落点规则以[环境模型](document-model/document-environments.md)为准。状态栏快照只提供稳定的语义 `kind` 与必要层级，不携带本地化标签；QML 使用当前 UI 语言将 `paragraph`、`heading`、`section`、`custom` 翻译并组合。不能把 Section 显示成底层 `MultiLine` 排版类别，也不能通过标题文字或 QML 类型推断语义。
