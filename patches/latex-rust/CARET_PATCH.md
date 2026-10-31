# Caret 补丁记录

本目录将 crates.io 发布的 `latex-rust` 3.0.1 作为 Cargo path patch 纳入仓库。上游 registry checksum 为
`2250132fbda62040ccba5ede9336add3257c76fe004f016d929926bf7530e9b6`。复制源码时保留上游 Apache-2.0/MIT 许可证、`NOTICE` 和 STIX Two Math 的 OFL 文件。

归一化后的 `Cargo.toml` 不注册上游测试和基准目标：本目录没有复制上游测试/基准源码；同时删除仅供这些目标使用的开发依赖。`Cargo.toml.orig` 保留未修改的上游 manifest 以供比对。上游测试与基准不属于 workspace 验收。

Caret 修改限于以下范围：

- `src/dim.rs`：将有限布局尺寸安全转换为整数微点单位。
- `src/layout/engine.rs`、`src/layout/engine_walk.rs`：支持调用方塑形的正文、外部正文盒及布局时稳定的来源节点 provenance。provenance wrapper 对字形、上下标和重音识别保持透明；phantom 保留视觉隐藏子盒的身份。
- `src/layout/mod.rs`、`src/lib.rs`：公开塑形和 provenance 适配接口。
- `src/render/{egui/emit.rs,png/raster.rs,svg/render.rs}`：这些 renderer 无法消费外部正文盒时显式返回错误。

源码跨度 parser adapter 尚未实现。Caret 当前只将完整公式范围赋给语法树根节点，嵌套 `MathSyntaxNode` 的范围仍为 `None`。在 parser token/AST span 及预处理偏移映射补齐并通过测试前，不得将嵌套源码位置视为可用。

更新补丁时，应与上游 3.0.1 原始源码逐项比对，保留原许可与声明，并在此记录每个改动文件及原因。
