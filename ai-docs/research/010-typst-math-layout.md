# 010 — Typst 数学排版移植

更新日期：2026-11-01。

Caret 公式排版移植自 [Typst](https://github.com/typst/typst) v0.15.1（提交 `9dfd3a08500b7896045f907433cf7b4b02434fad`）的数学布局。本文记录选型依据、字体读取依赖的可用性、Typst 各文件的职责与 Caret 的取用范围，以及许可证处理。实现约束见[数学公式](../architecture/editor/math.md)与 [042](../task/042-math-backend.md)。

## 需求

- 文档持有 Caret 自有的数学树，解析器直接产出它，不经过第三方 AST。
- 独立排版输出包含字形 ID、规则线、线段、填充/描边矩形及节点盒；044 再映射为页面字形快照，供绘制、命中与光标使用。
- 度量来自 OpenType MATH 表，便于将来换用其他数学字体。
- 许可证与 Apache-2.0 兼容，可在仓库内修改维护。

## 候选对比

| 项目 | 许可证 | 度量来源 | 输出与接入 | 结论 |
| --- | --- | --- | --- | --- |
| Typst（`typst-layout` 数学部分） | Apache-2.0 | OpenType MATH 表 | 内部 `Frame`（尺寸、基线、定位项）；与 Typst 内容模型耦合，不能作为库直接调用 | 采用：移植排版算法，换成 Caret 的数学树与帧 |
| RaTeX | MIT | KaTeX `fontMetrics` 静态表（Computer Modern / KaTeX 字体） | 显示列表，面向 Canvas/SVG/PDF 等后端 | 不采用：度量绑定 KaTeX 字体表，不读取 MATH 表 |
| ReX | MIT / Apache-2.0 | MATH 表（内置 XITS/STIX 数据） | SVG 等渲染器 | 不采用：上游 2020 年后无维护 |
| latex-rust | — | — | 自有 AST 与盒树，渲染器面向 egui/SVG/PNG | 不采用：需要在其 AST/盒树与 Caret 数学树之间再做一层适配，且依赖外部维护节奏 |

Typst 的数学排版是现有 Rust 实现中最完整、仍活跃维护、直接基于 MATH 表的一套：分式、根式、上下标与角落字距、定界符伸展与字形组装、重音附着、矩阵与对齐、原子间距都有实现，且帧模型与 Caret 页面字形快照的需求一致。

## read-fonts 的 MATH 表

Caret 用 `read-fonts` 读取 MATH 表、`skrifa` 读取字符映射与字形度量。

| read-fonts 版本 | MATH 表 |
| --- | --- |
| 0.41–0.44 | 无 |
| 0.45.0 | 首次提供 `tables::math` |
| 0.46.0 | 当前最新 |

Caret 固定 `read-fonts =0.45.0` 与 `skrifa =0.44.0`。parley 0.12 与 skrifa 0.44 仍依赖 `read-fonts 0.41`，`deny.toml` 为这一个重复版本设置豁免；两版只读同一份字体字节、互不交换类型，parley 升级到依赖 0.45 及以上的版本后删除豁免。

## Typst 数学代码与 Caret 取用

路径相对 `crates/typst-layout/src/math/`，另注明的除外。

| Typst 文件 | 职责 | Caret 取用 |
| --- | --- | --- |
| `mod.rs` | 数学排版入口，遍历内容并调度各元素 | 不移植；调度由自写的 `layout/convert.rs` 按 Caret 节点类别完成 |
| `fragment/mod.rs` | `MathFragment` 与帧组合 | 改写为 `layout/frame.rs`：左上角原点、y 向下、推入子帧时展平 |
| `fragment/glyph.rs` | 字形片段、斜体校正、角落字距、伸展变体与字形组装 | 移植为 `layout/glyph.rs`；定界符短缺量并入 `fenced.rs` |
| `fraction.rs` | 分式与 `binom` | 移植为 `layout/fraction.rs`，去掉斜分式 |
| `radical.rs` | 根式与根指数 | 移植为 `layout/radical.rs` |
| `scripts.rs` | 上下标、上下限位置 | 移植为 `layout/scripts.rs`，只保留右上、右下、上、下四个位置 |
| `fenced.rs` | `\left…\right` 定界与伸展 | 移植为 `layout/fenced.rs`，不支持 `\middle` |
| `accent.rs`、`line.rs`、`cancel.rs` | 重音、上下划线、删除线 | 合并移植为 `layout/accent.rs` |
| `table.rs` | 矩阵与对齐 | 移植为 `layout/table.rs`，列对齐由 Caret 指定，新增 `\intertext` 整行 |
| `run.rs` | 行内片段拼接与换行 | 拼接部分移植进 `layout/run.rs`；不支持公式内换行 |
| `crates/typst-library/src/math/ir/process.rs` | 原子类别与间距 | 间距表移植进 `layout/run.rs`，类别改用 Caret 的 `AtomClass`，二元运算符降级用 TeXbook 规则 5、6 |
| `text.rs`、`shaping.rs` | 数学文本与 rustybuzz 塑形 | 不移植；Caret 按字符映射取字形，字母变体由自写的 `layout/alphabet.rs` 处理 |
| `crates/typst-library/src/math/ir/item.rs`、`resolve.rs`、`multiline.rs` | Typst 语法到数学中间表示 | 不移植；Caret 解析器直接产出文档数学树 |

Caret 自写部分：`layout/alphabet.rs`、`context.rs`（样式、字号与 MATH 常量）、`convert.rs`、`font.rs`（read-fonts/skrifa 封装）、`layout.rs`（引擎与缓存）、`layout/tests.rs`，以及 crate 根下的 `parser.rs`、`serialize.rs`、`symbols.rs`、`lib.rs`、`tests.rs`。

## 许可证

- Typst 为 Apache-2.0，与 Caret 相同；移植满足第 4 条：保留许可证、标明修改、随附 `NOTICE`。
- 每个移植文件在 SPDX 行后写来源块（仓库、原路径、提交、原版权方与许可证、修改说明），不加 Caret 署名行；规则见[源码署名](../standards/source-attribution.md)。
- 根目录 `NOTICE` 汇总 Typst 来源；README 致谢 Typst。
- 内置 STIX Two Math 字体为 SIL OFL 1.1，字体与许可证放在公共资源目录 `resources/fonts/math/stix-two-math/`；后续内置代码字体可并列放入 `resources/fonts/code/`。
