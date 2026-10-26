# 调查索引

外部项目的阅读笔记。只记录可借鉴的行为，不表示要把它的源码或依赖拿进仓库。报告按主题顺序连续编号；删除报告的编号由新的研究报告接续，不保留空号。

界面和工程基础设施采用 panta 的 Qt / QML 与 Rust 栈，见[技术规范索引](../standards/README.md)。

| 编号 | 报告 | 用法 |
| --- | --- | --- |
| 001 | [WordCraft](001-wordcraft.md)（补充见 [004](004-wordcraft-editor-design-notes.md)） | 借鉴文档模型、命令和排版 |
| 002 | [craft-libs](002-craft-libs.md) | 对照少量文件与界面行为 |
| 003 | [TeXmacs](003-texmacs-environments.md) | 对照结构环境、宏定义与样式定制；只借鉴原则，不复制宏语言 |
| 004 | [WordCraft 编辑器方案参考笔记](004-wordcraft-editor-design-notes.md) | 补充记录 Paragraph/Run、版权边界和 Caret 设计结论 |
| 005 | [Rust 文档与排版任务的库审查](005-rust-library-audit.md) | 记录 021 和 022–027 的 Rust 库选型及验证边界 |
| 006 | [CJK 排版规则与学术论文样式](006-cjk-and-academic-layout.md) | 区分 Unicode 断行、CLREQ 中文需求和期刊模板差异 |
| 007 | [多文字体系字体选择器](007-font-selection.md) | 设计脚本、语言变体、数学字体与 fallback 的分层 |
| 008 | [Office Open XML 行距、段距与分页](008-office-open-xml-spacing-pagination.md) | 对照 ECMA-376 与 Word 公开语义，识别 Caret M0 的排版差异 |
