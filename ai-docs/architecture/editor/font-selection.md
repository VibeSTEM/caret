# 多文字体系字体选择

更新日期：2026-10-29。本文规定文档默认正文字体如何映射到文本脚本和语言变体；外部资料与实现比较见[字体选择器调查](../../research/007-font-selection.md)。

## 分开的概念

- **语言**使用 BCP 47 标签，按 Run → Paragraph → `globalSettings.defaultLanguage` → `und` 回退。它用于 OpenType 本地字形、断行词典和语言相关排版，不决定 UI 语言。
- **字符脚本**使用 Unicode `Script` / `Script_Extensions` 数据，由 Parley 段落分析提供已解析脚本上下文。语言标签不能代替字符脚本，Unicode Block 也不能代替脚本。
- **字体族偏好**是用户选择的族名，不保证字体存在或覆盖整段字符。Fontique 负责查询本机字体、覆盖和 fallback；应用不自行扫描字体文件或维护字符到 glyph 的映射。
- **数学公式**由语义公式节点识别，不使用正文脚本字体选择链；M3 首版固定使用内嵌 STIX Two Math，见[公式与表格](math-and-table.md)。普通希腊字母或数学符号字符本身不能推断整段是公式。

## `.cdoc` 全局设置

```json
{
  "globalSettings": {
    "defaultFontFamily": "Latin Family",
    "defaultCjkFontFamily": "CJK Family",
    "defaultMathFontFamily": "STIX Two Math",
    "scriptFontFamilies": { "Cyrl": "Cyrillic Family" },
    "languageScriptFontFamilies": {
      "Cyrl": { "bg": "Bulgarian Family", "sr": "Serbian Family" },
      "Hani": { "ja": "Japanese Han Family", "zh-Hant": "Traditional Han Family" }
    }
  }
}
```

字段均可选。`defaultFontFamily` 是默认拉丁字体；`defaultCjkFontFamily` 覆盖 Hani、Hira、Kana、Hang、Bopo；`defaultMathFontFamily` 是独立的数学字体设置，首版默认并仅支持 `STIX Two Math`；`scriptFontFamilies` 允许按单一 Unicode Script 覆盖；`languageScriptFontFamilies` 仅在实际文本已解析为外层脚本 key 后，才匹配该脚本内的 BCP 47 语言变体。数学字体不进入正文脚本候选链；其他字体请求原样保留，首版回退到 STIX Two Math 时给出诊断。

`Hani` 表示 Han 单脚本，不等于整个 CJK。`Jpan`、`Kore`、`Hanb` 等是多脚本书写系统组，不作为单字符脚本 map 的 key。`Hans`、`Hant` 等变体通过语言标签或地区字体覆盖表达。脚本配置 key 使用 Unicode Script 短值；语言覆盖 key 使用完整 BCP 47 标签。查找先 exact，再逐级截去末尾子标签；不从宽泛标签反向猜更具体变体。

新增字段使用 `caret.font.family-preferences` required feature。旧文件没有该能力时按已有默认族/System UI 行为运行；不了解该能力的读取器不得静默丢弃数据后继续写入。

## 候选与优先级

已解析的 Run/Paragraph/Environment 样式优先级先于字体候选。有效局部 `Font` EnvironmentNode 提供最高字体请求；随后按正文内容上下文查询语言脚本覆盖、脚本覆盖或 CJK 默认，最后回退默认拉丁字体；语义公式走独立的 STIX Two Math 排版管线。每一候选仍需通过 Fontique 的覆盖检查；缺字或复杂脚本支持不足时继续系统字体 fallback。选择器尽量让一个 shaping cluster 及其组合符使用同一字体，不能将组合符从基字符处分拆。

普通 CJK 文本依据实际 script 选 CJK 族，`ja`、`ko`、`zh-Hans`、`zh-Hant` 等语言标签继续提供本地字形上下文。西里尔文 `Cyrl` 可用俄语、乌克兰语、保加利亚语和塞尔维亚语等；通用字体由脚本设置提供，用户确需不同字体族时通过 `languageScriptFontFamilies` 覆盖。若字体本身有 OpenType `locl`，语言标签交给 shaping 引擎选择本地字形，Caret 不自己替换字符或伪造字形。

## Rust 分层

- `font_selection.rs` 公共层负责设置校验、BCP 47 lookup、脚本/CJK 分组、候选顺序与去重；不在每个字符上解析 JSON 或扫描字体目录。较大的单元测试位于 `font_selection/tests.rs`。
- `paragraph/parley.rs` 消费 Parley 已解析的段落 shaping 上下文，并应用 `font_selection.rs` 生成的候选族偏好；字体覆盖和 fallback 由 Parley/Fontique 管理。`shaping.rs` 负责可复用的段落分析与样式边界，`shaping/unicode.rs` 提供 Unicode 边界规则。
- `ResolvedDocumentTextSettings` 在打开文档或全局字体/语言设置改变时编译一次，由同一设置代际的段落共享，避免每段深拷贝文档字体规则。字体族存在性快照随 FontCatalog 代际复用，只有显式刷新时重建。
- 以后只有当某脚本存在超出 OpenType `locl`、ICU/CLDR 数据和 Fontique fallback 的可复现差异时，才增加对应策略模块；共享脚本规则放在脚本模块，语言差异作为窄增量子模块。没有测试过的差异不加代码，也不预建空文件。
- 格式化环境语言元数据保持独立；样式覆盖不得改写文档语言。

当前已实现结构：

```text
crates/caret-layout/src/
├── font_selection.rs         # 字体偏好解析与候选规则
├── font_selection/tests.rs   # 字体偏好单元测试
├── shaping.rs                # 可复用段落分析与 shaping 上下文
├── shaping/unicode.rs        # Unicode 边界与语言 tailoring
├── paragraph.rs              # 行范围与 cluster 度量
├── paragraph/parley.rs       # Parley 段落布局适配
├── line_metrics.rs           # baseline、行框和溢出策略
└── pagination.rs             # 页片段、段间距与分页
```

使用 ICU4X 做 BCP 47 lookup 和 Unicode 属性读取，使用 Parley 的 `Segment.script`、`ShapeOptions.language` 和 Fontique 字体查询/fallback。Caret 的字体偏好匹配保留完整 BCP 47 标签；当前 Parley `Language` 只表达可解析的语言前缀，无法保留所有 extlang、variant 和 extension，因此不能承诺这些子标签都进入 OpenType `locl`。Fontique 只为有限语言/脚本对提供平台 fallback key；Caret 的显式 `languageScriptFontFamilies` 仍保留合法但未被 Fontique 跟踪的标签，并由同一字体查询器按候选族处理。任何扩展差异都先检查已有库 API 和数据，再决定是否编写规则。

Common、Inherited 和多脚本标点由段落分析上下文解析，默认沿用 Parley 已解析脚本。组合符与基字符保持完整 shaping cluster；成对标点遵循所在文字体系和语言，不以“上一个任意脚本”作为普适规则。Unicode 版本、字体目录代际、字体偏好代际、resolved Script、有效语言和公式上下文进入布局/字体缓存键；同一 style index 的不同脚本或语言不得复用错误字体查询结果。

## 范围

本架构允许全局字体偏好先通过 API/命令设置，不要求本阶段增加字体选择窗口。M0 先覆盖拉丁、CJK 与本机 fallback，并测试 `Cyrl` 中 `bg`/`sr` 与 Latin 引用不串用。公式通过 `defaultMathFontFamily` 使用独立数学排版字体，M3 默认并仅支持 `STIX Two Math`；该设置不属于正文字体候选链，见[公式与表格](math-and-table.md)。逐步增加语言变体时使用已知覆盖的固定测试字体检查实际 glyph/locl 差异；仅有系统字体 smoke 不证明变体字形正确。
