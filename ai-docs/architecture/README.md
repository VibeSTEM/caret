# 架构文档

更新日期：2026-11-09。这里写产品边界和权威数据。版本、编码和移植范围在[技术规范](../standards/README.md)。实现状态只记在任务里。各架构专题文件标明各自唯一负责的契约；任务不得另立并行规则。具体实现状态见[任务索引](../task-index.md)。

Caret 是轻量的所见即所得结构化编辑器。用户看到的就是排好的结果。标题、列表、公式和表格可以用键盘进入、离开和跳转。命令可以用 `\` 直接输入。

## 文档

| 主题 | 文档 |
| --- | --- |
| 需求与里程碑 | [需求](requirements.md) |
| 运行时与数据流 | [运行时](core/runtime.md) |
| 仓库怎么放 | [仓库结构](core/repository-layout.md) |
| 编辑器自己的技术选择 | [技术选择](core/tech-stack.md) |
| 文档、命令、结构、公式、排版 | [编辑器](editor/README.md) |
| 窗口、画布、快捷键 | [界面](interface/README.md) |

## 一条编辑怎么走

```text
按键或 QML 意图
    → C++ 输入 / ViewModel
    → Session::run(命令 id, 参数)
        → 改 Document，或只改叠加层
        → 按变更类别使布局失效；仅结构语义变化时重建派生结构索引
    → 把页面快照交给 C++ 画布
    → QML 只绑定光标、选区、补全和状态栏
```

`Document` 是唯一权威数据。布局、结构索引和屏幕上的高亮框都是派生的。补全查询和当前环境高亮不写进文档。失效边界见[增量排版](editor/incremental-layout.md)和[结构导航](editor/structure-navigation.md)。

编辑宿主与内容层级由[文档树与结构](editor/document-model/document-structure.md)权威定义：Block 是统一宿主类别，Paragraph 是结构最简单的 Block 变体；Inline 是 Paragraph 与显式支持行内流的 Block 内部内容。共享 Block 交互协议见[块宿主交互](editor/document-model/block-interaction.md)，环境候选和载体视图见[环境视图](editor/document-model/environment-views.md)，位置/导航投影由[文档位置](editor/document-model/document-position.md)和[结构导航](editor/structure-navigation.md)定义。Table、Figure 等尚未实现的宿主只记录扩展契约，不代表已有可编辑实现。

分层的依赖方向见[分层规范](../standards/layering.md)。编辑器 crate 的成员以[仓库结构](core/repository-layout.md)为准。
