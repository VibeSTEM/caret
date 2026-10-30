# Office Open XML 行距、段距与分页规则调查

调查日期：2026-10-26。本文对照 Microsoft Word 公开帮助与 Office Open XML 标准，为任务 008 的 M0 文本编辑集成提供依据，并评估其依赖的 Caret 026 行距、段间距和分页语义。它是外部标准调查，不自动改写已确认的产品规则；需要采用的差异应回写到架构/任务规范并补命令级测试。

## 结论

Caret 将 Word 官方文档公开的行距语义与 ECMA-376 文件格式语义作为参考，新文档数值由 Caret 自有 `.cdoc` 默认预设管理。要达到可验证的 Word 数值/效果一致性，Exactly 定位和相邻段距仍需按 ECMA-376 明确语义实现并通过 Word/OOXML fixture 对照；布局结果也受字体度量和页面几何影响。期刊模板以后在 Caret 模型上覆盖这些默认值；不定义一个试图代表所有 LaTeX 文档类的通用配置。

## OOXML 的明确规则

ECMA-376 是 Office Open XML 的**文件格式及其元素语义规范**：它规定 WordprocessingML 如何存储段落属性，也规定消费者如何解释 `w:spacing` 等属性。它不是一套普适的论文排版指南，也不规定“所有论文都用某个行距/段距”；更不保证不同排版应用凭同一文件一定算出逐像素相同的整页结果。论文的具体数值通常来自期刊/会议模板或作者指南，再由 DOCX/LaTeX 模板分别表达。

WordprocessingML 在 ECMA-376 Part 1 §17.3.1.33 (`w:spacing`) 中定义段落行距、段前与段后：

- `lineRule="auto"` 时，`line` 以 1/240 单倍行距为单位：240 表示 1.0，360 表示 1.5，480 表示 2.0；例如 276 表示 1.15 倍。
- `lineRule="atLeast"` 或 `"exact"` 时，`line` 以 twentieths of a point（1/20 pt，也就是 twip）表示：240 表示 12 pt，500 表示 25 pt。
- 固定长度的 `before` / `after` 段前/段后值也使用 1/20 pt；另有 `beforeLines` / `afterLines` 和自动段距属性，因此严格互操作时需要保留单位种类，不能把所有值都当作 pt 数字。
- 固定高度大于内容需求时，文本在可用空间垂直居中；固定高度小于内容时，文本靠行框底部放置，超出的上部被裁切。
- 相邻段落间距按两段的 inter-line spacing、前段段后距和后段段前距的最大值确定。将标准里的 inter-line spacing/net spacing 映射到 Caret 的 line box advance、baseline 距离与段落间净空白，需要先统一度量口径，再用 Word/OOXML fixture 核对；不能只按变量名称推断是否额外累加。

Microsoft Word 官方帮助列出 Single、1.5、Double、At least、Exactly 和 Multiple。At least 需要容纳该行最大字体或图形；Exactly 是固定点值；Multiple 按数值倍乘行距。Microsoft 当前支持页说明 Word 默认行距为 1.15，段落默认后接空白；页面允许用户修改段前、段后数值，但没有指定统一的默认 pt 数值。这里不从非官方来源推断 Normal 模板的具体段距。

Word 的分页选项至少包括 Keep lines together、Keep with next、Page break before 和 Widow/Orphan control；官方帮助说明 Widow/Orphan control 默认开启。它们是段落级分页约束，不应与显式分页符混为一谈。

## LaTeX 与 OOXML 的关系

LaTeX **不符合也不实现 OOXML**。LaTeX 文档使用自己的源语言、宏包和文档类；例如 `\documentclass` 选择文档类，出版社/期刊可以提供自己的 `.cls`、`.sty` 与 `.tex` 模板。编译后常见交付物是 PDF。它和 OOXML 表达的文档语义有重合（段落、标题、字号、分页等），但数据模型、样式继承、排版算法和扩展机制不同，不能把 `w:spacing` 等价地套到 LaTeX 上。

论文模板的要求通常来自期刊/会议的作者指南；同一出版方可能提供 Word/DOCX 模板，也提供 LaTeX 文档类。DOCX 模板走 OOXML；LaTeX 模板走对应 class/package。二者可能要求相近的最终外观，但并不因此遵循同一文件格式标准。

### 排版语义对照

| 主题 | Word / OOXML | LaTeX / TeX | 是否可直接视为同一规则 |
| --- | --- | --- | --- |
| 行距 | Single、Multiple、AtLeast、Exactly；auto multiplier 以 1/240 单位存储，固定规则以 pt 单位存储 | 主要以 baseline-to-baseline 的 `\baselineskip` 控制，`\baselinestretch`/`setspace` 按文档类和字号调整 | 目标相似，但“单倍”的基准和行框定位方式不保证相同；转换需按值和字体度量核对 |
| 段间距 | 段前/段后属性，OOXML 规定相邻段的间距合成方式 | `\parskip` 等垂直 glue，通常可带 stretch/shrink，文档类与宏包可改写 | 都能设置段落间空白，但不能假设相邻间距合并规则相同 |
| 分页 | 段落级 keep-lines、keep-next、page-break-before、widow control 等显式属性 | TeX 页面构造器在可断点上综合 glue 与 penalties；标准类还设置 club/widow 等 penalty，模板可再覆盖 | 都能表达防孤行、保持标题和正文、强制分页等意图，但控制模型与断页优化不一一对应 |

因此，“基本差不多”适用于**排版目标和用户概念**，不适用于每个参数的精确数值解释、基线位置或分页结果。尤其 LaTeX 类文件可以重定义 spacing/penalty；支持任意 `.cls` 并不等于 Caret 能自动恢复其规则。

对 Caret 的建议是把 ECMA-376 作为 **DOCX 互操作语义目标**，而不是内部树模型或全部排版引擎的唯一标准。未来可在 `caret-doc` 与 OOXML 之间建立显式映射/导入导出层，并分别为 `.docx` 与 LaTeX 模板定义能力范围。导出 PDF 可以面向固定模板外观；这不等于生成 LaTeX，也不代表和 Word/LaTeX 引擎逐像素一致。

## 与 Caret 026 当前实现对照

Caret 现在**没有以 OOXML 作为存储格式或直接扩展 OOXML schema**：`.cdoc` 使用自己的文档结构。排版模型借鉴了相近的语义类型，但数值单位不同：`LineSpacing::AtLeast/Exactly` 用百分之一 pt，Caret 的分页输入以 pt 表示；OOXML 固定长度用 1/20 pt、倍数用 1/240 单位。比如 12 pt 在 Caret fixed-length 值中是 1200 centipoints，在 OOXML 中是 240 twips。内部精度更细并不自动代表排版规则相同；如果将来支持 DOCX，需要适配层做显式单位换算和语义映射。

| 项目 | ECMA-376 / Word | Caret 当前规则 | 差异与建议 |
| --- | --- | --- | --- |
| 行距模型 | auto/atLeast/exact 及线性倍数 | Natural/Multiple/AtLeast/Exactly，固定长度用整数百分之一 pt | 可保留内部整数模型；若需 OOXML 互操作，在边界转换单位。明确 Multiple 的参考“单倍行高”定义。 |
| AtLeast | 最小行距，需适配最大字体或图形 | 不小于字体/对象组合度量 | 语义基本一致。 |
| Exactly 高于内容高度 | 内容垂直居中 | 内容居中 | 一致。 |
| Exactly 低于内容高度 | 行框底部对齐，顶部裁切 | baseline 按行框高度减最大 descent 定位，使内容底边对齐；行内溢出范围由行框提供给绘制裁切 | 基线规则已接近；待渲染器接入时验证裁切范围。 |
| 相邻段距 | 取两段的 inter-line spacing、前段 after、后段 before 中最大值 | 每行按 line box advance 推进，再额外增加 max(after, before) | **需 fixture 核对**。先明确标准 inter-line spacing 与 Caret 行框推进、基线距离、净空白之间的对应关系；在同一测量口径下比较，不从字段名直接断言重复累加或行为不一致。 |
| 新页首段 | OOXML 还受段距属性、分页规则及 Word 兼容行为影响；不能据 UI 摘要推断所有场景都清零 | 忽略新页首段 before | 属于 Caret 明确简化规则；需要用 Word fixture 验证目标行为后再决定是否保留。 |
| 默认样式 | Microsoft 支持文档说明默认行距为 1.15，段后有空白；未指定统一的段前/段后 pt 数值 | Caret 默认 1.15 倍、段前 0 pt、段后 8 pt | Caret 数值是产品预设，不是 ECMA-376 强制值或 Microsoft 规定的所有模板默认；保存于 `.cdoc` 并允许期刊模板覆盖。 |
| 分页约束 | 可设置 widow/orphan、keep lines、keep with next、page break before | 分页器消费这些独立段落标记，并对无法满足的孤行约束放宽后按页高推进 | 已有 M0 规则实现；仍需 fixture 对照具体兼容行为，不宣称完整 Word 分页兼容。 |

## 建议实施边界

1. 将 OOXML 的 `w:spacing` 规则作为 Caret 行距/段距的互操作基线，而不是承诺 DOCX 整页逐像素一致。字体、shaping、分页例外仍会造成差异。
2. Exactly 短行框采用底对齐、顶部裁切；基线计算已有测试，渲染器接入时仍需确认按行框裁切。
3. 为段落间距建立 Word/OOXML 对照 fixture，覆盖两段行距不同、仅设置前段 after、仅设置后段 before、两者均设置及跨页等情形。先分别记录 line box advance、baseline 距离和段落间净空白，再按标准解释结果；确认语义映射后才调整分页公式。
4. 新建 `.cdoc` 使用 Caret 数值配置（1.15 倍、段前 0 pt、段后 8 pt、孤行/寡行控制开启），配置保存在独立的 `crates/caret-doc/resources/layout-themes/default.json`；已保存文档会保存完整配置快照。段距数字是产品设置，不宣称为 Word 通用默认。
5. 将 widow/orphan、keep-with-next、keep-lines-together、page-break-before 设计成独立段落分页约束，避免把它们编码成额外空段或硬分页符。
6. 中文断行仍由 Unicode UAX #14 与 CLREQ tailoring 管理；OOXML 行距/分页标准不替代 CJK 断行规则。
7. 不定义通用 LaTeX 排版配置；后续期刊模板按其明确文档类、字号、字体和分页规则映射到 Caret 的自有样式模型。

## 公开资料

Caret 仅把公开资料用作数值和语义参考；实现使用 Caret 自有的 `.cdoc` 字段、Rust 类型和布局代码，不导入产品代码或图标，也不实现 OOXML 存储。产品代码、资源和配置文件使用 Caret 自有命名；第三方产品名只出现在本研究的出处说明中。此工程边界不能替代对特定司法辖区版权、专利或商标问题的法律审查。

- [Ecma International：ECMA-376 Office Open XML](https://ecma-international.org/publications-and-standards/standards/ecma-376/)：官方标准目录与 Part 1 下载入口。
- [Microsoft Learn：SpacingBetweenLines.Line](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.spacingbetweenlines.line)：引用 ECMA-376 对 auto、atLeast、exact 的单位与垂直定位规则。
- [Microsoft Support：Adjust indents and spacing](https://support.microsoft.com/en-us/word/adjust-indents-and-spacing)：Word 用户界面的行距、段前/段后选项说明。
- [Microsoft Support：Change the default line spacing in Word](https://support.microsoft.com/en-us/word/change-the-default-line-spacing-in-word)：Word 当前默认行距与段落空白说明。
- [Microsoft Support：Use Line and Page Breaks settings in Word](https://support.microsoft.com/en-us/word/line-and-page-breaks)：段落分页控制及 widow/orphan 默认开启说明。
- [Microsoft Learn：Office implementation information for ECMA-376](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oe376)：Office 对 OOXML 的实现差异资料入口。
- [LaTeX Project：Standard Document Classes](https://www.latex-project.org/help/documentation/classes.pdf)：LaTeX 文档类、选项和样式接口。
- [LaTeX2e Reference Manual：baseline skip 与 line spread](https://tug.ctan.org/info/latex2e-help-texinfo/latex2e.html)：`\baselineskip`、`\baselinestretch`、字号以及行距说明。
- [CTAN：setspace](https://ctan.org/pkg/setspace-?lang=en)：单倍、1.5 倍、双倍与自定义行距命令。
- [CTAN：ASME journal LaTeX template](https://ctan.org/tex-archive/macros/latex/contrib/asmejour)：期刊样式通过独立 `.cls` 和 `.tex` 模板提供的示例。
