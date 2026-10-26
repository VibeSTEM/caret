# 多文字体系字体选择器

调查日期：2026-10-26。目标是为 `.cdoc` 的字体默认值和 Rust 字体选择接口建立可扩展的底层契约；当前不新增字体选择窗口。此文的选择顺序和模块组织是 Caret 设计建议，标准依据与产品决策分开说明。

## 语言、脚本与字形变体

语言标签、文字脚本、字体族和字形不是同一个维度。语言按 [BCP 47 / RFC 5646](https://www.rfc-editor.org/rfc/rfc5646) 记录，例如 `en`、`fr`、`sr-Cyrl`、`zh-Hant-TW`；同一语言可能使用多个脚本，同一脚本也可以供多种语言使用。字体族名是用户请求；最终字形由实际字体、语言、粗斜体属性和 shaping 共同决定。

[Unicode 17 UAX #24](https://www.unicode.org/reports/tr24/tr24-39.html) 为字符提供 `Script` 和 `Script_Extensions`。`Latn`、`Cyrl`、`Grek`、`Arab`、`Hani` 等用于识别脚本，不能把 Unicode Block 当作脚本或字体覆盖范围。脚本也不能自动判断数学用途：普通希腊语正文和公式中的希腊变量可以使用不同字体。

`Hani` 是 Han 单脚本，包含汉字在中文、日文和韩文中的使用。CJK 是 Caret 的便捷字体分组，包含 `Hani`、`Hira`、`Kana`、`Hang` 和 `Bopo`；它不是一个 Unicode `Script` 值。ISO 15924 的 `Jpan`、`Kore`、`Hanb` 则分别表示日文多脚本组合、Han 与 Hangul、Han 与 Bopomofo 等 writing-system group，不能作为单个字符的脚本值直接查表。代码注册表应明确展开分组，不从代码名称猜成员。代码名称可对照 [ISO 15924 注册表](https://unicode.org/iso15924/iso15924-codes.html)。

## Cyrl 等脚本内部怎样细分

俄语、乌克兰语、保加利亚语和塞尔维亚语等都可以使用 `Cyrl`，但所需字符子集和字形习惯不同。OpenType 的 [`locl`（Localized Forms）](https://learn.microsoft.com/en-us/typography/opentype/spec/features_ko#locl) 明确以保加利亚、塞尔维亚和俄语西里尔字形为例：同一 Unicode 字符可根据语言系统选择本地化字形，语言元数据应传给 shaping，引擎在字体提供对应功能时应用它。

因此 Caret 不建立 `CyrlRussian`、`CyrlSerbian` 等自造 Unicode Script，也不在应用代码中手写字母到替代字形的映射。默认使用 `Cyrl` 字体，并保持有效 BCP 47 标签；用户确实需要不同字体族时，再在 `Cyrl` 下配置 `bg`、`sr` 等语言覆盖。同一字体已提供正确的语言字形时，不要求用户为每个分支选一次字体。

同样的原则适用于 CJK 地区字形和其他脚本语言系统。字体缺少某个 `locl` 语言系统时，程序不能凭语言标签制造对应字形，也不能据此把内容标成其他语言。请求语言与实际字体能力应在诊断/预览中可查看。[OpenType Layout 公共表结构](https://learn.microsoft.com/en-us/typography/opentype/spec/chapter2)规定了脚本下的语言系统及特性组织。

## 建议的文档设置

继续使用现有的 `globalSettings.defaultFontFamily` 作为默认拉丁字体和最后一层应用字体默认值，新增 CJK、数学、脚本以及脚本内语言覆盖。保持字段可选；未设置表示继承主题或系统默认，而不是打开文件时把本机实际匹配字体写入文档。

```json
{
  "globalSettings": {
    "defaultLanguage": "sr-Cyrl-RS",
    "defaultFontFamily": "Latin Family",
    "defaultCjkFontFamily": "CJK Family",
    "defaultMathFontFamily": "Math Family",
    "scriptFontFamilies": {
      "Cyrl": "Cyrillic Family",
      "Grek": "Greek Family",
      "Arab": "Arabic Family"
    },
    "languageScriptFontFamilies": {
      "Cyrl": {
        "bg": "Bulgarian Family",
        "sr": "Serbian Family"
      },
      "Hani": {
        "ja": "Japanese Han Family",
        "zh-Hant": "Traditional Han Family"
      }
    }
  }
}
```

示例族名是占位值。保存字体族名，不保存系统字体路径、字体目录索引或进程内 handle。字体是否存在、实际使用族和是否缺字属于派生结果。旧 `defaultFontFamily` 的通用回退作用保持有效；同一字体配置不复制到每个 Run。

`languageScriptFontFamilies` 的外键必须与 shaping 分析得到的脚本匹配，然后才在该脚本内查语言覆盖。例如 `Cyrl → sr` 只影响已解析为 `Cyrl` 的片段，不能因为段落标签是 `sr-Cyrl` 就把段内的拉丁引用、邮箱或 URL 改成西里尔字体。同理 `Hani → ja` 只覆盖日文内容中的 Han，假名仍由自身脚本覆盖或 CJK 默认提供。

脚本键使用 Unicode `Script` 属性的短值（与 ISO 15924 对齐的 `Latn`、`Cyrl`、`Hani` 等），不等于接受 ISO 注册表的每个代码。`Hans`、`Hant`、`Cyrs` 这类变体不作为字符脚本 key；`CJK`、`Jpan`、`Kore`、`Hanb` 等组合也不能混入单脚本 map，否则一个字符可能同时命中多项且顺序不唯一。语言键使用完整 BCP 47 标签，不使用 UI 语言或未经确认的语言检测结果。

语言匹配先使用大小写无关的精确标签，再使用 [RFC 4647 lookup](https://www.rfc-editor.org/rfc/rfc4647#section-3.4) 的截断思想形成回退链，例如 `sr-Cyrl-RS → sr-Cyrl → sr`。Caret 首版在每级只匹配相同配置 key，不从 `sr` 猜选一个更具体的 `sr-Cyrl`/`sr-Latn` 分支，也不通过 map 遍历顺序挑选。`zh-TW` 不等于词法上的 `zh-Hant`；如后续采用 CLDR likely-subtags，应使用固定版本的 ICU4X 派生匹配结果，保留文档原标签，不以 CLDR 资源继承代替语言匹配。[UTS #35](https://unicode.org/reports/tr35/tr35.html#Likely_Subtags)明确区分这些机制；[ICU4X `icu_locale`](https://docs.rs/icu_locale/latest/icu_locale/)已有 canonicalizer 和 likely-subtag expander，适合复用，但选库时需检查其支持的 BCP 47 范围，不能因匹配库仅支持常见 locale 而丢弃原文中合法的完整语言标签。

## 字体候选顺序

先按现有样式优先级解析局部环境、文档覆盖和主题默认，再在有效全局字体设置中选择候选族。局部 `Font` 环境的显式族优先。公式上下文先尝试数学字体；普通文本按已解析脚本查语言覆盖，再查脚本覆盖，CJK 成员再查 CJK 默认，最后查默认拉丁族与系统回退。候选族去重且保持顺序。

| 内容/配置 | 应尝试的应用候选族 |
| --- | --- |
| 局部 Font 环境 | 显式局部族，然后是该内容的全局候选 |
| 公式节点 | 数学族，然后是公式文本需要的脚本/语言候选 |
| Cyrl + `bg` | `Cyrl/bg` 语言族 → `Cyrl` 族 → 拉丁默认族 |
| Hani + `ja` | `Hani/ja` 语言族 → `Hani` 族 → CJK 族 → 拉丁默认族 |
| Hira / Kana / Hang / Bopo | 对应语言族 → 对应脚本族 → CJK 族 → 拉丁默认族 |
| 普通 Latn | `Latn` 下的语言/脚本覆盖 → 拉丁默认族 |
| 未配置或未知文字体系 | 拉丁默认族 → 系统按脚本和语言回退 |

这里的“数学 → 其他字体 → 默认拉丁字体”是内容对应的应用候选链；普通希腊字母不会仅因有数学字体就进入公式字体。数学上下文必须来自语义公式节点，不能用 `Script=Grek` 或单个 Unicode 数学符号猜测公式。后续公式布局还必须验证数学字体的 OpenType [`MATH` 表](https://learn.microsoft.com/en-us/typography/opentype/spec/math)与所需字形能力，普通正文 fallback 不能代替伸展括号、数学度量和公式布局算法。

所有候选后仍需要 shaping backend 的实际覆盖检查。族名存在不表示包含该范围的全部字符，也不表示支持该复杂脚本。回退尽量保持整个 shaping cluster/字素组合使用同一字体；不能为了补一个组合符把它随意拆离基字符。平台字体和文件都相同前，不承诺三系统逐像素或分页完全相同。[Microsoft 字体回退说明](https://learn.microsoft.com/en-us/windows/win32/intl/using-font-fallback)区分字符缺失与复杂脚本支持缺失，可作为覆盖检查的参考。

## Rust 中的规则组织

共享规则负责语言匹配、候选顺序、族去重、配置验证和缓存；脚本模块只提供该脚本真实需要的差异。语言模块是脚本规则的增量，不复制整份选择器。示意目录如下，未出现真实差异时不创建空模块：

```text
caret-layout/src/font_selection/
├── mod.rs                 # 选择器接口与上下文
├── resolve.rs             # 公共候选链和去重
├── language_match.rs      # BCP 47 lookup
└── scripts/
    ├── mod.rs             # resolved Script 分派
    ├── latin/
    │   ├── mod.rs         # Latn 共用字体策略
    │   └── french.rs      # 仅在确有字体策略增量时添加
    ├── cyrillic/
    │   ├── mod.rs         # Cyrl 共用字体策略
    │   ├── bulgarian.rs   # 仅放超出通用 locl/数据覆盖的必要差异
    │   └── serbian.rs
    ├── cjk/
    │   ├── mod.rs         # CJK 分组与通用回退
    │   ├── chinese.rs
    │   ├── japanese.rs
    │   └── korean.rs
    └── arabic/
        └── mod.rs         # Arab 共用字体策略
```

这个目录是扩展位置，不是首版必须全部生成的文件清单。法语通常可共用 Latn 字体选择；语言标签照常传给引擎。法语标点间距、阿拉伯语断行/两端对齐等若有独立排版规则，应放在相应断行/排版模块的脚本与语言层，字体选择模块只处理字体策略。阿拉伯字形连接、Cyrl 本地化字形和标准 OpenType 特性由成熟 shaping 引擎负责。

现有 Parley `FontSelector` 已接收 `Segment.script` 和 `ShapeOptions.language`，适合在实际字体选择处查候选；不需要为字体默认值重新按单个字符切断段落。Common/Inherited 的解析能力仍要以具体引擎版本和测试为准，不应把“前一个非 Common 脚本”当成覆盖所有语言的规则。

## 标点、组合符和运行性能

空格、数字和多数标点可能是 Common；组合符可能是 Inherited 或具有自己的 Script_Extensions。选择器应沿用段落分析器解析的脚本范围，保持完整字素和 shaping cluster；成对括号尽可能使用同一外围文字体系。独立字符分类只用于诊断，不用来在每个码点上反复改字体。Unicode 17 的 Common、Inherited 与组合序列处理说明见 [UAX #24 实现备注](https://www.unicode.org/reports/tr24/tr24-39.html#Implementation_Notes)。

配置在安装/修改文档设置时校验并编译成只读策略；字体目录只在创建或显式刷新时枚举。语言 lookup 链、脚本到组映射、候选族和族存在性缓存复用，编辑热路径不扫描目录或重新解析全局 JSON。字体查询缓存至少区分有效样式、resolved script、有效语言、数学/emoji 上下文、字体目录代际和字体策略代际；只用 `style_index` 缓存会让同样式的 Latn→Cyrl 片段沿用错误字体候选。

字体策略变化可能改变字宽、行高与分页，因此推进度量/样式代际并使受影响范围的布局失效。字色等纯绘制变化不扩大这条缓存键。实际族列表和缺失族诊断由布局结果产生，不反向修改请求的全局设置。

## 未来选择器界面

常用入口展示“默认拉丁字体、CJK 默认字体、数学字体”；高级入口按脚本展开，再按需添加语言/地区覆盖，例如 `Cyrl → 保加利亚语 bg`。列表不预先列出每种语言的空行，避免把语言目录变成上百个字体必填项。选择语言后可以复用当前字体并预览 `locl` 效果，也可显式选另一字体族。

字体族来自本机安装目录，支持搜索、样例预览和显式刷新。脚本覆盖筛选只作为提示，实际字符和 shaping 测试决定能否使用。预览同时显示请求族、实际族、回退和缺失状态；缺失字体可继续保留在文件中。可参考 [Qt 6.8 QFontDatabase](https://doc.qt.io/qt-6.8/qfontdatabase.html)按 writing system 枚举与按脚本设置 fallback 的能力，但正文字体决策继续由 Rust 的 FontCatalog/选择器统一，Qt UI 不另建一套正文回退规则。

## 本次基础与后续边界

本次适合落地可选设置类型、脚本和语言匹配、候选族解析、shaping 入口接线、代际和行为测试。数学偏好字段可先存储与解析；没有语义公式节点时不应用于正文，也不能据此声称公式排版完成。当前字体窗口冻结的约定继续有效，设置可先通过 API/命令接入。

旧文件没有新增字段时维持现有系统字体行为；已有 `defaultFontFamily` 保持通用默认意义。新设置应登记独立的 `requiredFeatures` 能力；不理解该能力的读取器保留数据并按既有安全预览策略处理，不能保存时丢掉规则。已知字段类型错误、无效标签和空字体名应原子拒绝；缺失本机字体属于可回退状态。未知可选字段继续保留，不把未来 ISO 15924 值静默解释成某个已知脚本。

实施验证至少覆盖：Latn/Cyrl/Grek/Arab/CJK 同段；Cyrl 下 `bg`、`sr` 与通用字体的语言选择；`sr-Cyrl` 标签内的 Latin 引用；Hani 的 `ja`/`zh-Hant` 覆盖与 CJK 其他脚本；组合符跨 Run；成对标点；缺失族和 cluster 缺字回退；相同 style 不同脚本/语言的查询缓存；字体设置往返和旧文件默认。验证 `locl` 必须使用已知包含相应语言系统的固定测试字体，并检查字形差异或 glyph ID，系统字体 smoke 不足以证明本地化字形正确。
