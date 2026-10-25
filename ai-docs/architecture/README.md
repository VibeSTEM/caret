# 架构文档

更新日期：2026-10-25。这里写产品边界和权威数据。版本、编码和移植范围在[技术规范](../standards/README.md)。实现状态以后记在任务里。基础设施 crate 已存在；这里涉及的编辑器文档树、命令引擎和排版能力仍是规划，具体现状以对应任务为准。

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
        → 按段落 rev 更新布局和结构索引
    → 把页面快照交给 C++ 画布
    → QML 只绑定光标、选区、补全和状态栏
```

`Document` 是唯一权威数据。布局、结构索引和屏幕上的框都是派生的。补全查询和焦点框不写进文档。

分层的依赖方向见[分层规范](../standards/layering.md)。编辑器 crate 的成员以[仓库结构](core/repository-layout.md)为准。
