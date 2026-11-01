# 文档模型

更新日期：2026-11-01。本文是文档模型主题的入口索引；详细规则分别在专门文档中维护，避免同一契约出现多份副本。

| 权威主题 | 文档 | 范围 |
| --- | --- | --- |
| 文档树与页面结构 | [文档树与结构](document-model/document-structure.md) | 当前 DTO、块/行内节点、页面角色和实现边界 |
| 行内与块级适配 | [行内与块级环境适配](document-model/inline-and-block-environments.md) | 两层适配结构、各自载体、焦点、操作与几何 |
| 块环境语义 | [块环境模型](document-model/document-environments.md) | 内置/自定义块环境、标题、编号与编辑规则 |
| 位置与限制 | [文档位置与限制](document-model/document-position.md) | Pos、Affinity、位置映射及公式/表格上限 |
| 原生存储 | [`.cdoc` 存储格式](document-model/document-storage.md) | 文件信封、版本、能力登记、未知字段、媒体和原子写入 |
| 文字格式 | [文字格式语义](text-formatting.md) | 格式解析、环境模板和直接属性 |
| 字体选择 | [多文字体系字体选择](font-selection.md) | 文档字体字段、候选解析、语言与回退 |
| 结构导航 | [结构导航](structure-navigation.md) | 结构索引、导航行为和布局几何投影 |

任务文档只记录实现进度与验收，不复制上述长期契约。架构文档和规范如需引用模型细节，应直接链接到对应权威主题。
