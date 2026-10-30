# 页面与段落排版

更新日期：2026-10-30。本文定义 `.cdoc` 页面几何、行框、段落间距和分页的产品规则，以及布局层的职责。文档属性属于 `Document`；页、行和屏幕上的字形位置均为派生结果。文档树与样式归属见[文档模型](document-model.md)，缓存失效见[增量排版](incremental-layout.md)，Caret 默认数值见[默认排版预设](../../../crates/caret-doc/resources/layout-themes/default.json)，OOXML 映射依据见[008 研究](../../research/008-office-open-xml-spacing-pagination.md)。

## 页面几何与单位

页面尺寸、方向和四边边距由 `PageSection` 管理，与内容环境 `EnvironmentNode` 无关。M0 使用一个 `PageSection` 覆盖整份正文；分节范围和文档内分节符留待后续设计。

物理长度统一使用 pt：`1 in = 72 pt = 25.4 mm`。M0 纸型为 A4 或 A3，默认 A4 纵向、四边各 72 pt。方向只交换页面宽高，不旋转页面图像。页面边长扣除左右、上下边距后得到版心；边距必须是有限非负值，且对边之和小于对应页面边长。零边距有效。装订线、页眉和页脚距离不属于 M0 页面几何。

A4 为 210 × 297 mm，约 595.276 × 841.890 pt；A3 为 297 × 420 mm，约 841.890 × 1190.551 pt。内部保留足够精度；界面显示到 0.01 mm 时，换算误差不超过 0.005 mm。交换格式只在边界换算并明确定义舍入。几何保存在 `PageSection.props` 的 `paperSize`、`orientation`、`marginsPt` 中。旧文件缺少这些可选键时使用默认几何；未知键保留；已知键类型错误、非有限值或无效几何时拒绝文档，且不改变文档和 Undo 状态。

页面边缘、工作区留白、页间距、阴影、缩放和 DPR 是视图属性，不得写回物理页面尺寸。自动分页是派生结果，不写入正文；显式分页符属于文档内容。开头、连续和末尾的显式分页符都保留对应空页。

## 行框、基线与字形

排版需要分别计算三件事：

| 概念 | 含义 | 影响 |
| --- | --- | --- |
| 行框与行推进 | 行占用的布局高度，以及下一行/段落从哪里开始 | 行距、段距、分页 |
| 主基线 | 当前行的基线参照 | 不同脚本及字号的字形如何对齐 |
| 字形 ink bounds | 字形实际绘制的范围，可能超出 advance 或行框 | 绘制、裁剪和溢出 |

Caret 采用 [Microsoft Learn 的 OpenType BASE 表说明](https://learn.microsoft.com/en-us/typography/opentype/spec/base)作为不同脚本基线对齐的依据。该页面是 Microsoft 发布的公开 OpenType 技术规范文档。BASE 模型以一个 script/size 的 dominant run 为参照，其他脚本基线相对于它定义；字体提供的是建议基线，排版客户端仍可采用自己的基线策略。BASE 还可按脚本、语言系统或特性提供字形范围度量。

段落有效语言按 `Paragraph.language`、文档 `defaultLanguage` 的顺序选择，用于塑形和 BASE 语言系统上下文。行距网格与脚本基线锚点是两项独立决策：行距网格使用文档/段落基础样式指定字体及字号，以该字体的 Latin 度量作为稳定基准，不因正文的语言标签、首字符 Script 或同字号 fallback face 改变；文档默认语言只用于解析该基础字体的语言相关回退。脚本基线锚点则取整段逻辑文本（包括软换行两侧）中第一个强 Unicode Script。`Common`、`Inherited` 与 `Unknown` 不参与锚点选择；前缀标点归入其后第一个强脚本，其他此类字符沿用塑形 run 的脚本。若整段没有强脚本，使用 `Latn` 的确定回退。锚点确定一次，不能因软换行或后续视觉行的脚本变化而切换。dominant run 是第一个属于锚点脚本的实际塑形 run，使用 Parley 已解析出的字体 face、字号、语言和 variation 实例；这避免用未安装的请求字体或全局默认字体代替真实 fallback 字体。阿拉伯语段落若以阿拉伯字符开始，就以 Arab run 为基线参照；混排段落中的其他脚本按 BASE 中相对于该参照定义的基线对齐。

行内脚本归属使用塑形器为每个实际 run 确定的 Script；锚点选择只看首个强脚本，不因后续 Common/Inherited 字符而改变。单独插入一个全角符号不应改变 dominant run，也不应把既有文字或后续行整体推移。若字体缺少 BASE 表、对应脚本或所需 baseline tag，按当前字体各自的 alphabetic/default baseline 与 Parley run metrics 对齐，不伪造零坐标、不借用无关拉丁字体的 BASE 值，并将该 run 标为度量回退供测试诊断。

dominant run 的选择、Common/Inherited 字符归属和 BASE 缺失时的回退必须确定、可测试。横排只读取 BASE 的 `HorizAxis`；以 dominant run 对应脚本的默认 baseline tag 作为对齐 tag，在其他 run 的实际字体 face/脚本记录中查找同一 tag，并相对该脚本自己的默认 baseline 计算垂直偏移。坐标按该字体 units-per-em 和 run 的实际字号缩放，转换到 Caret 的 y 向下坐标后应用；计算在 run 级完成，不按 glyph 重复解析。字体不提供同一 tag 时使用前述确定回退。BASE 坐标采用字体给出的静态 design-unit 值；依赖轮廓点 hinting、设备像素或 variation delta 才能修正的格式，在当前布局尺度没有可靠解析路径时回退到字体度量，不应用未经调整的近似值。

相同段落基础样式和字号下，Latin、Hani 与混排文字共享同一行距网格；切换段落语言或同字号 fallback 字体不能改变行框高度、选区高度、后续行基线或段落起点。BASE 只调整各脚本字形相对 dominant baseline 的位置，不把 fallback 字体的全局 ascent/descent/leading 或脚本 extent 加到行高；因此中文 ink 可以超出共享行框的下缘，但不额外推开下一行或段落。该规则是 Caret 对齐用户在 Microsoft Word 中观察结果的产品行为；Microsoft 公开资料没有说明 Word 的内部行距网格字体选择算法。显式更大字号或较高的行内对象仍按行距类型参与行高；同字号脚本 fallback 的字体全局度量只用于基线回退和越界诊断。若字形 ink 越出行距网格，记录其范围，但 Natural/Multiple 不得因此暗中改变网格；Exactly 继续按固定行距裁切/溢出规则处理。空段落使用同一基础样式行距网格，不向正文插入测量字符。

Microsoft Word 官方文档说明 Before/After 分别控制段前和段后间距；Multiple 按数值倍乘行距，At least 容纳该行最大字号或图形，Exactly 使用固定 pt 值。官方文档还说明当前 Word 默认行距为 1.15，并允许用户自定义段前、段后数值；它没有规定所有模板统一采用 0/8 pt。Caret 的默认预设独立定义如下，不把模板默认值称作 Word 的普遍默认。BASE dominant run 选择由 Caret 明确定义；BASE 坐标按实际字体与字号缩放。BASE 对齐不能缩小行框、丢弃 ink 范围或掩盖裁剪/溢出。固定行距继续按其固定推进规则处理溢出。

## 行距与默认值

行距是有语义和单位的段落属性，不是单一无类型的 `lineHeight`。文档模型保存整数厘点或明确的比例；布局器在内部换算为 pt。字号不等于行高。段落行距首版支持：

| 类型 | 语义 |
| --- | --- |
| `Natural` | 按基础样式行距网格、明确更大字号的 run 及行内对象的度量计算自然行高；同字号跨脚本 fallback 不改变网格 |
| `Multiple` | 基础样式行距网格乘正比例；明确更大字号的 run 与行内对象仍可提高行框下限 |
| `AtLeast` | 行高不小于明确更大字号/对象的内容需求和指定下限；同字号脚本 fallback 不提高下限 |
| `Exactly` | 行推进保持指定高度；字形或对象允许绘制溢出并按页面/行框裁剪，不偷偷扩成 `AtLeast` |

界面可提供单倍、1.5 倍、双倍、多倍、最小值和固定值入口；持久化值使用上述有类型语义。Natural 的行网格由文档/段落基础样式字体的实际度量，以及明确更大字号/行内对象需求计算；该字体按固定 Latin 样本解析，且不随正文语言与 Script 改变。同字号脚本 fallback 的全局度量与 ink extent 只用于基线回退和越界诊断，不改变行框。Multiple 以基础样式单倍网格乘倍率；明确更大字号或对象需求高于结果时，Caret 按自身产品规则提高行框，脚本 BASE 偏移或同字号 fallback ink 超界不能悄悄改变倍率/网格。AtLeast 至少容纳明确更大字号或图形的行高需求，再应用用户下限；这与 Word 官方文档对 At least 的说明一致，同字号脚本 fallback 不独自提高下限。Exactly 高于内容需求时内容垂直居中，低于内容需求时保持固定推进高度、按基础样式网格定位内容，并允许 ink 溢出后由行框裁切。BASE 脚本偏移不改变行距定义；发生 Exactly 溢出时必须保留溢出状态，不能通过移动字形或错误度量标成未溢出。

新建 `.cdoc` 使用内置默认排版预设：正文行距 1.15 倍、段前 0 pt、段后 8 pt，并启用孤行/寡行控制。段前和段后是独立、可自定义的 pt 数值，不随字号变化。正文默认不启用“与下段同页”“段落不拆分”或“段前分页”；标题样式可单独启用与下段同页。默认预设文件位于 `crates/caret-doc/resources/layout-themes/default.json`，新建文档和缺少全局段落样式的旧文档从该预设取值。文档将实际生效的全局值保存在 `.cdoc` 的 `globalSettings.paragraphLayout`，段落只覆盖不同于全局值的属性。修改应用默认预设不静默改变已有文档。

Caret 的排版默认数值由内置预设定义；OOXML fixture 用于核对数值映射，不作为 `.cdoc` 存储模型或全局默认值来源。不同期刊通过同一模型的独立排版预设提供。LaTeX 没有唯一通用的行距与分页默认值，因此不定义笼统的“LaTeX 默认”。Word 行距说明仅陈述 Microsoft 官方文档明确记载的语义；来源见[Word 段落间距选项](https://support.microsoft.com/en-us/word/adjust-indents-and-spacing)和[Word 默认行距](https://support.microsoft.com/en-us/word/change-the-default-line-spacing-in-word)。

## 段间距与分页

段前/段后间距是独立物理长度。Caret 默认段前 0 pt、段后 8 pt；用户可分别自定义，软换行不引入段间距。相邻段落按当前预设的规则计算净间距，不可只根据字段名推导算法。孤行/寡行、与下段同页、段落不拆分和段前分页是互相独立的段落分页属性。

分页器消费行推进高度、有效段间距、版心高度和显式分页符。跨页段落引用同一段文本与范围，不复制正文。若单行推进高度超过版心，或对象高于整页版心，该行只放置一次并允许溢出，不能为同一行不断创建空页。Exactly 下溢出的 ink 不得悄悄增加分页高度。版心、字体度量、基线对齐、行距、段距和分页规则改变时，失效范围从受影响段落扩展至后续受影响页面。

## 布局阶段与职责

段落仅塑形一次。Parley 的 `Layout` 是 glyph shaping、行切分和双向视觉交互几何的权威来源；Caret 在其结果上计算文档行框、页内偏移和分页。Qt scene graph 复用 Rust 交付的字体实例、glyph ID 和位置绘制，不再将正文交给 Qt 文本引擎单独换行。UTF-8 文档顺序不能直接当作屏幕上的视觉左右顺序。

| 阶段 | 职责 | 不负责 |
| --- | --- | --- |
| `font_selection` | 根据默认字体、CJK、Unicode Script 和 BCP 47 语言偏好生成候选字体 | 扫描字体目录、分析字符、替代 Fontique 字形覆盖查询 |
| `shaping` | 展平段落行内结构，生成语言相关边界并提供 Parley 样式范围 | 自行选字体、塑形 glyph、生成 cluster、断行或分页 |
| `shaping/unicode.rs` | 复用 ICU4X 与 `unicode-segmentation` 生成固定数据版本的断行/字素边界，并应用语言 tailoring | 字体查询或 glyph shaping |
| `paragraph/parley.rs` | 构建并保留 Parley `Layout`；适配字素安全光标、命中、选区和视觉行结果 | 文档持久化、字体目录扫描或分页 |
| `line_metrics` | 消费字体/对象度量与行距语义，计算行框、主基线和溢出 | 重复塑形 |
| `pagination` | 消费段落行、`PageSection` 版心、段距及显式分页，输出页片段和页内偏移 | 复制正文或改写文档树 |

布局快照必须带文档 revision、样式代际、字体目录代际及视口/缩放所需的版本信息；过期的异步结果不得覆盖更新后的布局。缓存按失效范围复用塑形、基线度量和分页结果，不能为每次按键重复扫描字体目录或重塑未受影响段落。具体缓存键、脏范围传播和性能约束见[增量排版](incremental-layout.md)。Unicode grapheme、bidi、断行和脚本基线对齐需由固定 corpus 覆盖，并通过跨脚本、回退字体和字号差异用例验证。
