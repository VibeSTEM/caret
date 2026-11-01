# `.cdoc` 存储格式

更新日期：2026-11-01。本文是原生文件信封、兼容和原子写入语义的权威定义。持久化节点结构见[文档树与结构](document-structure.md)；文档范围和任务状态见[架构总览](../../README.md)与[任务索引](../../../task-index.md)。

`requiredFeatures` 使用稳定的 `caret.` 命名空间。当前登记表由 `crates/caret-doc/src/model/features.rs` 与存储校验实现共同定义：

## 能力登记

编码器始终声明 `caret.paragraph.layout-settings`、`caret.text.default-font-size` 与 `caret.paragraph.content-flow.v1`；数学树使用 MathFlow 时声明 `caret.math.content-flow.v1`。其余已知能力按文档实际内容推导。

| ID | 能力 |
| --- | --- |
| `caret.env.bold` | 行内粗体环境 |
| `caret.font.family-preferences` | 文档字体族偏好 |
| `caret.paragraph.layout-settings` | 段落排版设置 |
| `caret.env.explicit-format-state` | Bold/Italic 环境中显式的 `enabled` 状态，包括局部关闭及重新开启 |
| `caret.env.font` | 行内字体族环境 |
| `caret.env.italic` | 行内斜体环境 |
| `caret.env.section` | 块级章节、层级计数和生成标签 |
| `caret.env.custom` | 当前已实现的块级可嵌套、可命名自定义容器 |
| `caret.env.templates` | 主题中的声明式环境模板、同 kind 变体和实例选择 |
| `caret.math.align` | 多行对齐公式 |
| `caret.math.equation` | 单行显示公式 |
| `caret.math.inline` | 行内公式 |
| `caret.math.syntax.v1` | 当前数学语法子集 |
| `caret.text.default-font-size` | 文档默认字号 |
| `caret.text.language-tags` | BCP 47 文档/段落/Run 语言标签 |
| `caret.text.underline` | Run 直接下划线属性 |
| `caret.theme.snapshot` | `.cdoc` 内嵌文档排版主题快照 |

`requiredFeatures` 按字节序去重排序；除编码器始终声明的基础能力外，只列文档实际使用的能力。读取器发现未知能力时只读打开；能力 ID 的语义在发布后不可变，语义不兼容时使用新 ID 或提升 `.cdoc` 主版本。

Page/Inline 自定义环境尚未实现；接入时应新增 scope 专属的必需能力标识，不能静默扩张当前 `caret.env.custom` 的块级语义。

### 043 内容流能力

| ID | 能力 | 状态 |
| --- | --- | --- |
| `caret.paragraph.content-flow.v1` | `Paragraph.inlines` 与 InlineEnvironment 子项使用 `ParagraphContent` 混合序列 | 当前 schema；所有文档恒定声明 |
| `caret.math.content-flow.v1` | MathTree 使用 MathFlow，包含 MathNode、TextFlow 与数学环境包装分支 | 当前 schema；含 MathTree 的文档声明，包括不含 TextFlow 的纯数学树 |

当前读取器不把旧的临时 inline 或字符串 `TextContent` 结构转换成新 schema，也不提供向前兼容迁移器。无法按当前 schema 解码的节点进入不透明安全预览；已知旧 MathTree schema 只读打开，保持原数据，不能作为当前可编辑公式保存。切换到新 schema 后生成的文档由能力标识声明，新模型发布后旧读取器因未知必需能力只读打开。

## `.cdoc`

原生文件是 UTF-8 JSON。

```json
{
  "format": "caret",
  "version": 1,
  "requiredFeatures": [
    "caret.paragraph.content-flow.v1",
    "caret.paragraph.layout-settings",
    "caret.text.default-font-size"
  ],
  "media": [],
  "document": {}
}
```

- `format` 必须是 `caret`，否则拒绝。
- `version` 是正整数。只有不兼容变化才加一：删字段、改语义、增加旧程序无法跳过的枚举。新增可选字段不加版本。
- 版本更高：拒绝打开，提示更新程序，不猜测内容。
- 已发布的较低版本：在内存里逐级迁移，保存时写成当前版本。未发布的早期开发格式不承诺迁移；043 的临时 inline / 字符串 `TextContent` 结构按上一节进入不透明安全预览，不转换。
- 每个主题快照另有主题格式版本；仅对已知旧版本且有完整迁移器的快照执行迁移。未知未来主题版本或缺失迁移器进入只读安全预览，不猜测解释。迁移在临时副本上完成并验证所有引用，成功后才替换当前内存快照；失败保留原文档和快照。
- `requiredFeatures` 是可选的顶层字符串数组，缺省为空。编码器始终声明 `caret.paragraph.content-flow.v1`、`caret.paragraph.layout-settings` 与 `caret.text.default-font-size`，其他已知能力按文档内容推导；当前程序不认识其中任一项时不得宣称文档已被正确打开：若能可靠定位未知数据，则进入标注清晰的只读安全预览，未知内容原样保留并占位；若未知能力可能改变解析边界且无法可靠定位，则拒绝预览。安全预览禁用保存和会遗漏不透明内容的复制/导出。能力标识及其语义的唯一登记表由本文维护；节点语义见[文档树与结构](document-structure.md)和[环境模型](document-environments.md)。
- 同一版本里，可选字段必须有默认值。读取器保留未知可选字段，并在往返保存时原样保留。该保留规则覆盖信封和 `document` 内部。
- 媒体放在 `media` 数组里，用 id 引用，内容是 base64。单个媒体超过 32 MiB 就拒绝插入。更大的外链不在第一版里。

保存先写同目录临时文件，fsync，再替换原文件。中途失败时原来的文件还在。畸形 JSON 返回错误，不 panic。

每个发布过的版本留一份夹具：读、写、再读必须相等；更高版本必须报错。本文的版本迁移契约适用于已发布格式；未稳定发布的早期开发分支不构成兼容承诺。
