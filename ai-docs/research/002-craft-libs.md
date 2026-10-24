# craft-libs

只打开来看行为。不复制源码，不 vendor，也不加 git 或 crates.io 依赖。

对象：[storytold/craft-libs](https://github.com/storytold/craft-libs)，调查时 HEAD `2ab6049`（2026-10-24）。许可是 `MIT OR Apache-2.0`。仓库 2026-10-09 才建立，接口一天内可以大改，所以不能当成可锁定的依赖。wordcraft `cf2fe47` 当时锁了三个不同 rev，也说明调用方跟不上它的变化。

## 值得对照

| 主题 | 看什么 | Caret 怎么用 |
| --- | --- | --- |
| 原子写入 `craft-atomic` | 同目录临时文件，先 fsync（macOS 用 `F_FULLFSYNC`），再 rename，然后 fsync 目录 | 保存自己写。顺序可以照这个，不拷文件 |
| 设置 `craft-settings` | 加载失败不拖垮进程；损坏文件隔离；schema 迁移；保留手改的 TOML 注释 | 设置 schema 在 Rust 服务里。存储格式自己定 |
| 文档路径 `craft-docpath` | 纯词法判断：拒绝远程地址、设备路径和非 file URL | 逻辑根以 Caret 的路径服务为准。它不做符号链接隔离 |
| 快捷键 `craft-keymap` | 作用域、冲突、系统保留键、用户覆盖。明确不支持按键序列（`chord.rs` 的 `ParseError::Sequence`） | 键位按 Caret 的命令表实现。`\` 加后续按键不属于单键查找。查找开销没有测过 |
| 翻译 `craft-i18n` | 目录回退和复数 | 文案用本仓库的 `.pa`。不嵌入 [storytold/craft-i18n](https://github.com/storytold/craft-i18n) 的 TSV |

## 做对应功能时再看

- **打印。** wordcraft 只用了 `craft-ui` 的打印对话框状态，以及 `craft-print` 的系统队列：Unix 走 `lp` / `lpstat`，Windows 走 PDF API。没有查到可直接替换的跨平台打印库。
- **PDF 写出。** 它给 krilla 打了补丁：panic 展开期间 `drop` 直接返回，写入被标记为 poisoned，避免写出半截文件。没打补丁的 crates.io krilla 对不上这个检查，会编译失败。导出若要这条保证，按这个行为自己做，不带走 `vendor/krilla`。
- **字体嵌入。** `craft-fonts-build` 在构建时做子集化。字体文件的许可没有确认，不拷贝字体。

## 不参考

`craft-ui` 是 egui 控件。`craft-ai-core`、`craft-cms`、`craft-media`、`craft-prepress`、`craft-plate-test`，以及 `vendor/` 里的 hayro 系列，和编辑器无关。`craft-quit`、`craft-platform`、`craft-window` 只在做 macOS 退出、系统外观或 Linux 标题栏时再翻。
