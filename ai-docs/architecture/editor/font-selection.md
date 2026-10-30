# 多文字体系字体选择

更新日期：2026-10-30。本文是 Caret 字体目标、`.cdoc` 字段、解析优先级、平台回退和实现职责的唯一权威。文档节点结构见[文档模型](document-model.md)，字体平台映射与统计见[平台字体映射目录](font-defaults/README.md)，资料出处见[字体选择器调查](../../research/007-font-selection.md)。

## 设计边界

- 用户可为 Unicode Script 和 Script 内的 BCP 47 Language 设置字体族；目标身份和存储键稳定，不保存翻译后的名称。
- 系统默认字体由当前平台字体服务按实际字符脚本、语言与本机字体动态解析。Caret 不把单一系统版本的字体名当作跨平台默认值。
- 文档只保存用户请求的字体族。字体缺失或字形覆盖不全时，保留原请求并使用字体后端 fallback，不静默改写 `.cdoc`。
- 文字分析、字体族查询、覆盖判断和 fallback 由 Rust、Parley、Fontique 及系统后端完成；Qt/QML 负责传递设置和显示目录，不扫描字体目录、不再实现第二套字体选择器规则。

## 字体目标与 `.cdoc` 映射

选择器只有 `Script` 和 `Script+Language` 两类目标。所有有效单一 Unicode Script 平级展示，`Latn` 放在第一项作为默认入口；Latin、CJK 不构成额外的目标分类。Script 清单及英文名称由当前锁定版本的 ICU4X Unicode Script 数据生成，不手写维护。`Zyyy`、`Zinh`、`Zzzz`、`Jpan`、`Kore`、`Hanb` 等 Common/Inherited/Unknown 或多脚本书写系统组不作为单一字符 Script 目标。

| 目标 | 稳定身份 | `.cdoc` 字段 | 示例 | 作用范围 |
| --- | --- | --- | --- | --- |
| Script `Latn` | `script:Latn` | `globalSettings.defaultFontFamily` | `"defaultFontFamily": "Latin Family"` | Latin 默认族；也是正文候选链中的文档级通用回退 |
| 任意其他 Script | `script:<ISO15924>` | `globalSettings.scriptFontFamilies[script]` | `"scriptFontFamilies": { "Cyrl": "Cyrillic Family" }` | 该 Unicode Script 的默认族 |
| Script + Language | `script:<ISO15924>:language:<BCP47>` | `globalSettings.languageScriptFontFamilies[script][language]` | `"languageScriptFontFamilies": { "Cyrl": { "sr": "Serbian Family" } }` | 实际 Script 匹配且 BCP 47 语言匹配的文字 |
| 数学 | 非 Script 目标 | `globalSettings.defaultMathFontFamily` | `"defaultMathFontFamily": "STIX Two Math"` | 语义公式；首版固定 STIX Two Math |
| 代码 | 非 Script 目标 | `globalSettings.defaultCodeFontFamily` | `"defaultCodeFontFamily": "<default>"` | 命令输入和代码块；必须使用等宽字体语义 |

例如 `Cyrl:sr` 写入 `languageScriptFontFamilies.Cyrl.sr`，只影响同时匹配 Cyrl 脚本和 `sr` 语言的文字；不能因段落语言是塞尔维亚语就把其中的 Latin 字符改用西里尔字体。`Hani:ja` 只覆盖 Han 字符，不会把 Hiragana 或 Katakana 重新分类为 Han。常见的 `Hani:zh-Hans`、`Hani:zh-Hant`、`Hani:ja`，以及 `Hira:ja`、`Kana:ja`、`Hang:ko`、`Bopo:zh-Hant` 都遵循相同规则。

字段语义：

- `defaultFontFamily` 是 `Latn` 的 Script 默认；缺省时不创建人为字体名，继续由后端系统回退。
- `scriptFontFamilies` 保存其他 Script 的显式字体族请求。为读取旧文档，若其中已有 `Latn` 项，则它在有效字体解析中优先于 `defaultFontFamily`；选择器读取和编辑时也遵守此优先级，清除该目标会同时清除 Latin 默认项。新文档和新建目标使用 `defaultFontFamily`，不再新增 `scriptFontFamilies.Latn`。
- `languageScriptFontFamilies` 是按 Script 分组的语言覆盖，不是全局 Language → Font 映射。
- `defaultMathFontFamily` 与 `defaultCodeFontFamily` 不进入正文 Script 候选层级。
- `defaultCodeFontFamily` 的保留值 `"<default>"` 表示平台等宽通用族，不是可查询的字体族名。Qt 命令输入使用 `QFontDatabase::FixedFont`；Rust shaping 的代码字体角色使用 Fontique `GenericFamily::Monospace`。两者的解析后端和具体行为不同，不能假设解析到同一字体；平台映射见[平台映射目录](font-defaults/README.md)。
- `defaultCjkFontFamily` 是历史字段，仅旧文档解析兼容；新选择器、命令和样例不得继续写入或扩散。当前正文候选路径仍会在 Hani/Hira/Kana/Hang/Bopo 且没有更具体值时读取它。移除该字段需另行设计 `.cdoc` 数据迁移。

完整示例：

```json
{
  "globalSettings": {
    "defaultFontFamily": "Latin Family",
    "defaultMathFontFamily": "STIX Two Math",
    "defaultCodeFontFamily": "<default>",
    "scriptFontFamilies": {
      "Cyrl": "Cyrillic Family",
      "Arab": "Arabic Family"
    },
    "languageScriptFontFamilies": {
      "Cyrl": { "sr": "Serbian Family" },
      "Hani": { "ja": "Japanese Han Family", "zh-Hant": "Traditional Han Family" }
    }
  }
}
```

设置新增/改变时声明 `caret.font.family-preferences` required feature。旧文件缺少该字段时按系统默认与现存字段解析；读取器不认识 required feature 时必须拒绝写入，避免静默丢失用户偏好。未知可选字段保留规则见[`.cdoc` 存储格式](document-model/document-storage.md)。

## 正文字体解析

Run / Paragraph 的语言按 Run → Paragraph → `globalSettings.defaultLanguage` → `und` 回退；应用 UI 语言独立。Parley 对段落完成文字分析后提供 Script 和有效 shaping 语言，Caret 不用语言标签替代字符 Script，也不按 Unicode Block 猜脚本。

对每个文字范围，候选按以下顺序生成；缺少或重复的字体族会被跳过：

1. 局部 Font EnvironmentNode 的显式字体族。
2. 与实际 Script 对应的语言字体，按 BCP 47 lookup 从最具体标签向父标签查找。
3. 该 Script 的字体族。
4. 仅对 Hani/Hira/Kana/Hang/Bopo 读取历史 `defaultCjkFontFamily`。
5. 文档 `defaultFontFamily`（Latn 默认族）。
6. `SystemUi` 通用族。
7. Fontique/Parley 的平台系统 fallback。

数学公式使用独立数学字体请求；未知数学族按[公式规范](math-and-table.md#解析数学排版和字体)回退到内嵌 STIX Two Math 并报告诊断。代码布局通过 `ShapingStyle.font_role = FontRole::Code` 进入代码字体选择：普通字符先尝试显式 `defaultCodeFontFamily` 族名，再用 Fontique `GenericFamily::Monospace`，随后追加 `SystemUi` 通用族；`"<default>"` 从 Monospace 开始。Emoji presentation 字素仍优先使用 Emoji 通用族。代码角色不继承正文 Script、CJK 或数学字体偏好，并保留所选等宽字体的原生空格 advance，不应用正文的 0.5em 空格校准。Qt 命令输入仍使用 Qt 固定宽度系统字体或文档显式代码字体。当前文档模型尚无代码块节点，因此代码块节点接入时应设置此角色，不得另写字体解析逻辑。显式族名缺失或缺少字形时保留用户请求并继续候选链及平台 fallback；组合字符和其他 shaping cluster 应尽可能从同一实际字体塑形，不能把组合符从基字符处分开。真实字体解析可能因系统版本、字体安装、语言偏好和 Linux 发行版不同而变化；映射细节见[macOS](font-defaults/macos.md)、[Windows](font-defaults/windows.md)、[Linux](font-defaults/linux.md)。

字体选择候选只表达期望族名，不保证该族存在或覆盖全部字符。实际字体目录、可用族检查及显式刷新由 024 的 Fontique catalog 持有；刷新后使用新 catalog generation，失败时保留旧目录。数学字体的唯一首版回退是内嵌 STIX Two Math，其他值按公式规范报告诊断。

## 语言标签与平台能力

- 字体偏好接受完整 BCP 47 标签并按 lookup 链匹配；标签规范化不回写正文码点，也不静默更改文档默认语言。
- Parley 当前 `Language` 只表示可解析的语言前缀，不能保证所有 extlang、variant、extension 都进入 OpenType `locl`。
- Fontique 0.12.0 的 `FallbackKey` 对 Script+Language 平台 fallback 只维护有限组合。Caret 可以保存和匹配其他合法标签，但不能宣称操作系统 fallback 一定会针对任意语言覆盖切换；是否改善须通过库 API、系统行为与固定字体测试确认。
- OpenType `locl` 若可用，由 shaping 引擎结合语言标签选择本地字形；Caret 不替换字符或伪造字形。
- Common、Inherited、多脚本标点由 Parley 的脚本分析上下文处理。组合符保持所在 cluster；不通过“沿用前一个任意脚本”作为通用策略。

## 字体选择器交互

`\defaultfont` 使用下列 Rust 状态机驱动的层级：

```text
Close
Script（ICU4X 单一脚本列表；Latn 置首）
└── Close
    ├── Script       → 本机字体族
    └── Script+      → 输入 BCP 47 标签 → 本机字体族
```

上下键移动候选；Enter 或右方向键进入子层；左方向键返回父层；每层首项为 Close。选定族后立即提交到文档设置、返回 Script 根层并保持弹窗打开，以便继续设置；Escape 关闭弹窗。`\font` 复用字体族候选界面，但只展示本机字体族，选定后创建/应用局部 Font EnvironmentNode。QML 展示 Rust 提供的候选、选中态、层级提示和错误；Rust 持有状态、校验并提交设置。

跨 Rust/FFI/Qt/QML 的稳定目标 key 为 `script:<ISO15924>` 或 `script:<ISO15924>:language:<BCP47>`。该 key 只用于命令、DTO、状态和测试，不取代 `.cdoc` 字段。脚本名称来自 ICU4X 数据，语言名/输入使用 BCP 47，控件文案来自翻译资源，系统字体名保持原样。

## 实现与性能约束

- `crates/caret-layout/src/font_selection.rs` 负责编译文档字体规则、校验 Script key、BCP 47 lookup、候选优先级和去重；文档目标键与 BCP 47 语法校验由 `caret-doc` 提供唯一权威实现。不得在每个字符上读 JSON 或扫描字体目录。测试放在 `font_selection/tests.rs`。
- `paragraph/parley.rs` 使用 Paragraph shaping 已解析的 Script/Language、字体角色和 Fontique 查询；不得重复实现字体覆盖和系统 fallback。
- `ResolvedDocumentTextSettings` 在打开文档或字体/语言偏好变化时按代际编译一次，供段落共享。FontCatalog 目录存在性快照在 catalog generation 内复用。
- 样式索引、Script、有效语言、公式上下文、字体偏好代际、字体目录代际及 Unicode 数据版本必须进入相关 shaping/layout 缓存键，不能在同一 style index 下错误复用其他脚本/语言的字体解析结果。
- 只有当 ICU/CLDR、OpenType、Parley 或 Fontique 的既有行为无法表达一个可复现语言差异时，才增加窄策略；新增策略必须有固定字体与可观察结果测试。不得为假想差异预建脚本空模块或第二套 fallback 表。
- 性能结论需使用代表性混合脚本文本和字体目录，记录实测布局耗时/分配量及缓存代际；代码更短或增加缓存本身不算优化。

当前相关模块边界：

```text
crates/caret-layout/src/
├── font_selection.rs         # 文档字体偏好编译与候选规则
├── font_selection/tests.rs   # 字体目标和优先级行为
├── fonts.rs                  # Fontique 目录、generation 与本机族快照
├── shaping.rs                # 段落分析及样式范围
├── shaping/unicode.rs        # Unicode 边界及语言 tailoring
├── paragraph.rs              # 行范围和 cluster 度量
├── paragraph/parley.rs       # Parley shaping/layout 适配
├── line_metrics.rs           # baseline 与行框
└── pagination.rs             # 段落片段和分页
```

## 验收边界

行为测试至少覆盖 `Latn` 默认、多个平级 Script、语言覆盖仅作用于对应 Script、BCP 47 精确/逐级回退、未知 Script 数据往返、缺失字体请求保留、公式/代码与正文链隔离、代码族名在等宽通用族前且默认值直接使用等宽族、代码空格保持等宽 cell advance、目录刷新 generation 和 `.cdoc` 往返。测试应断言字体请求、可观察 shaping 结果或副作用，不断言某个 OS 上固定字体名；需要验证具体 fallback 时，使用固定已知字体与明确平台环境。

仅有系统字体 smoke 不证明不同脚本外观一致；字体基线、fallback 选择与跨平台差异需和 [033 OpenType baseline](../../task/033-open-type-baselines.md) 的规则一起评估。公式默认字体固定 STIX Two Math；行内/显示公式与表格模型见[公式与表格](math-and-table.md)。
