# `.cdoc` 存储格式

更新日期：2026-11-03。本文是原生文件信封、兼容和原子写入语义的权威定义。持久化节点结构见[文档树与结构](document-structure.md)；文档范围和任务状态见[架构总览](../../README.md)与[任务索引](../../../task-index.md)。

`requiredFeatures` 使用稳定的 `caret.` 命名空间。下表定义各能力的语义与声明规则；已发布的能力 ID 语义不可变。

## 能力登记

编码器始终声明 `caret.paragraph.layout-settings`、`caret.text.default-font-size` 与 `caret.paragraph.content-flow.v1`；数学树使用 MathFlow 时声明 `caret.math.content-flow.v2`。其余已知能力按文档实际内容推导。

| ID | 能力 |
| --- | --- |
| `caret.env.bold` | 行内粗体环境 |
| `caret.font.family-preferences` | 文档字体族偏好 |
| `caret.paragraph.layout-settings` | 段落排版设置 |
| `caret.env.explicit-format-state` | Bold/Italic 环境中显式的 `enabled` 状态，包括局部关闭及重新开启 |
| `caret.env.font` | 行内字体族环境 |
| `caret.env.italic` | 行内斜体环境 |
| `caret.env.section` | 块级章节、层级计数和生成标签 |
| `caret.env.custom` | 块级可嵌套、可命名自定义容器 |
| `caret.env.templates` | 主题中的声明式环境模板、同 kind 变体和实例选择 |
| `caret.env.inline-text-templates.v1` | 行内文本环境主题模板的 kind 映射、默认属性、命名变体和实例覆盖解析 |
| `caret.math.align` | 多行对齐公式 |
| `caret.math.equation` | 单行显示公式 |
| `caret.math.inline` | 行内公式 |
| `caret.math.syntax.v1` | 当前数学语法子集 |
| `caret.text.default-font-size` | 文档默认字号 |
| `caret.text.language-tags` | BCP 47 文档/段落/Run 语言标签 |
| `caret.text.underline` | Run 直接下划线属性 |
| `caret.theme.snapshot` | `.cdoc` 内嵌文档排版主题快照 |

`requiredFeatures` 按字节序去重排序；除编码器始终声明的基础能力外，只列文档实际使用的能力。读取器发现未知能力时只读打开；能力 ID 的语义在发布后不可变，语义不兼容时使用新 ID 或提升 `.cdoc` 主版本。

Page 与 Inline 自定义环境分别使用 scope 专属的必需能力标识；`caret.env.custom` 的语义限定为块级自定义容器。

### 行内文本环境模板

主题快照中的 `defaultInlineEnvironmentTemplates` 是 kind 到模板 ID 的映射，例如 `{"bold":"body-bold"}`。映射所指的 `environmentTemplates[templateId]` 必须是对象，且含与映射 kind 相同的 `baseKind`；可选 `props` 提供该 kind 的默认属性，`variants` 是以稳定 variant ID 为键、以属性对象为值的映射。Bold/Italic 只接受布尔 `enabled`，Font 只接受非空字符串 `fontFamily`。模板顶层只接受 `baseKind`、`props`、`variants`；不支持的字段、错误类型、缺失模板或不匹配的 `baseKind` 都使当前版本文档校验失败。

行内环境实例的 `definitionId` 指向其 kind 默认模板中的 variant ID；解析顺序为 kind 默认模板 `props` → variant `props` → 实例 `props`。未设置的字段继续继承外层环境。Bold/Italic 最终仍未设置时默认为启用；空白实例 `fontFamily` 作为未指定处理。该契约由 `caret.env.inline-text-templates.v1` 标识，读取器要求显式声明此能力；不识别它的旧程序必须按未知必需能力进入只读安全预览。Caret 初期不实现旧 `caret.env.templates` 行内语义兼容；未声明新版能力但含旧行内 `definitionId` 引用的文档会保留为 `SafePreview`，不可编辑或保存；声明了新版能力但模板无效时则拒绝打开。

### 内容流能力

| ID | 能力语义 | 声明规则 |
| --- | --- | --- |
| `caret.paragraph.content-flow.v1` | `Paragraph.inlines` 与 InlineEnvironment 子项使用 `ParagraphContent` 混合序列 | 所有文档恒定声明 |
| `caret.math.content-flow.v2` | MathTree schema v3 使用 MathFlow，包含 MathNode、TextFlow 与数学环境包装分支；TeX 草稿与源码映射不属于持久节点 | 含 MathTree 的文档声明，包括不含 TextFlow 的纯数学树 |

读取器不把旧的临时 inline、字符串 `TextContent` 或 MathTree schema v2 转换成新 schema，也不提供迁移器；这些已知废弃格式在文档打开边界拒绝加载，不能作为不透明节点或只读预览保留。未知但可安全隔离的节点仍可进入不透明预览，未知必需能力则使文档只读打开。新数学 schema 由版本与必需能力共同标识；缺少声明或仍带旧源码字段的树按格式错误拒绝，旧字段不得通过 `extra` 回写。

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
- 已发布的较低版本：在内存里逐级迁移，保存时写成当前版本。未发布的早期开发格式不承诺迁移；043 的临时 inline、字符串 `TextContent` 与带持久 TeX/sourceMap 的双表示 MathTree 属于已知废弃格式，按上一节拒绝打开，不转换也不进入不透明预览。
- 每个主题快照另有主题格式版本；仅对已知旧版本且有完整迁移器的快照执行迁移。未知未来主题版本或缺失迁移器进入只读安全预览，不猜测解释。迁移在临时副本上完成并验证所有引用，成功后才替换当前内存快照；失败保留原文档和快照。
- `requiredFeatures` 是可选的顶层字符串数组，缺省为空。编码器始终声明 `caret.paragraph.content-flow.v1`、`caret.paragraph.layout-settings` 与 `caret.text.default-font-size`，其他已知能力按文档内容推导；当前程序不认识其中任一项时不得宣称文档已被正确打开：若能可靠定位未知数据，则进入标注清晰的只读安全预览，未知内容原样保留并占位；若未知能力可能改变解析边界且无法可靠定位，则拒绝预览。安全预览禁用保存和会遗漏不透明内容的复制/导出。能力标识及其语义的唯一登记表由本文维护；节点语义见[文档树与结构](document-structure.md)和[环境模型](document-environments.md)。
- 同一版本里，可选字段必须有默认值。读取器保留未知可选字段，并在往返保存时原样保留。该保留规则覆盖信封和 `document` 内部。
- 媒体放在 `media` 数组里，用 id 引用，内容是 base64。单个媒体超过 32 MiB 就拒绝插入。更大的外链不在第一版里。

保存先写同目录临时文件，fsync，再替换原文件。中途失败时原来的文件还在。畸形 JSON 返回错误，不 panic。

每个发布过的版本留一份夹具：读、写、再读必须相等；更高版本必须报错。本文的版本迁移契约适用于已发布格式；未稳定发布的早期开发分支不构成兼容承诺。
