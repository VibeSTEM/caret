# latex-rust 数学排版后端验证

更新日期：2026-10-31。此报告是任务 [042](../task/042-latex-rust-math-backend.md) 的 crate 元数据、锁定依赖、临时原型和适配边界权威记录。

## 结论

保留 `latex-rust` 作为 LaTeX 数学 parser 和 TeX 数学布局引擎。Caret 通过独立低层 `caret-math` crate 隔离版本、API 适配与上游 patch，不自行实现公式 parser/layout。workspace MSRV 已提升到 Rust 1.99，与 `latex-rust 3.0.1` 元数据一致。

Caret 尚未对外发布公式 LaTeX 输入语法，因此选用上游已经支持的语法：

| Caret 结构 | 输入/交换形式 | 原型结果 |
| --- | --- | --- |
| `InlineMath` | `$...$` 或 `\(...\)` | parser 可识别；公式正文 layout 使用 Text style |
| `Equation` | `\[...\]` 或 `$$...$$` | parser 可识别；无编号的 Caret 块由文档结构表达 |
| `Align` | `\[\begin{aligned}... \\ ...\end{aligned}\]` | parser 生成多行 aligned AST，Display style layout 成功 |

不要求 `equation*` / `align*` 环境，也不需为兼容未发布的输入格式改写 parser。`aligned` 与 `align` 的布局路径不同，Caret 的多行输入契约采用 `aligned`。

## 包元数据和依赖

来源：[crates.io 3.0.1](https://crates.io/crates/latex-rust/3.0.1)、[官方 docs.rs API](https://docs.rs/latex-rust/3.0.1/latex_rust/)、[上游源码](https://github.com/jscarr64/LaTeX-Rust)。本地 `cargo info latex-rust@3.0.1` 确认版本 3.0.1、`rust-version = 1.99`、MIT OR Apache-2.0。STIX Two Math 2.13 随包嵌入，字体使用 SIL Open Font License 1.1；分发需保留上游许可和 NOTICE。

crate 归档 908 KiB，解包 2.1 MiB。默认特性树中 `latex-rust` 仅直接依赖 `skrifa 0.39.0`，完整锁定项如下；PNG 和 egui 可选特性未开启。

| 包 | 版本 | crates.io checksum (SHA-256) |
| --- | --- | --- |
| latex-rust | 3.0.1 | `2250132fbda62040ccba5ede9336add3257c76fe004f016d929926bf7530e9b6` |
| skrifa | 0.39.0 | `9c9eb0b904a04d09bd68c65d946617b8ff733009999050f3b851c32fb3cfb60e` |
| read-fonts | 0.36.0 | `5eaa2941a4c05443ee3a7b26ab076a553c343ad5995230cc2b1d3e993bdc6345` |
| font-types | 0.10.1 | `39a654f404bbcbd48ea58c617c2993ee91d1cb63727a37bf2323a4edeed1b8c5` |
| bytemuck | 1.25.2 | `95832e849adfb21180ccb6826a99da14e5d266ae5c2e668e1602cf234f153797` |
| bytemuck_derive | 1.12.1 | `6a1f896587b6f2c069c73d2f0913e2d590c3990285cd2f0b6aa02b786b4c679c` |
| syn | 3.0.9 | `d78c8dee4c7bf0e14673097256fed6142ce9d3b85a408189d07482442145823b` |
| proc-macro2 | 1.0.107 | `985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9` |
| quote | 1.0.47 | `1fbf4db142a473a8d80c26bbf18454ed458bf8d26c8219c331daecfdbd079001` |
| unicode-ident | 1.0.27 | `a2c754d6c33795a1c324727428e5a7dedb5b06195f9890bdbcba760d3e246563` |

The workspace already locks newer `skrifa 0.44.0`, `read-fonts 0.41.0`, and `font-types 0.12.7`, so both dependency lines may coexist. The integrated binary's incremental size has not been measured.

## Security and depth

[RUSTSEC-2026-0311](https://rustsec.org/advisories/RUSTSEC-2026-0311.html) reports stack overflow on deeply nested untrusted LaTeX for versions `<1.0.5`; 3.0.1 is beyond the patched range. Its parser and layout APIs expose a 32-level default nesting limit. The probe generated nested numerators as `x -> \frac{x}{1}`: 31 levels parsed, 32 were rejected. This sample describes that exact nesting shape only. Production must retain parser/layout limits and not raise them without evidence.

This review checked the latex-rust advisory and its affected range; it did not run `cargo audit` against the entire eventual Caret dependency tree. Run a full dependency audit before adding the crate to the production workspace.

## API and adapter boundary

The public `MathBox` exposes width, height, depth, italic correction, baseline shift and `BoxContent`. The payload distinguishes glyphs (character, OpenType glyph ID, scale), rules, horizontal/vertical lists, overlap, kerns and decorations. The temporary probe read width/height/depth/shift, glyph fields, rules and container hierarchy for inline math, display math and a two-row `aligned` formula.

The probe is field-access evidence, not a completed renderer adapter. It did not map italic correction, color values, `Frame` inner content, line endpoints/thickness/border color; accumulate absolute x/y; or verify rational `Dim` conversion to Caret integer units, rounding and overflow. Task 044 owns those checks.

`MathNode::Text(TextRun, TextStyle)` preserves text-mode content, but upstream `text_items` lays it out character by character through the embedded math font. English text laid out; Chinese parsed but failed layout with `MissingGlyph('速')`. There is no public Parley shaping, language, font fallback or external glyph-provider hook.

Use `caret-math` as a low-level adapter crate with no dependency on Qt, Parley, or `caret-doc`. `caret-doc` remains authoritative for the persistent math schema; `caret-layout` and `caret-engine` bridge the Caret tree to the adapter API. Required upstream patch/API capabilities:

- A text-layout provider receives a text run, style, size and caller context, then returns shaped-run identity (font, glyphs, offsets and clusters) plus width/height/depth. Layout must use those dimensions before placing surrounding fractions and scripts, and carry the shaped-run handle in its box tree for rendering.
- Layout accepts caller-stable Caret math-node IDs and propagates them to output boxes. This mapping enables editable hit testing/caret geometry.
- Parser token/AST source byte ranges and preprocessing offset mapping support import diagnostics. Source ranges and math-node provenance are separate mappings.

The adapter preserves failed input source and a locatable error. On successful import the Caret tree is authoritative; the persistence/export contract decides whether original source bytes are retained after normalization.

## 验证记录

隔离临时 Rust 项目锁定 `latex-rust = 3.0.1`，运行默认特性 `cargo run --locked`、`cargo tree --locked` 和 `cargo +1.95.0 check --locked --ignore-rust-version`。最后一条命令绕过包声明的 MSRV，只作为升级前边界探测；workspace MSRV 现已提升到 1.99。原型验证如下：

- 行内数学及显示定界 `\[x^2 + 1\]` 均解析并生成 MathBox 几何。
- `\[\begin{aligned}a &= b+c \\ d &= e+f\end{aligned}\]` 解析成两行并完成 Display style 布局。
- 上述分子嵌套深度边界。
- 英文 `\text{...}` 使用数学字体字形布局；中文返回缺字错误。
- 上游不支持 `equation*` / `align*` 环境；Caret 输入约定无需采用它们。

记录验证和锁文件 checksums 后，按维护者指示删除临时 probe 源码及构建产物。生产 workspace 还没有加入 `latex-rust` 依赖。
