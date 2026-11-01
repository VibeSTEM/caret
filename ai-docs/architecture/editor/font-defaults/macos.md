# macOS 字体默认映射

| 目标 | 映射/解析 | 实际字体名 |
| --- | --- | --- |
| 正文 `Script` | Fontique 0.12.0 CoreText fallback，以 Script 样本请求 CoreText 字体级联 | 由当前 macOS 字体目录和字体级联决定 |
| 正文 `Script+Language` | 将已支持的 Script + 语言传给 CoreText fallback；语言影响 fallback 列表 | 由系统语言偏好、macOS 版本及已安装字体决定 |

正文 fallback 由 CoreText 字体级联依字符和语言选取；Fontique 负责将它接入 shaping 字体查询。代码与数学角色使用仓库内置字体，不依赖此平台映射。字体安装/移除或系统更新后，正文 fallback 结果可能变化。相关来源：[Apple Core Text 字体级联](https://developer.apple.com/library/archive/documentation/StringsTextFonts/Conceptual/CoreText_Programming/Overview/Overview.html)、[Fontique CoreText 后端](https://docs.rs/crate/fontique/0.12.0/source/src/backend/coretext.rs)。
