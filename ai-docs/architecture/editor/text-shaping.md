# 文字塑形与位置

更新日期：2026-10-26。文字运行和格式来源见[格式语义](text-formatting.md)；文档全局字体选择见[多文字体系字体选择](font-selection.md)。文本分析、字体目录与 shaping 由 Rust `caret-layout` 管理，Qt 只暴露平台默认文本字体等宿主信息。

## 跨平台与多语言契约

Windows、macOS 和 Linux 共用同一 Unicode 数据版本及字素、双向和断行规则；版本在构建配置中集中锁定，升级必须显式进行，并运行相同的 conformance corpus。不能让光标边界、删除单位或行断机会因操作系统 API 默认值而变化。规范依据：[UAX #29 文本分段](https://www.unicode.org/reports/tr29/)、[UAX #9 双向算法](https://www.unicode.org/reports/tr9/) 和 [UAX #14 行断算法](https://www.unicode.org/reports/tr14/)。光标与退格/删除使用 UAX #29 extended grapheme cluster 边界；RTL 重排在段落内进行；断行先按 UAX #14 及 ICU4X locale tailoring 确定语言范围内的允许断点，再由段落排版器选取实际断点。M0 首先验证英语默认规则和中文横排禁则；CLREQ 的采用范围见[调查记录](../../research/006-cjk-and-academic-layout.md)。语言按 Run → Paragraph → 文档默认 BCP 47 标签回退；不按平台或未经确认的自动检测猜测。

一致性目标是相同的文档内容、编辑操作和 Unicode 边界行为，不承诺系统字体不同情况下逐像素相同或完全相同的分页。测试几何时使用固定测试字体；真实默认字体按平台和脚本回退。请求字体名和实际匹配字体分开保存/报告。

## 塑形流程

实现流程：Parley 0.12 高层 `Layout` 是段落分析、双向重排、字体回退、HarfRust shaping、换行、glyph/cluster 映射以及光标/命中/选区几何的唯一实现；Caret 不另建第二套通用塑形或行布局引擎。`FontCatalog` 持有的 `FontContext` 同时供可复用 `LayoutContext` 使用，字体集合和 source cache 不重复扫描或维护。`fontique` 0.12 通过 CoreText、DirectWrite、Fontconfig 等平台后端发现字体并匹配覆盖。Caret 用 `icu_segmenter` 2.3.0 的 Unicode 17 UAX #14 按 BCP 47 语言范围先计算允许断点，再通过 Parley line-break override 限定最终可断位置；覆盖位集按非强制换行的码点对顺序消费，并校验回调数量。M0 启用英语默认规则与中文 `zh` locale tailoring，混合语言 Run 按自身语言替换对应范围内断点；`zh` tailoring 不等于完整 CLREQ 支持。字素边界由 `unicode-segmentation` 1.13.3 在串接后的段落逻辑文本上计算，Run 与行内环境边界不重置分段。依赖精确版本和数据均由 Cargo.lock 固定，Windows、macOS、Linux 运行同一份 Unicode 17 UCD grapheme、bidi-character 和 line-break corpus。

文档排版器传入已按格式优先级解析的 `ShapingStyle`；`caret-layout` 再应用 Paragraph/Run 语言与嵌套 Font/Bold/Italic 环境。有效样式相同的相邻 Run 会合并为 Parley 样式范围，避免源节点边界无故改变塑形；若一个扩展字素簇跨越不同样式 Run，格式仍按各自字节范围传入 Parley，而 Caret 光标与选区 API 只允许完整字素边界。全局字体族按[多文字体系字体选择](font-selection.md)解析成脚本/语言上下文候选；候选以 Parley `FontFamily` 列表传入，高层布局和 Fontique 覆盖查询决定实际族；emoji cluster 将 Emoji generic family 放在候选前。请求候选缺失时保留文档请求，并由布局快照报告优先请求不可用，不回写文档。系统及用户字体列表在 FontCatalog 创建或显式 refresh 时读取，字体 blob 与引擎状态缓存，不在编辑热路径扫描磁盘。Windows 额外扫描系统 Fonts 与当前用户 Fonts 标准目录，以绕过 Fontique 0.12 对 DirectWrite 缓存集合不检查更新的问题；不承诺检测非标准路径注册及删除项，待 Windows 平台 smoke 复核。

`ParagraphLayouter` 保留完整 Parley `Layout`，分页只消费同一 Layout 导出的 UTF-8 行范围、推进宽度及 Caret 行框；不会把已塑形文本拆成子串后重新分析。Caret 的自然/倍数/最小/固定行距仍由 `calculate_line_metrics` 决定，并将行顶端与 baseline 调整量映射回 Parley 的行内绘制、光标和选区几何。Exactly 的绘制裁切与复杂行内对象仍须由对应绘制层消费 Caret 行框元数据；未支持的文档对象由文档布局服务显式拒绝，不能静默丢失。Parley 的公开高层 API 是 shaping/layout 单一来源；低层 `parley_engine` 只作为 Parley 的传递依赖，Caret 不直接维护第二份 cluster 布局管线。

Latin、East Asian 和 Complex Script 可以有不同字体。实际字体选择必须考虑系统可用性和许可证；不随应用分发未获许可的字体。请求字体、实际字体文件/实例、合成字重和语言方向都可能影响度量和缓存键。

字体目录、字体目标映射、请求族与实际匹配族、Font EnvironmentNode 以及字体 fallback 的权威规则见[多文字体系字体选择](font-selection.md)。本节只要求 shaping 使用 Rust `caret-layout` 提供的候选和字体实例；不能因本地缺字而改写文档保存的请求字体。

Glyph run 是排版结果，不取代文档中的文字和格式片段。布局需保留 cluster 到文本位置的映射：多个码点可能组成一个字素或 glyph，一个连字也可能覆盖多个字符。需处理组合附加符、emoji ZWJ、阿拉伯连接、RTL/LTR 混排和中文标点禁则。Unicode 17 官方 grapheme、bidi-character、line-break 测试集位于 [`tests/fixtures/unicode-17`](../../../tests/fixtures/unicode-17/)；规范断行将复杂上下文脚本的词典切分作为显式语言 tailoring。

替代字体的度量可能改变换行和页数；不能只凭字体名称映射承诺与 Word 分页一致。

## 位置契约

文档存储位置、用户可见光标移动边界、shaping cluster 和 glyph index 是不同概念。每个 Paragraph 建立逻辑文本视图，将 Run 按文档顺序串接；行内格式环境对字素分段透明，因此 Run、Bold/Italic/Font 环境边界都不能制造额外光标停靠点。行内对象是原子边界，不能进入对象内部。内部位置用 UTF-8 字节偏移，但方向键、删除、选区扩展和鼠标命中按同一 Unicode 版本的 extended grapheme cluster 边界移动；普通格式操作的非空选区按逻辑起止边界向外扩展到完整字素簇，保留锚点/焦点方向。词典断行仅替换其有效 Run 语言范围内的默认断点，并将词典新增断点约束到完整字素边界；范围外仍提供完整 UAX #14 机会，实际行布局还须避免在字素簇中断行。任何转换都经过同一份逻辑文本到节点、cluster 和 glyph 的映射。位置不得落在无效 UTF-8 边界或字素簇中间；外部 API 提交非法偏移时拒绝，不静默猜测。

环境边界亲和性使用逻辑文档顺序，决定插入时继承的环境，不表示屏幕左/右。视觉 caret affinity 是独立的布局状态，用于 bidi 混排或换行处同一逻辑位置的视觉侧；方向键先依视觉布局确定目标，再映射回逻辑 Pos，不能直接增减字节偏移。

命中测试顺序：找到页面矩形 → 把屏幕坐标逆变换到页面物理单位 → 找行与视觉片段 → 根据 bidi 与 cluster 映射回文档位置。光标、选区、文字绘制引用同一版本的布局结果。

## 显示坐标

文档几何和字号以 pt 保存，界面按 pt 输入和显示。排版 API 用 `u16` 百分之一 pt（`centipoints`）传递字号，50 表示 0.5 pt；API 边界再转换为 Parley 使用的浮点 pt，便于低层样式紧凑且支持 0.01 pt 精度。Parley 尺度沿用文档 pt，输出度量也以 pt 表示；视图边界统一转换为 logical px，系数为 `96 / 72`。坐标为 `screenLogical = pageOrigin + documentPt * (96 / 72) * zoom - scrollLogical`，再由设备像素比转换为物理像素。该约定确保 zoom 不改文档字号，也不使 DPR 参与换行。

如果鼠标事件已是 Qt logical pixels，不再除 DPR。缩放应以鼠标点或视口中心为锚，调整滚动位置以保持文档锚点稳定。页面 origin 是滚动前内容坐标时才减 scroll，不能把视口坐标再减一次。

以上是候选坐标契约，最终数值范围、锚点策略和缓存键由 020 的设计决策与相应实现任务确认。
