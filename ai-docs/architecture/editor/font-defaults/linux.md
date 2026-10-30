# Linux 字体默认映射

| 目标 | 映射/解析 | 实际字体名 |
| --- | --- | --- |
| 正文 `Script` | Fontique 0.12.0 Fontconfig fallback，以 Unicode 字符集构造匹配请求 | 由发行版 Fontconfig 配置和已安装字体决定 |
| 正文 `Script+Language` | 在 Fontconfig 匹配请求中加入语言集和字符集 | 由 locale、Fontconfig 配置和已安装字体决定 |
| 代码 `<default>`（Qt 命令输入） | `QFontDatabase::systemFont(QFontDatabase::FixedFont)` | Qt 查询的当前系统固定宽度字体 |
| Rust 代码字体角色 `<default>` | Fontique 0.12.0 `GenericFamily::Monospace` → Fontconfig `monospace` | 由发行版 Fontconfig 配置决定 |

Linux 发行版、桌面环境和用户配置均可能改变 `monospace` 或 Script fallback 实际解析到的字体；Qt 命令输入的 FixedFont 是另一条 Qt 动态查询路径。Caret 不把某套发行版字体名写成跨 Linux 默认值。相关来源：[Fontconfig 用户配置](https://fontconfig.pages.freedesktop.org/fontconfig/fontconfig-user.html)、[Fontique Fontconfig 后端](https://docs.rs/crate/fontique/0.12.0/source/src/backend/fontconfig.rs)。
