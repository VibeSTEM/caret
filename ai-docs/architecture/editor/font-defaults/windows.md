# Windows 字体默认映射

| 目标 | 映射/解析 | 实际字体名 |
| --- | --- | --- |
| 正文 `Script` | Fontique 0.12.0 DirectWrite fallback，以脚本样本文本查询 `IDWriteFontFallback` | 由当前 Windows 字体集合和 fallback 配置决定 |
| 正文 `Script+Language` | 将脚本样本与可识别 locale 传给 DirectWrite 映射 | 由 Windows 版本、locale 和已安装字体决定 |

正文 fallback 仍由系统字体集合、DirectWrite 映射和 locale 决定；代码与数学角色使用仓库内置字体。未追踪的 Script+Language 不能保证语言级区分。相关来源：[Microsoft DirectWrite](https://learn.microsoft.com/en-us/windows/win32/directwrite/introducing-directwrite)、[Microsoft 字体回退与字体链接](https://learn.microsoft.com/globalization/fonts-layout/fonts)、[Fontique DirectWrite 后端](https://docs.rs/crate/fontique/0.12.0/source/src/backend/dwrite.rs)。
