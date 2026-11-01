# Caret

<p align="center">
  <img alt="Written in Rust and Qt 6" src="https://img.shields.io/badge/written%20in-Rust%20%2B%20Qt%206-2b47b5?style=flat-square&logo=rust&logoColor=white">
  <img alt="Runs on macOS, Windows and Linux" src="https://img.shields.io/badge/runs%20on-macOS%20%C2%B7%20Windows%20%C2%B7%20Linux-3b5bdb?style=flat-square">
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-2b47b5?style=flat-square"></a>
</p>

Caret 是轻量的所见即所得结构化编辑器，用 Rust 实现领域规则，用 Qt 6 / QML 实现桌面界面。产品目标和里程碑见[需求与范围](ai-docs/architecture/requirements.md)。

仓库已建立 Cargo、CMake 和 QML 工程。`cargo run` 打开桌面文档窗口，已接入基础文本编辑和 `.cdoc` 打开/保存；公式、表格及其他能力的状态见[任务索引](ai-docs/task-index.md)。CI、夜间检查、桌面打包与 GoogleTest SDK 工作流已配置；当前远程验收证据与待复验项见[任务 006](ai-docs/task/006-infra-ci.md)。

在仓库根目录使用 `cargo format --check`、`cargo lint --check`、`cargo audit` 和 `cargo test --workspace --locked`；`cargo quality` 聚合这些检查。首次运行需要 git、rustup 和[平台前置](ai-docs/standards/dependency-acquisition.md#宿主前置)，并联网取得锁定工具。裸 `cargo test` 只测 launcher。桌面安装使用 `cargo caret-package`，SDK 的独立生产和消费见[CI 规范](ai-docs/standards/ci.md)。

| 内容 | 入口 |
| --- | --- |
| 协作规则 | [仓库协作指南](AGENTS.md) |
| 工作队列与实现状态 | [任务索引](ai-docs/task-index.md) |
| 产品行为与架构 | [架构文档](ai-docs/architecture/README.md) |
| 工程栈、版本和长期规则 | [技术规范](ai-docs/standards/README.md) |
| 外部项目调查 | [调查索引](ai-docs/research/README.md) |

## 致谢

- [Typst](https://github.com/typst/typst)（Apache-2.0）：公式排版移植自 Typst v0.15.1 的数学布局，来源见 [NOTICE](NOTICE) 与[调查 010](ai-docs/research/010-typst-math-layout.md)。
- [STIX Two Math](https://github.com/stipub/stixfonts)（SIL OFL 1.1）：内置数学字体。
- [read-fonts / skrifa](https://github.com/googlefonts/fontations)（MIT / Apache-2.0）：OpenType MATH 表与字形度量读取。
- [Parley / Fontique](https://github.com/linebender/parley)（Apache-2.0 / MIT）：段落塑形、换行与系统字体发现。
- [HarfRust](https://github.com/harfbuzz/harfrust)（MIT）：Parley 使用的 OpenType 塑形引擎。
- [swash](https://github.com/dfrg/swash)（Apache-2.0 / MIT）：字形轮廓与栅格化。
- [ICU4X](https://github.com/unicode-org/icu4x)（Unicode-3.0）：`icu_segmenter`、`icu_properties`，用于断行与 Unicode 属性。
- [unicode-segmentation](https://github.com/unicode-rs/unicode-segmentation)（MIT / Apache-2.0）：字素簇边界。
- [undo](https://github.com/evenorog/undo)（MIT / Apache-2.0）：撤销/重做历史。
- [Qt 6 / Qt Quick（QML）](https://www.qt.io/)（LGPL-3.0）：桌面界面与画布渲染。
- [Fontconfig](https://www.freedesktop.org/wiki/Software/fontconfig/)（MIT 风格许可）：Linux 系统字体配置。
- [cxx](https://github.com/dtolnay/cxx)（MIT / Apache-2.0）：Rust 与 C++ 之间的安全绑定。
- [serde](https://github.com/serde-rs/serde) / [serde_json](https://github.com/serde-rs/json)（MIT / Apache-2.0）：`.cdoc` 与数据交换的序列化。
- [pest](https://github.com/pest-parser/pest)（MIT / Apache-2.0）与 [quick-xml](https://github.com/tafia/quick-xml)（MIT）：主题 DSL 与 XML 解析。
- [sha2](https://github.com/RustCrypto/hashes)、[base64](https://github.com/marshallpierce/rust-base64)、[fs4](https://github.com/al8n/fs4)（MIT / Apache-2.0）：内容哈希、媒体编码与文件锁。
