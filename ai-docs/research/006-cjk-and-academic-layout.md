# CJK 排版规则与学术论文样式

调查日期：2026-10-26。此文区分文本断行算法、中文排版要求和期刊提交样式；调查结论不自动成为产品承诺。

## 中文排版

[Unicode UAX #14](https://www.unicode.org/reports/tr14/) 是跨平台行断机会的基线。语言/地区 tailoring 可以收紧或补充可选断点，但最终断点还必须落在完整字素和 shaping 安全边界上。

[W3C 中文排版需求（CLREQ）](https://www.w3.org/TR/clreq/) 覆盖中文特有的行首/行尾禁则、标点悬挂、行内调整、标点宽度调整和中西文混排。当前发布标注为 2026-10-07 的 W3C Group Note Draft；文档本身说明它不是 W3C 或其成员背书的标准。因此可把它作为中文需求参考，并把选中的规则写成 Caret 自己的可测试契约，不应宣称完整实现了 CLREQ。

CLREQ 指出中文排版存在区域规则差异，并建议按地区而不是仅按繁简体区分。M0 的第一目标为横排简体中文常见场景，使用文档的 BCP 47 语言/区域信息选择 profile；繁体、台湾/香港地区规则、直排、标点悬挂与压缩、两端对齐细节逐项支持并测试后才能对外宣称。

实现分工：ICU4X 根据 Unicode 17、UAX #14 与可用 locale tailoring 产生基础机会；`caret-layout` 再应用 Caret 明确采纳的中文禁则并保证安全边界。不能假设 `zh` locale 本身实现了 CLREQ 的所有版面规则。

## 英文科学论文样式

用户提供的[SCI 字体与排版指南](https://www.rbsci.com/k/sci-ying-wen-lun-wen-yi-ban-yong-ji-hao-zi-ti.html)列举了 12 pt、Times New Roman、1.5/2 倍行距等常见建议，也提醒要以目标期刊作者指南为准。该页面为 2023 年的二手通用指南，未给“多数期刊”统计的可核验来源，不应作为 Caret 内置默认值或格式标准。

官方要求本身有差异：[Nature 格式指南](https://www.nature.com/nature/for-authors/formatting-guide)偏好 12 pt Times New Roman、双倍行距；[IEEE Access 投稿指南](https://ieeeaccess.ieee.org/authors/submission-guidelines/)要求双栏、单倍行距和指定模板；[Elsevier 投稿格式说明](https://service.elsevier.com/app/answers/detail/a_id/5955/supporthub/publishing/~/is-there-a-template-available-for-my-manuscript-file%3F/)说明特定期刊可能要求模板，应查目标期刊的 Guide for Authors。

因此学术论文格式应设计成可选择、可更新的样式/模板 profile，而不是英语语言的默认行为。可组合命名段落/字符样式与页面属性；具体期刊若要求双栏、连续行号、标题页、脚注或参考文献格式，还需相应文档语义与导出能力，不能仅靠字体和段距模拟。M0 不据此加入投稿检查、模板下载或 Word/DOCX 兼容承诺。
