# 026 — 行距、段间距与分页

- 状态：in-progress
- 依赖：023、024
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-26

## 目标与背景

按字体度量、段落属性和页面几何生成行框与分页，为 M0 基础文本编辑提供可靠默认排版；固定行距、软换行、空段和对象不能按字号近似。

## 必读

- [页面与段落排版](../architecture/editor/page-layout.md)、[文字塑形](../architecture/editor/text-shaping.md)、[增量排版](../architecture/editor/incremental-layout.md)
- [文档模型](../architecture/editor/document-model.md)、[测试](../standards/testing.md)

## 范围与非目标

实现自然行高、段落标记度量、软换行、带独立固有度量输入的内联对象、基础分页与显式分页。断行以 Unicode 17 UAX #14 为规范基线；M0 先覆盖英语默认规则和中文横排的 `zh` locale tailoring，并按[CLREQ 调查记录](../research/006-cjk-and-academic-layout.md)落实选定的行首/行尾禁则。ICU4X locale tailoring 不代表完整支持 CLREQ。文档全局默认语言来自 `.cdoc` 的 `globalSettings.defaultLanguage`，再由 Paragraph 和 Run 覆盖；最终断点不切开扩展字素簇或 shaped cluster，并保留完整段落 shaping 上下文；完整 UBA 行级视觉重排与复杂上下文安全重塑由 027 验收。支持 Multiple/AtLeast/Exactly 的版式语义，但 M0 只使用默认行距和段距；用户可选的行距/段距命令由 025 增加。明确相邻段、新页首段、baseline 和固定行高溢出策略。

不包含复杂表格分页、任意 DOCX 模板解析、文档网格及跨平台逐像素分页一致性。首版只支持 Word Normal 默认配置及它明确提供的段落分页属性；后续期刊模板按各自规则映射到 `.cdoc`，不建立通用 LaTeX profile。

## 前置条件与待决策

023 页面几何、024 shaping metrics 可用。默认行距、段距与分页采用 Word Normal 配置：1.15 倍行距、段前 0 pt、段后 10 pt、孤行/寡行控制开启；Exactly 按 WordprocessingML 的固定行高定位规则执行。页面几何仍按 023 的 A4 设置。期刊模板可通过 `.cdoc` 全局段落样式覆盖这些数值；025 可在之后增加用户编辑属性的命令，不是基础排版的前置任务。

## 实施步骤

1. 分离 line box、baseline、glyph ink bounds 和对象度量。
2. 实现行高类型及段间距规则。
3. 实现分页和跨页段落范围引用。
4. 覆盖英语/中文断行、对象、空段、tab-only、固定溢出和段间距边界。
5. 独立评审并复验。

实现按职责拆为 `shaping/unicode.rs`（ICU4X/字素边界）、`shaping/font_selector.rs`（Parley 到 Fontique）、`paragraph/`（整段 cluster 到行范围）、`line_metrics.rs`（行框）和 `pagination.rs`（稳定 Paragraph ID 与行范围页片段）。`ResolvedDocumentTextSettings` 在文档设置代际编译一次并由各段落共享；013 再拥有增量布局缓存与失效传播。

## 预计改动

caret-layout 行/段布局、页结果、缓存失效和测试。

## 验收标准

- [ ] 三种行距的数值、对象下限、baseline 位置和固定溢出语义有测试。
- [ ] 新文档默认配置为 Word Normal 数值：Multiple 115%、段前 0 pt、段后 10 pt、孤行/寡行控制开启；默认设置随 `.cdoc` 保存，并可被期刊模板/局部段落属性覆盖。
- [ ] Word profile 的 Exactly 短行框定位、相邻段距与分页约束有对照用例；段距测试比较行推进、基线距离和净空白，不以字段值本身代替布局结果。
- [ ] 空段、空格、tab、图片/公式行不会零高或无限分页。
- [ ] 软换行不触发段间距；相邻段规则明确。
- [ ] 跨页片段引用同一文档范围，不复制正文。
- [ ] 结果正确性不依赖增量缓存；改动前后分页内容连续、无丢失或重复。
- [ ] 英语使用 Unicode 17 UAX #14 默认机会；中文应用 `zh` locale tailoring 与已选 CLREQ 行首/行尾禁则，测试标点边界和中英混排；语言按文档默认 → Paragraph → Run 解析，断点不切开 UAX #14 规则、扩展字素簇或 shaped cluster，行计算保留整段 shaping 上下文。复杂 shaping 安全重塑和完整 UBA 行级视觉结果由 027 验收。
- [ ] 图片/公式等未来对象通过独立 `InlineObjectMetrics` 输入其 advance、ascent/descent 和 ink bounds；Opaque 占位字形不能冒充对象固有尺寸。
- [ ] 独立评审和测试通过。

## 验证计划与结果

| 日期 | 环境 / 检查 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-26 | `cargo test --offline --locked -p caret-layout --lib` | 行高、Unicode 断行、字体候选、对象与分页用例通过 | 46 项通过；含英语/中文、emoji、空段/空格/tab、对象高度、段距折叠、显式分页及跨页稳定 ID 用例 |
| 2026-10-26 | `cargo clippy --offline --locked -p caret-layout --all-targets -- -D warnings` | 无警告 | 通过 |
| 2026-10-26 | `cargo test --offline --locked --workspace` | workspace 回归与 native/QML 集成通过 | 通过，native CTest 4/4 |
| 2026-10-26 | `cargo clippy --offline --locked --workspace --all-targets -- -D warnings` | workspace 无警告 | 通过 |
| 2026-10-26 | Parley 0.12 高层 Layout API 只读评估 | 确认能否完整替代现有段落实现 | 可复用完整段落 shaping 和 bidi level，但存在 UAX #9 L1、Unicode 17 断行数据、InlineBox bidi 和逐行重塑限制；暂不直接替换，见架构边界 |
| 2026-10-26 | `cargo fmt --all --check`、`git diff --check`、`cargo test --offline --locked --workspace`、`cargo clippy --offline --locked --workspace --all-targets -- -D warnings` | 格式、完整 workspace/native 回归、Clippy | 通过；layout 46 项，native CTest 4/4 |

## 风险与回退

页边界错误会漏字或重复。先保证跨页内容区间连续且不重叠，再优化重排范围。当前已有布局函数仍接收调用方提供的 `LineSpacing` 与 pt 段距，尚未从 `.cdoc` 的 Word Normal 默认样式解析并驱动分页；在该连接完成前，不得把当前实现描述为 Word profile 已落地。

## 决策与工作记录

- 2026-10-25：从原 008 整体布局拆出段落度量和分页；增量缓存和脏段落范围仍由 013 验收。
- 2026-10-26：用户最初确认自然行高、零段距与固定行高溢出策略；后续明确默认采用 Word Normal 的数值和分页规则，旧默认决策由新配置取代。
- 2026-10-26：用户要求断行遵循 Unicode 国际标准并考虑语言差异；M0 先覆盖英语默认 UAX #14 与中文 `zh` locale tailoring。
- 2026-10-26：用户指定文档语言需作为文件全局默认值；使用 `.cdoc` 已有的 `globalSettings.defaultLanguage`，段落和 Run 仍可覆盖。
- 2026-10-26：用户指定中文排版参考 W3C CLREQ；任务仅承诺明确选取并测试的规则，不宣称完整实现 CLREQ。
- 2026-10-26：为避免子串重分析丢失 bidi 上下文和每行重复展平，段落只塑形一次，行布局按字节有序 cluster 做二分定位并只扫描本行；cluster 携带其字体度量，避免 RTL 视觉 cluster 次序与度量数组错位。
- 2026-10-26：Parley 0.12 高层 Layout 有可复用的段落/Bidi API，但仍缺行末 L1、Unicode 17 断行对齐和 InlineBox bidi；当前不把它包装成已完成的全局替代，视觉行重排留给 027 的适配验收。

## 完成摘要

未完成。
