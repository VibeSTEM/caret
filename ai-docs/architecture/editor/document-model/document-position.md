# 文档位置与限制

更新日期：2026-11-10。本文定义运行时位置、位置映射及模型深度/尺寸契约。持久化节点语义见[文档树与结构](document-structure.md)，共享 Block 交互见[块宿主交互](block-interaction.md)，导航投影见[结构导航](../structure-navigation.md)，`.cdoc` 格式见[文档存储](document-storage.md)。

## 位置

`Pos` 是文本宿主中的文本边界坐标，其身份由 story ID、终止节点稳定 ID、UTF-8 字节偏移和逻辑边界亲和性组成；它不单独表示空 Inline 内部状态或 Block 虚拟边界。有类型祖先路径（`EnvironmentBody(environmentId)`、`TableCell(tableId, cellId)`、`Paragraph(paragraphId)` 或 `EnvironmentTitle(environmentId, paragraphId)`）可作为可重建的定位与校验投影，不使用数组下标。节点 ID 在文档内唯一，索引可从 ID 重建当前路径；解析器验证路径段类型和父子关系。插入、删除、替换、移动、拆分、合并和解除环境都通过事务返回位置映射；跨父移动保留节点 ID，但旧祖先路径不再作为目标路径。偏移按目标段落逻辑文本的 UTF-8 字节计数。Run 边界和行内 EnvironmentNode 嵌套不改变偏移单位；给定偏移所属的真实 InlineEnvironment 链和 Run 属性派生范围分别从内容树/Run props 计算；适配后的当前环境视图不得给 Run 范围伪造节点身份。Rust 内以 UTF-8 字节作为规范存储单位，键盘左右移动在连续文本中按同一 Unicode 版本的 extended grapheme cluster 移动，不允许落在文本字素簇内部；Qt/C++ UTF-16 索引只在 FFI 边界映射。

具有独立边界语义的 Block 与 Inline 焦点共用三态分类：`外前`、`内部(宿主局部位置)`、`外后`。这是三种状态类型，不是固定三个光标坐标；内部位置由宿主局部结构决定。单个空 InlineEnvironment 或单个空编辑槽只有一个内部插入点；复合 Block 即使内容为空，也可因多个 cell、标题或子宿主拥有多个合法内部位置。若宿主没有合法内部编辑位置，适配器不得伪造 caret。外侧位置即使映射到同一文档偏移，也必须与内部位置保持可区分。普通 Paragraph 使用真实 TextMap 首尾位置，不额外制造虚拟 Block 外侧停靠；复杂 Block 的 `外前/外后` 保存稳定宿主身份和侧向，内部位置由 Section 子树、Equation MathEdit、Align 行/单元格等适配器提供。

对于格式 InlineEnvironment，边界若是合法字素边界，非空范围内部包含从起点到终点的全部合法文本 caret 位置。Right 从外前进入合法内部起点，不越过首个字素；内部按普通字素边界向右移动，到达内部终点后再按 Right 进入外后。Left 对称。若环境结构边界落在跨 Run/环境的扩展字素内部，该结构边界不成为 caret：外前进入内部时映射到环境内方向最近的合法字素边界；从内部起点退出外前时映射到环境外方向最近的合法字素边界。内部终点与外后之间按相同规则对称映射。映射可以跨过包含该结构边界的整个字素簇，但不得在簇内停留；若无法唯一映射，转换原子拒绝并保持文档、焦点、选区及格式上下文不变。空环境仅在其偏移是合法字素边界时提供唯一内部插入点；不得拆开字素制造空槽位。内部位置携带环境 ID/完整环境链或等价的显式宿主上下文，不能只靠二值 `Affinity` 表达。

InlineMath 使用相同的外前｜内部｜外后三态；外侧是 Paragraph/TextMap 中 U+FFFC 原子前后位置，内部位置由 MathEdit adapter 提供。其内部移动由公式局部位置规则决定，到达首尾时再转换为外侧。`InlineEnvironment` 与 `InlineMath` 共享状态仲裁，但不强迫使用相同的局部坐标或内部移动策略。空光标处的 `PendingInlineStyle` 可使用相同粉色槽位几何，但它是待输入样式，不是 Inline 文档位置，也不创建空节点。

`Affinity::Upstream` 与 `Affinity::Downstream` 表示文本逻辑边界侧，不代表屏幕左右或上述三态的通用编码。对于非空 InlineEnvironment，它们可辅助表示文本边界上的外侧/内部归属；空环境内部态与外前、外后在同一偏移重合，必须由带宿主身份的共享位置状态区分。`VisualAffinity` 则表示同一逻辑位置在排版中的视觉候选，不能与文档顺序或 Inline 三态混用。

Paragraph 与 MathFlow 的连续 TextFlow 均按拼接后的 Unicode 文本计算字素边界，Run、InlineEnvironment 与相邻 TextFlow 边界不打断字素分段；可编辑结构原子（Paragraph 中的 `InlineMath`、MathFlow 中的 MathNode）和显式 `SoftBreak` 两侧设置强制停靠点。即使拼接后的文本会把结构占位符/换行与邻接字符组成扩展字素，caret 仍可停在这些结构项两边，结构项和换行可独立选择/删除。Opaque 节点不提供编辑位置。其他简单 LTR 移动按相邻字素边界进行。鼠标命中按布局给出的相邻字素簇边缘选择亲和性；若点击精确落在无法区分两侧的边界，确定性地选择逻辑 `Downstream`。工具栏格式命令保留原光标亲和性；Paragraph 文本选区保留锚点和焦点的完整 `Pos`；跨宿主选区保留共享文档位置及各自宿主局部位置，并在事务改写后通过位置映射恢复。对于 bidi 或换行处同一逻辑位置存在不同视觉 caret 候选时，方向键优先移动到相邻视觉候选并记录其 `VisualAffinity`；只有没有不同视觉候选时，才按[结构导航边界转移](../structure-navigation.md#inline-与-block-的边界转移)在同一逻辑位置切换宿主状态。移动始终落在合法字素边界，不允许三态边界转换制造字素内部 caret。混合方向文本不能按屏幕左/右推断逻辑 upstream/downstream。鼠标命中测试和结构命令也必须明确提交逻辑侧。Paragraph 选区端点携带完整 `Pos`；跨宿主端点使用共享文档位置包装及宿主局部位置，因此选区方向和两端边界语义都能保留。

选区记住锚点和焦点。公式编辑会话记录宿主、活动分支和分支内位置；显式源码草稿的位置是 UTF-8 字节偏移，TextFlow 模式使用共享文本位置，二者都不直接等同于宿主段落 `Pos`。`Paragraph.inlines` 是 `Vec<ParagraphContent>`，行内公式使用 `ParagraphContent::Math` 并在 `TextMap` 中占 U+FFFC 原子位置；Equation/Align 还带公式块及 Align 行列定位。实际目标调用链见[内容节点代码链路](content-code-path.md)。MathFlow 与 Paragraph 的 TextFlow 事务共用文本位置原语，并通过各自宿主路径映射回文档位置；事务失败时保留原文档和有效焦点。多个 InlineEnvironment 边界若重合在同一合法字素位置，位置携带完整环境链并一次转换，不按包装层数增加 caret stop；具体方向转移见[结构导航](../structure-navigation.md)。

Paragraph 是 `Block` 的简单变体，使用完整的 Paragraph/TextMap 位置；不能将复杂 Block 的边界编码成 Paragraph 零长度文本偏移，也不能因为共享 Block 协议而替 Paragraph 加入虚拟块停靠点。InlineMath 外部两侧是 U+FFFC 前后两个位置；非空 InlineEnvironment 的边界归属可由逻辑 affinity 表示，空环境内部位置则由环境身份或等价的显式宿主状态表达。Section、Equation、Align 等复杂 Block 的外部位置由会话持有稳定 Block ID 和前/后侧，内部位置由各自适配器提供（例如 Section 子树位置、Equation 焦点或 Align 行列）。共享选区保留有序 anchor/focus 及方向，并映射到同一文档顺序；各宿主坐标无需相同，也不能在缺少邻接 Paragraph 时凭空构造一个段落位置。Paragraph 的成熟文本编辑行为是抽取共享不变量的基准，但其 paragraph_id/UTF-8 坐标和文本历史映射不是其他 Block 的通用表示。具体移动、停靠和几何规则见[块宿主交互](block-interaction.md)与[结构导航](../structure-navigation.md)。

## 跨宿主位置与顺序

共享交互层需要一个可解析、可比较的文档位置契约，但不要求把所有宿主坐标压成同一种整数偏移或预先采用某个 Rust enum。位置应保留其真实载体：Paragraph 的 `Pos`、Inline 原子两侧及其内部编辑位置、复杂 Block 的外侧边界与宿主内部位置。Block/Inline 共享焦点载体携带稳定宿主身份、外前/内部/外后三态和宿主局部位置；跨宿主全序与 locator 消费该载体，不重复定义边界身份。MathEdit 的目标、分支、草稿和投影身份等编辑上下文，只有在其确实参与焦点恢复或历史时才映射到文档位置；它们不因此全部变成选区端点字段。

文档选区的 anchor 与 focus 都引用完整的可解析位置，并保留方向。共享层必须能确定其文档顺序、构造范围、跨宿主移动两端，并在事务位置映射后恢复位置。`Affinity` 表示同一文本边界的逻辑侧，`VisualAffinity` 表示排版候选的视觉侧；二者不能被混作范围顺序。两个交互停靠状态可以共享同一个文本偏移或排序边界，但只要用户可分别到达或执行不同操作，就不能在状态解析时误合并。

顺序由文档树的父级路径、同级顺序和宿主内坐标共同推导。多个宿主边界若指向同一编辑间隙且没有不同的操作目标，应规范为一个文档位置（例如 Section 最后子块的外后与 Section 内部终点、相邻 Block 之间的前后侧）。需要通过方向进入特定宿主时，由转换器结合相邻顺序恢复目标；父容器自身的外侧边界仍与子树边界可区分。段落序号可用于 Paragraph 导航，但不是全树全序；稳定 ID 用于身份而不是排序。索引或位置解析器必须覆盖嵌套块、标题、空容器、连续公式块及其边界；对于不属于同一可排序 Story、结构已变化或代际不匹配的位置，应显式映射或原子拒绝，不能靠任取一个 Paragraph、ID 字典序或全树临时猜测顺序。

`ExplicitBreak` 与 `Opaque` 当前不都具有可用节点 ID，且 Opaque 可能承载必须保留的未知数据。位置协议不应仅为满足某种键形状就给持久化 DTO 增加 ID，也不应把未知节点假定为可编辑原子。只有当具体编辑能力要求稳定导航、选择或事务定位时，才单独评估其身份、兼容、往返与历史语义。顺序查找的性能目标应由真实文档基准验证；本规范不预先承诺 O(深度) 或任何未测量的复杂度。

## 上限

公式树深度 32（与 `MAX_MATH_TREE_DEPTH` 一致）。表格最多 32767 行、63 列、嵌套 24 层。超过就拒绝这次命令，文档保持原样。
