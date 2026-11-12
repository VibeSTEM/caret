# 画布与交互背景

更新日期：2026-11-12。本文是文稿画布交互高亮与命令提示背景的唯一权威，定义数据来源、渲染层和复用边界。命令行为见[命令输入](../editor/command-input.md)，环境语义见[结构导航](../editor/structure-navigation.md)，数学节点语义见[数学公式](../editor/math.md)，实现进度见[031 当前环境背景高亮](../../task/031-environment-highlight.md)和[075 Equation/Align 活动焦点高亮](../../task/075-equation-align-active-highlight.md)。

## 两个渲染层

Caret 有两类不同用途的背景：

| 背景 | 表示什么 | 渲染位置 | 几何来源 |
| --- | --- | --- | --- |
| 块焦点高亮 | 当前块环境或活动 Equation/Align，奶油黄 | 文档画布通用块高亮 QSG 层；纸张之上、选区与字形之下 | Rust 分别投影块环境分页范围和 MathFragment 宿主范围；ViewModel 合并到同一列表 |
| 行内焦点高亮 | 当前行内环境、活动 InlineMath、块级公式内的活动 TextFlow island，或空光标处有效的 PendingInlineStyle，粉色 | 文档画布通用 inline 高亮 QSG 层；位于块层之上、选区与字形之下 | Rust 投影行内环境文本视觉行/空范围槽位、InlineMath MathBox、数学 TextFlow island 或 caret/行框派生的 pending-style 槽位；ViewModel 合并到同一列表 |
| 命令提示背景 | 正在输入的命令查询/候选控件 | QML 命令输入/补全控件自身的背景 | QML 控件的内容尺寸、候选尺寸和插入点锚定位置 |

行内对象（InlineMath、行内环境、普通 U+FFFC）的选区几何由布局层统一归一化：每个非零宽对象的版面盒区间始终从同行文本选区矩形中裁除，被完整选中的对象只补一次整盒矩形，首尾相接的同段矩形最终合并；因此选区起点/终点落在对象任一侧时，对象区间恰好出现一次，不因 Parley 把对象推进折进后一字素簇而重复或膨胀。

结构选区覆盖的 Equation/Align 属于文本选区层：Rust 用同代布局快照的 MathFragment 生成整栏对象矩形，与选区端点文本矩形、块环境范围矩形合并后交给选区绘制；它不是块焦点高亮，只在范围完整覆盖该公式对象时出现。公式宿主内部的 MathTree 局部选区也属于文本选区层：Rust 从同代布局快照把同一结构序列中锚点到焦点之间的结构项盒投影为选区矩形，嵌套 Math/Environment 作为整体、Align 按所属行裁切，空范围不发布矩形；Qt 只消费该矩形列表，不重算公式内部选区。块与 inline 焦点是文稿画布内容的临时视觉投影。Paragraph 虽是 Block，但普通段落焦点不自动产生块背景；只有活动环境或公式适配器提供有效高亮范围时才绘制块底色。两个层级可同时显示，各取本层级的活动范围；Qt/QML 保留并绘制两组独立矩形与颜色。文本范围按视觉行切片，公式块使用 MathFragment，数学文本使用 TextFlow island 几何。InlineMath 和 InlineEnvironment 共用 inline 空范围槽位策略：MathBox 已有占位宽度时复用其几何；空格式环境或活动的空光标 PendingInlineStyle 则以布局提供的 caret/行框几何生成非零背景槽位，不能因范围长度为零或尚未产生环境节点就丢失活动反馈。槽位仅用于绘制，不创建文档字符或环境节点；PendingInlineStyle 关闭或焦点移走后不再投影。所有几何必须与页面缩放、分页裁剪、文字、选区和光标处于同一画布坐标与场景图顺序中。Rust 提供领域范围及布局几何，Qt 校验快照并合并同视觉层的来源，QSG 在字形之前绘制背景；这些层不遍历文档树、不重复塑形，也不改写文档或布局状态。

## 活动公式焦点

活动 MathEdit target 为块级 Equation 或 Align 时，Rust 以经过 revision 校验的 `MathEditTarget::Block` 匹配同代 `MathFragment`，生成奶油黄宿主矩形。矩形横跨页面版心宽度，纵向采用 MathFragment 的位置与高度并裁切到页面；Align 按整个宿主着色，不单独着色 cell。InlineMath 不产生块背景。

块级 Equation/Align 中的活动 MathFlow `TextFlow`（包括 `\text{...}`）产生粉色 inline 高亮，覆盖该 island 的完整布局范围。IME 预编辑使用预览 Session 与布局快照重新计算文本岛范围；细线 composition underline 由独立的输入法预编辑层绘制，不拼入粉色背景矩形。文本矩形按所在纸张的 x/y 边界裁切，超出纸张的数学排版不会让高亮快照失效。没有活动空 InlineEnvironment 时，空 TextFlow 的零宽度文本范围不制造额外宽度；若其内部有活动空 InlineEnvironment，则由统一 inline 槽位策略提供非零粉色背景。空公式宿主由块级黄色背景标示。

数学几何与普通环境几何在快照中保留领域来源，Qt ViewModel 分别把它们合并为 `blockHighlightRects` 和 `inlineHighlightRects`。普通 Section 与 Equation/Align 共用同一块层，普通 inline 环境与数学 TextFlow 共用同一 inline 层；两者复用对应颜色、圆角、padding、裁剪和节点更新实现。MathEdit 块目标有效时，不从其正文停靠点推导相邻 Section 的高亮。

命令提示是一个可交互的界面控件，不是文稿内容。它跟随插入点弹出，尺寸由查询和候选内容决定，并需要自己的内边距、焦点和指针命中区域。因此背景属于 QML 控件自身，随控件一起布局和交互；它不进入正文画布的 QSG 文稿层，也不遮盖正文的环境高亮状态。

## 复用边界

块焦点和 inline 焦点使用各自一个通用 QSG 绘制层：绘制器消费矩形列表与样式参数，不理解环境 kind 或数学目标，也不计算文档范围。两个层可有独立的颜色 token 与几何策略；层内的来源共享绘制、裁剪、圆角和节点更新机制，不能为 Section、Equation/Align 或不同 inline 内容复制平行 renderer。inline 的空范围槽位由 Rust 统一适配，Bold 等格式环境与空 InlineMath 使用同一粉色视觉槽位契约；TextMap 范围和 MathBox 内容几何仍由各自载体提供。

命令提示与环境高亮可以共享设计令牌或视觉约定，例如主题色管理方式、圆角规范；但 QML `HighlightBackground` 和 QSG 节点属于不同渲染及交互边界，不能为了“共用背景”把命令弹出控件移入文稿场景图，也不能让 QML 重建正文环境几何。只有确实跨两种渲染系统共用且保持纯数据的样式定义，才应抽为共同主题属性；绘制代码应留在各自的渲染层。

三类背景使用独立的语义颜色 token。环境高亮颜色、圆角和留白由 GUI Theme 提供；命令提示按自身控件尺寸处理内边距，环境高亮按 Rust 布局矩形绘制，不把命令控件的留白直接套到正文范围上。所有背景均为界面状态，不序列化到 `.cdoc`。

## 场景图顺序

文稿画布的相关顺序为：

```text
页面背景
  → 块/行内环境高亮（两组独立几何可同时绘制）
  → 文本选区
  → 字形
  → 光标
```

命令提示控件位于该文稿内容层之外，按 QML 弹出控件的锚定与焦点规则显示；命令模式可以与环境高亮同时存在。完整布局快照解析失败时保留当前已安装的高亮；交互快照若含非法高亮几何则整体拒绝且保留上次有效状态。与当前 document/font generation 不一致的迟到交互结果直接丢弃，不能让旧结果覆盖或清除较新的有效高亮。被识别为可恢复的旧局部布局快照时，保留当前已安装状态并请求完整快照。

环境模型及状态来源见[结构导航](../editor/structure-navigation.md)。
