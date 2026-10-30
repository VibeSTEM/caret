# 技术选择

更新日期：2026-10-30。语言、Qt、构建和工具版本见[技术基线](../../standards/baseline.md)。这里只记编辑器自己的选择。

| 选择 | 决定 |
| --- | --- |
| 界面 | Qt 6 / QML。画布是 C++ 的绘制面，不是每帧回 Rust 的控件树 |
| 桥 | CXX。签名见 [CXX](../../standards/cxx.md) |
| 文档模型 | Paragraph/Run 表示段落与文字；行内或块级 EnvironmentNode 表达格式作用域和嵌套结构，由 [020](../../task/020-document-model-decision.md)细化。公式作为语义树，而非一段带标记的 LaTeX |
| 原生文件 | UTF-8 JSON，扩展名 `.cdoc`，带格式版本。见[`.cdoc` 存储格式](../editor/document-model/document-storage.md) |
| 命令 | Rust 里的注册表。界面只提交 id 和参数 |
| 主题和界面文案 | `.pa`，运行时用快照和 QM。见 [DSL](../../standards/dsl.md) |
| 键位 | 语义借鉴 Mogan，绑定由 Caret 自己写。Mogan 是 GPL，不复制它的代码和键表 |

不把网页版、插件宿主或第二种 UI 工具包放进这个仓库的目标。
