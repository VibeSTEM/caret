# macOS 字体默认映射

| 目标 | 映射/解析 | 实际字体名 |
| --- | --- | --- |
| 正文 `Script` | Fontique 0.12.0 CoreText fallback，以 Script 样本请求 CoreText 字体级联 | 由当前 macOS 字体目录和字体级联决定 |
| 正文 `Script+Language` | 将已支持的 Script + 语言传给 CoreText fallback；语言影响 fallback 列表 | 由系统语言偏好、macOS 版本及已安装字体决定 |
| 代码 `<default>`（Qt 命令输入） | `QFontDatabase::systemFont(QFontDatabase::FixedFont)` | Qt 查询的当前系统固定宽度字体 |
| Rust 代码字体角色 `<default>` | Fontique 0.12.0 CoreText `GenericFamily::Monospace` 静态候选 | `Courier`，然后 `Courier New`；实际可用性取决于系统字体目录 |

Caret 不另写正文 Script 到字体名表。CoreText 字体级联会依字符和语言选取合适字体；Fontique 负责将它接入 shaping 字体查询。Qt 命令输入的 FixedFont 与 Fontique 的静态等宽候选是两条解析路径，不能假设使用相同字体。字体安装/移除或系统更新后，正文 fallback 结果可能变化。相关来源：[Apple Core Text 字体级联](https://developer.apple.com/library/archive/documentation/StringsTextFonts/Conceptual/CoreText_Programming/Overview/Overview.html)、[Fontique CoreText 后端](https://docs.rs/crate/fontique/0.12.0/source/src/backend/coretext.rs)。
