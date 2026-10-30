# Office Open XML 行距、段距与分页语义

调查日期：2026-10-30。本文以 ECMA-376 / ISO/IEC 29500 的 WordprocessingML 规范语义为主要依据，整理段落行距、段间距和分页属性。Microsoft Word 帮助资料仅用于补充说明 Word 产品界面与默认模板行为；它不替代标准，也不代表所有文档的统一默认值。

Caret 使用 `.cdoc` 作为原生格式。本文用于确定与 Office Open XML 交换时的语义映射和建立兼容性测试，不要求 `.cdoc` 复制 OOXML 的 XML 结构，也不把 OOXML 的序列化单位当作 Caret 内部存储单位。产品排版规则见[页面与段落排版](../architecture/editor/page-layout.md)。

## 标准范围与解释边界

[ECMA-376](https://ecma-international.org/publications-and-standards/standards/ecma-376/) 定义 Office Open XML 的词汇、文档表示、打包方式，以及生产者和消费者的要求；本文采用 Part 1《Fundamentals and Markup Language Reference》第 5 版（2016 年 12 月）的 WordprocessingML 段落属性，对应 ISO/IEC 29500-1:2016。本文不讨论包格式、完整 `.docx` 导入导出或全部兼容性扩展。

WordprocessingML 为行距、段距和分页控制定义了可交换的属性及其单位/语义。它不是一个规定所有排版器必须产生相同像素、相同字体度量或完全相同分页结果的渲染算法。需要逐项解释 Word 的实际界面或默认模板时，另查 Microsoft Word 官方产品资料，并把产品行为与标准要求分开记录。

## 段落行距：`w:spacing`

段落属性 `w:spacing` 可用 `w:line` 和 `w:lineRule` 描述行距。行距数值的解释取决于规则：

| `w:lineRule` | `w:line` 的单位 | 规范语义 |
| --- | --- | --- |
| `auto` | 单倍行距的 1/240 | 自动行距比例；例如 `w:line="276"` 表示 276/240，即 1.15 倍 |
| `atLeast` | twips（1/20 pt） | 行高至少为指定值，内容需要更多空间时可增大 |
| `exact` | twips（1/20 pt） | 行高固定为指定值；内容超出时按规范描述裁切/定位 |

`auto`、`atLeast`、`exact` 分别对应 Word 界面常见的 Multiple、At least、Exactly 语义。界面名称与文档格式属性不是同一层概念，映射时应保留模式和数值，不能只保存换算后的一个行高。

示例：

```xml
<w:pPr>
  <w:spacing w:line="276" w:lineRule="auto" />
</w:pPr>
```

这表示 1.15 倍单倍行距。`atLeast`/`exact` 下，数值以 twips 表示；XML 格式使用 twips 不意味着 Caret 内部也必须用 twips。

## 段前与段后间距：`w:before`、`w:after`

`w:spacing` 的 `w:before`、`w:after` 以 twips 表示段前、段后间距。WordprocessingML 还定义了按行比例表达的 `w:beforeLines`、`w:afterLines`，以及自动间距属性；实现映射时必须保留原属性语义，不能把行单位、自动值和固定物理长度混为一个数值。

段落间距属性可由段落样式层级继承，也可在段落属性中覆盖。`w:contextualSpacing` 还能影响相邻同样式段落的段前/段后空白。因此，仅读取一个段落的 `before`/`after` 字段不足以总是推导最终可见间距；交换测试需要覆盖样式继承与上下文间距。

## 分页控制

WordprocessingML 用独立的段落属性表达分页约束：

| 属性 | 语义 |
| --- | --- |
| `w:keepNext` | 尽可能让本段与后续段落处在同一页；可链式组合 |
| `w:keepLines` | 尽可能让本段所有行保持在同一页 |
| `w:pageBreakBefore` | 在本段之前开始新页 |
| `w:widowControl` | 避免段落首行或末行单独留在相邻页面 |

这些属性是不同约束，映射时不应折叠成单一“保持段落”布尔值。标准允许属性从样式层级继承；缺省时也可能有规范定义的默认行为，例如未显式设置 `widowControl` 时消费者仍须遵守其默认规则。分页器需根据整组约束和可用页面空间求解，不能仅以单个属性机械切页。

## 默认值与标准不保证的内容

ECMA-376 定义格式属性、单位和文档中的样式/默认属性表达方式；Word 模板、应用版本和用户设置决定具体默认排版配置。Microsoft Word 当前帮助页提到其产品默认行距和段落间距行为，但这不是 ECMA-376 对所有文档规定的固定 pt 默认值。Caret 的默认行距和段距由自己的 layout theme 明确给出，不从 Word UI 推导，也不宣称等同于任意 Word 模板。

标准语义兼容不等于跨排版器逐像素、逐页一致。字体文件、字体回退、字形塑形、兼容扩展及分页决策都可能影响最终视觉结果。Caret 只有在建立并验证具体 OOXML fixture 后，才对该用例声明映射兼容。

## 验证策略

- 用最小 OOXML fixture 覆盖 `auto`、`atLeast`、`exact` 的单位转换和边界语义。
- 覆盖 `before`/`after`、`beforeLines`/`afterLines`、自动间距、样式继承和 `contextualSpacing`。
- 分别验证 `keepNext`、`keepLines`、`pageBreakBefore`、`widowControl`，再验证组合约束。
- 对照目标版本 ECMA-376 / ISO/IEC 29500 文本记录判定；Microsoft Word 的实际打开结果作为实现互操作测试，不替代规范依据。
- `.cdoc` 内部单位、默认值及布局决策仍由 Caret 架构文档定义；不得因 OOXML 使用 twips 就把单位转换散落到布局代码。

## 来源

- [ECMA-376: Office Open XML file formats](https://ecma-international.org/publications-and-standards/standards/ecma-376/)：标准版本、各部分范围及 ISO/IEC 29500 对应关系；WordprocessingML 规范以 Part 1 为主。
- [Microsoft Learn: `w:line` / `w:spacing` line attribute](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.spacingbetweenlines.line?view=openxml-3.0.1)：引用 ECMA-376 对 `auto`、`atLeast`、`exact`、单位和文本定位的说明。
- [Microsoft Learn: `w:spacing` properties](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.spacing?view=openxml-3.0.1)、[`w:beforeLines`](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.spacingbetweenlines.beforelines?view=openxml-3.0.1)、[`w:contextualSpacing`](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.contextualspacing?view=openxml-3.0.1)：引用 ECMA-376 对段距单位、按行计量和同样式段间距的说明。
- [Microsoft Learn: `w:keepNext`](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.keepnext?view=openxml-3.0.1)：引用 ISO/IEC 29500 对保持后续段落及链式约束的说明。
- [Microsoft Learn: `w:keepLines`](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.paragraphproperties.keeplines?view=openxml-3.0.1)、[`w:pageBreakBefore`](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.paragraphproperties.pagebreakbefore?view=openxml-3.0.1)、[`w:widowControl`](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.widowcontrol?view=openxml-3.0.1)：WordprocessingML 分页属性定义及其默认语义。
- [Microsoft Support: Adjust indents and spacing](https://support.microsoft.com/en-us/word/adjust-indents-and-spacing)、[Change the default line spacing in Word](https://support.microsoft.com/en-us/word/change-the-default-line-spacing-in-word)：仅作 Word 产品界面和产品默认行为的补充参考。
