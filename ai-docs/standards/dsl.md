# `.pa` DSL

更新日期：2026-10-30。引擎随 `caret-dsl-core` 和 `caret-dslc` 移植。文法、kind 和诊断契约保持来源行为。改扩展名或改关键字属于单独的不兼容变更。

`.pa` 是给人读写的 UTF-8 源文件。界面运行时不解析它：语言走编译好的 QM，变量和主题走已经校验的快照。

## 文法

- UTF-8、LF、两格缩进，文件末尾一个换行。禁止 Tab。
- 可读性借用 `key: value` 和缩进。它不是通用 YAML：没有多文档、anchor、alias、tag、flow 集合或隐式类型。
- 值默认是不带引号的行尾标量。注释用 `//`。
- 关键字、kind、locale 和变量名用 ASCII。变量和主题键用 kebab-case。
- 一个解析器服务全部 kind。业务模块不用正则或第二套 parser 旁路处理。

当前 kind：

| kind | 用途 |
| --- | --- |
| `language` | 一个 locale 的界面文案。`[Context]` 是 Qt 上下文，`src` 是英文基线，`tr` 是译文 |
| `variables` | 受限的类型化值，包括 `project:/...` 资源引用 |
| `theme` | 颜色、间距和控件尺寸。键由主题 schema 解释 |
| `fsm` | 状态、初态、终态、事件边和具名 guard。没有脚本、动作或 I/O |

语言文件示例：

```text
version: 1
kind: language
language: zh_CN
sourcelanguage: en

[FileMenu]
open:
  src: Open
  tr: 打开
```

变量文件示例：

```text
version: 1
kind: variables

values:
  zoom: real = 1.0
  figure: resource = project:/assets/figure.png
```

`cn` 归一化为 `zh-CN`，写入 TS 时用 `zh_CN`。未知的必需版本和未知 kind 直接拒绝。

## 工具行为

从仓库根目录运行：

```text
cargo run --locked -p caret-dslc -- check file.pa
cargo run --locked -p caret-dslc -- format file.pa
cargo run --locked -p caret-dslc -- format --check file.pa
```

`check` 只读，失败时返回稳定错误码和源码位置。`format` 规范化空白并保留声明顺序，不重排主题键来改变诊断顺序。根 `cargo format` 覆盖 `crates/`、`resources/` 和 `qml/Themes/` 里的生产 `.pa`，跳过名为 `fixtures` 的目录。

解析器限制单文件和聚合适配的字节数、来源数和声明数。禁止 import、文件访问、网络、shell 和任意代码执行。任一来源失败时不发布部分快照。

## 构建流

语言：读取 `.pa`，解析并聚合，用 quick-xml 在构建树写出临时 TS，再调用供给里的 `lrelease` 生成 QM。QM 嵌入资源。临时 TS 和 QM 不提交。生成失败时保留上一份有效目录。`caret-dslc` 不自己下载 Qt。

主题和变量：Rust 求值后把快照交给 C++ ViewModel。QML 不读 `.pa`，也不保存第二份默认值。

FSM 只作为构建输入，生成代码留在 `OUT_DIR`。来源仓库里打开 STL 的那份状态机不移植；Caret 有真实流程消费者时再写自己的 `.pa`。

界面字符串用英文源加 `qsTr()` / `qsTranslate()`。随组件移植的控件带上自己的条目。工艺面板的文案不整包搬入。
