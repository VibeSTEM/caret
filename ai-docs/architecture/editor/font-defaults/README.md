# 平台字体默认映射目录

更新日期：2026-11-01。本文汇总各平台正文脚本/语言回退来源；代码与数学角色使用仓库内置默认字体。字体目标、用户覆盖层级和 `.cdoc` 字段以[多文字体系字体选择](../font-selection.md)为唯一权威。

## 映射统计

| 平台 | 正文 Script / Script+Language 回退 | 映射文件 |
| --- | --- | --- |
| macOS | Fontique 0.12.0 CoreText；按 Script 与可识别语言标签请求系统 fallback | [macOS](macos.md) |
| Windows | Fontique 0.12.0 DirectWrite；以脚本样本和语言映射到系统 fallback | [Windows](windows.md) |
| Linux | Fontique 0.12.0 Fontconfig；以字符集和语言集查询系统 fallback | [Linux](linux.md) |

**总计：3 个桌面平台；3 种正文系统回退后端。**命令输入和 Rust 代码 shaping 使用同一内置 JetBrains Mono 字体族；数学布局使用内置 STIX Two Math。正文脚本目标不在此复制成静态字体族表：有效 Script 集合由锁定版本的 ICU4X Unicode 数据生成，具体字体名由运行设备的字体后端解析。Fontique 对特定 Script+Language 对的显式追踪是有限集合，未追踪的合法语言仍保留在文档设置中，但不能据此声称系统 fallback 一定按该语言区分。

## 记录边界

本目录只记录正文系统回退后端及其观测来源，不承诺固定的预装字体名。`.cdoc` 字段、用户偏好优先级、`<default>` 语义与解析行为以[字体选择规范](../font-selection.md)为准。

## 参考

- [Fontique 0.12.0](https://docs.rs/fontique/0.12.0/fontique/)：本项目锁定的字体枚举、平台回退与通用族接口。
- [Core Text 字体级联](https://developer.apple.com/library/archive/documentation/StringsTextFonts/Conceptual/CoreText_Programming/Overview/Overview.html)：macOS 按字符和语言回退字体。
- [DirectWrite 字体回退](https://learn.microsoft.com/en-us/windows/win32/directwrite/introducing-directwrite)：Windows 多语言脚本塑形和字体选择。
- [Fontconfig 匹配](https://fontconfig.pages.freedesktop.org/fontconfig/fontconfig-user.html)：Linux 字体匹配配置与语言属性。
