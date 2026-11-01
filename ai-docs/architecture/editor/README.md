# 编辑器

这些文档定义文档怎么存、命令怎么改它、结构怎么导航。界面只消费这里的结果。

| 主题 | 文档 |
| --- | --- |
| 文档模型入口 | [文档模型](document-model.md) |
| 文字格式与颜色 | [格式语义](text-formatting.md) |
| 字体、shaping 与位置 | [文字塑形](text-shaping.md) |
| 文档字体目标、映射与 fallback（唯一权威） | [多文字体系字体选择](font-selection.md) |
| 各平台脚本/语言与代码字体默认映射 | [平台字体映射目录](font-defaults/README.md) |
| 页面、行距与分页 | [页面与段落排版](page-layout.md) |
| 文档单位及跨层换算 | [文档单位](document-units.md) |
| 注册表与撤销 | [命令](commands.md) |
| `\` 输入 | [命令输入](command-input.md) |
| 结构索引与跳转 | [结构导航](structure-navigation.md) |
| 数学公式与表格 | [专题索引](math-and-table.md)、[公式](math.md)、[表格](tables.md) |
| 外部文档格式 | [文档导入与导出](document-interchange.md) |
| 缓存键 | [增量排版](incremental-layout.md) |

## 文档模型专题

专题文件集中在 [`document-model/`](document-model/)；下表各主题只在对应文件维护，其他文档引用时直接链接到该文件。

| 权威主题 | 文档 |
| --- | --- |
| 文档树与页面角色 | [文档树与结构](document-model/document-structure.md) |
| 行内载体、语义适配层与块环境视图 | [行内载体与环境视图](document-model/inline-and-block-environments.md) |
| Block 到 Paragraph 行内节点的当前代码链路 | [内容节点代码链路](document-model/content-code-path.md) |
| 块环境标题、编号与结构编辑 | [环境模型](document-model/document-environments.md) |
| 位置与模型上限 | [文档位置](document-model/document-position.md) |
| `.cdoc` 文件格式与存储边界 | [文档存储](document-model/document-storage.md) |
