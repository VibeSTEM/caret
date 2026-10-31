# 044 — M3：公式布局、画布渲染与表格集成

- 状态：in-progress
- 依赖：012、027、043
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-01

## 目标与背景

把 `caret-math` 的排版结果接入 Caret 段落布局与页面字形快照，使绘制、命中、光标与环境高亮共用同一份 Rust 侧几何。Qt 只消费快照。总体验收由 [011 M3 公式](011-impl-math.md)管理。

## 必读

- [公式与表格](../architecture/editor/math-and-table.md)、[画布与交互背景](../architecture/interface/canvas-highlighting.md)、[结构导航](../architecture/editor/structure-navigation.md)
- [研究 010](../research/010-typst-math-layout.md)、[分层](../standards/layering.md)、[数值规范](../standards/numeric-values.md)、[测试规范](../standards/testing.md)

## 范围与非目标

布局：

- `MathLayoutEngine` 驻留在布局服务中，首次遇到公式时加载内置字体；按“内容哈希（含节点 ID）+ 字号 + 样式 + 字体标识”缓存 `Arc<MathLayout>`，上限 4096 条，超出时整体清空。
- 行内公式把宽度、上伸、下伸作为行内对象度量交给段落行框；行高随公式增高，基线对齐正文。
- `Equation` 与 `Align` 合成为显示伪段落，按显示样式排版并居中；`Align` 按列对齐。
- 插入点高度只取文字度量：含行内对象的行按去掉对象后的同一行计算光标上沿和高度，再对齐到本行基线，光标在公式旁与纯文字行等高。

绘制：

- 公式字形与矩形写入页面字形快照，与正文共用字形 atlas 与 QSG 路线；背景层在字形之下。

环境高亮：

- 快照输出 `environmentHighlights: [{level, kind, rects}]`，每个层级只输出最内层环境。
- 原生端按 `level` 拆分为块级与行内两组矩形，画布图层不变。
- 行内环境（含行内公式，矩形为公式盒）使用 `color-inline-environment-highlight`（默认浅粉 `#FCE4EC`）；块级环境（含 `Equation`/`Align`）使用 `color-block-environment-highlight`（默认浅米黄 `#fff2cc`）。

非目标：编号公式、字体切换 UI、SVG/PNG 交互路径、`\text{…}` 交给正文 shaping（当前用数学字体的正体字形）。

## 前置条件与待决策

表格单元格内公式依赖 045/046。插入点目前不会进入 `Equation`/`Align`，因此它们的块级高亮路径已接通但尚无可达的交互入口；待 043 开放块级公式编辑后验证。

## 实施步骤

1. 驻留引擎与按公式缓存；缓存命中不重新排版。
2. 行内公式度量进入行框；显示公式合成伪段落。
3. 字形与矩形写入页面快照，画布绘制。
4. 统一环境模型、`environmentHighlights` DTO 与原生拆分；统一插入点高度。
5. 表格单元格内公式；独立评审。

## 预计改动

`crates/caret-engine`（`math_layout.rs`、`layout.rs`、`page_glyphs.rs`、环境模型）、`crates/caret-layout`（行内对象度量、插入点度量）、`crates/caret-app` 快照、`native` 快照解析与画布。

## 验收标准

- [x] 行内公式、`Equation`、`Align` 可显示；分式、根式、上下标、定界符伸展使用 MATH 表度量。
- [x] 绘制、命中、光标几何来自同一布局快照。
- [x] 行内公式显示行内高亮矩形；块级环境高亮按层级拆分。
- [x] 公式旁插入点与纯文字行等高。
- [ ] `\text{…}` 使用正文字体与语言 shaping。
- [ ] 表格单元格内公式的显示、命中、导航与保存往返。
- [ ] macOS、Windows 验证。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-01 | Linux：`cargo test --workspace` | 布局、快照、高亮与光标高度测试通过 | 通过 |
| 2026-11-01 | Linux：`cargo clippy --workspace --all-targets -- -D warnings` | 无警告 | 通过 |
| 2026-11-01 | Linux：`cargo lint`、`cargo format --check`、`cargo deny check` | 通过 | 通过 |
| 2026-11-01 | Linux：原生构建与 `ctest`（offscreen） | 8 项全部通过 | 通过 |
| — | macOS / Windows | 构建与测试通过 | 未验证 |

## 风险与回退

缓存键使用 64 位内容哈希，碰撞会返回错误排版；概率极低，键中同时含字号、样式与字体标识。字体缺字时绘制占位方框并产生 `math.glyph.missing` 诊断。

## 决策与工作记录

- 2026-11-01：快照几何比较改为带容差（d59ca1d）；caret-app 测试失败来自 serde_json 浮点往返约 1 ulp 的偏差。
- 2026-11-01：插入点高度按无行内对象的同一行计算，不随公式撑高。
- 2026-11-01：独立评审（同一模型、只读）发现与处理见下表。

| 发现 | 处理 |
| --- | --- |
| （待评审） | — |

## 完成摘要

未完成。行内与显示公式的布局、绘制、高亮和光标已完成；`\text` 正文 shaping、表格集成与另外两个平台待做。
