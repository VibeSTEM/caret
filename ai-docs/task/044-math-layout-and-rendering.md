# 044 — M3：公式布局与画布渲染

- 状态：in-progress
- 依赖：027、043
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-10-31

本文件是 044 的唯一范围与验收记录。用户要求先合并当前分支并暂停未完成部分；因此本任务仍为 `in-progress`，当前实现状态与恢复顺序见下文。

## 目标

通过 `caret-math` 适配 `latex-rust` MathBox 布局，并将公式呈现在 Caret 页面画布。布局、绘制、命中和光标几何共用同一 Rust 快照；Qt/QML 消费快照，不实现公式领域规则。

044 提供不依赖表格模型的公式布局、绘制、命中与焦点能力。表格模型及保存由 [045](045-table-model-and-editing.md) 负责，表格中的定位、裁剪、绘制与导航由 [046](046-table-layout-and-canvas.md) 负责；总体验收由 [011](011-impl-math.md) 管理。

开始实现时按需查阅[公式与表格架构](../architecture/editor/math-and-table.md)、[画布与命中任务](027-canvas-and-hit-testing.md)、[分层规范](../standards/layering.md)、[测试规范](../standards/testing.md)和[验证评审标准](../standards/validation-and-review.md)。

## 范围与边界

- 支持行内 `InlineMath`、块级 `Equation` 和多行 `Align`，公式默认使用 STIX Two Math。
- `\text{...}` 通过 Caret 正文 shaper 排版；字形、规则线、基线、盒尺寸和节点 provenance 使用 Caret 自有快照及整数度量。
- `caret-math` 隔离 pinned `latex-rust`，不依赖 Qt、Parley 或 `caret-doc`；engine 转换持久模型，layout 提供正文 shaping，FFI 传递验证后的 DTO，Qt 负责绘制。
- 命令和公式编辑状态由 Rust 持有；命中、caret、选区及绘制使用同代布局快照。失败保留有效文档和公式源码，并向用户显示可定位错误。
- 044 不接入表格模型，不实现编号公式、字体切换 UI、SVG/PNG-only 可编辑渲染或未经测量的大范围排版优化。

## 当前进度

| 能力 | 状态 | 当前边界 |
| --- | --- | --- |
| `caret-math` parser/layout adapter | 已实现基础 | 持久树转换、MathBox 快照、STIX 与正文 `\text` shaping、Align 多行组合和预算校验已接入；parser 仍只有公式级源码范围，缺 token/AST 细粒度 spans |
| 行内布局与页面绘制 | 已接通首个纵向切片 | 行内公式参与段高、分页和有序页面 glyph；Rust 栅格、FFI DTO、QSG 混合图元及错误提示已接通。需继续验证完整像素链、失败注入与方向混排 |
| 行内 MathFocus 与编辑 | 部分实现 | Session 有行内公式焦点、输入路由、部分结构编辑和撤销状态；画布尚未按公式几何进行命中/caret 投影，入口/退出及 UI 命令候选仍未验收 |
| 块级 `Equation` / `Align` | 部分实现 | 持久模型导入和 `layout_math_align` 已有；文档布局仍以源码预览为主，块公式页面绘制、焦点、命令和编辑未接通 |
| 性能、RTL 与跨平台 | 未完成 | 尚无公式场景基线；RTL/混合方向和三平台行为未验证 |
| 表格集成 | 后续任务 | 045/046 负责模型、单元格定位/裁剪、命中与导航 |

## 后续实施顺序

1. 以显式块布局输入接通 `Equation`/`Align` 的 display MathBox、资源命名空间、分页和页面 glyph；块公式不得伪装成可编辑正文段落。公式按不可拆分块处理，超高/超宽时必须可见失败，不得静默截断。
2. 将同一行内公式快照用于命中、MathFocus caret 和正文边界导航；覆盖左右进入/退出、连续公式、上下移动、选区、保存重开和旧 generation 拒绝。
3. 在 `latex-rust` adapter 中为 token/AST 建立 UTF-8 字节范围及预处理偏移映射；保留旧解析入口，并正确处理 token 重写、嵌套 `\text` 和 Align cell 的源码重基。
4. 接通块级创建、MathFocus、上下标、已提交节点删除和对应撤销/失败原子行为；只有布局及焦点投影可用后才开放命令候选。
5. 补公式完整行为矩阵、RTL/混排验证、代表性性能基线、三平台检查；处理领域评审发现并执行未参与实现者的完整 diff 终审。

## 验收标准

### 布局与渲染

- [ ] 行内 `InlineMath`、块级 `Equation` 和 `Align` 可见；分数、根式、伸展符号、基线、对齐和 `\text` 均使用对应布局度量与字体资源。
- [ ] 公式快照可被页面 glyph、FFI/QSG 以及后续 046 的单元格画布消费；正文与公式按视觉顺序混合绘制并正确裁剪。
- [ ] 字体缺字、未知 `defaultMathFontFamily`、无效树、布局失败和 GPU/QSG 失败有明确 fallback/诊断；失败保留文档和公式源，过期帧不覆盖当前结果。
- [ ] 缩放、主题色、字体和文档代际变化正确更新缓存；异步结果绑定 session、revision/generation 与几何身份。

### 输入、命中与编辑

- [ ] `\math`、`\equation`、`\align` 及 `\frac`、`\sqrt`、`^`、`_` 命令仅在对应结构和焦点能力可用时开放，创建/编辑支持撤销。
- [ ] 行内与块级公式支持上下标及已提交文本/节点 Backspace/Delete；结构边界、占位槽、失败原子性有行为测试。
- [ ] 正文方向键和点击可按同一布局几何进入公式；公式边缘退出后正文 caret/selection 投影正确。覆盖两侧边界、连续公式、上下导航、保存重开及相邻文本。
- [ ] parser token/AST 的 UTF-8 源码范围和预处理映射对转义、重写、嵌套文本、多字节输入、Align cell 和错误位置正确；不相关旧解析 API 保持兼容。

### 验证与边界

- [ ] Qt Quick/Qt Test 覆盖公式像素、命中、焦点和键盘行为；Rust/C++ 覆盖率门槛与行为用例分别核实，未覆盖项明确记录。
- [ ] 有代表性输入、机器/构建/缓存条件和可复现基线；没有测量数据时不宣称性能提升。
- [ ] 公式无需临时 SVG/PNG 作为交互对象；格式、lint、构建及三平台验证通过，领域评审与最终只读 review 完成。
- [ ] 单元格公式定位和裁剪留给 046 验收，本任务不依赖或改写表格模型。

## 已确认的架构决策

- 保留 `caret-math` 独立 crate：它隔离第三方 TeX parser/layout/font 依赖并拥有稳定适配契约。新增 crate 只有出现独立复用、发布、依赖约束或安全边界需求时才考虑。
- 持久文档模型由 `caret-doc` 权威持有；engine 负责模型转换和布局快照编排；layout 负责 Parley shaping；FFI 负责稳定 DTO；Qt/QML 不重复持有公式状态或数学策略。
- `ExternalText` 资源句柄只在创建它的 provider 命名空间内有效。公式快照与所属 provider 资源表成对保存，不跨公式合并局部 handle。
- `caret-layout` 输出稳定的 `PositionedParagraphDrawItem`，engine 不解释 Parley positioned enum；页面栅格按有序 draw item 合并正文 glyph 与公式图元。
- Align 行由上游 `EnvRow` 折叠，当前没有独立行盒几何；不能用 cell 几何伪造行 ID 命中或 caret。跨页 Align 需先获得保持列宽的行边界契约。

## 验证摘要

- Rust workspace 全量测试和 doc-tests 通过；本轮 native CTest 9/9、定向 Qt Quick 用例、`cargo lint --check` 九阶段、`cargo format --check` 和 `git diff --check` 通过。
- 当前完整分支 Rust 全局覆盖率函数 85.91%、行 86.99%；native 产品覆盖率函数 89.47%、行 85.22%。两侧 85% 门槛均通过。
- QSG 纹理创建失败尚无可靠注入；回调 ABA 已加 page submission generation 并经 Terra/medium 只读复核，但延迟 success/failure、窗口销毁行为仍待故障注入测试。
| 验证 | 最近结果 | 限制 |
| --- | --- | --- |
| `cargo test --locked --offline --workspace` | Rust workspace 与 doc-tests 全量通过 | — |
| `ctest --test-dir target/native/debug --output-on-failure` | native CTest 9/9 通过 | QSG 实际纹理失败没有故障注入 |
| Qt Test `documentCanvasMapsPageCoordinatesAndHitsPages` | offscreen + software backend 通过，含 mixed commands、clip、页面错误横幅 | 未覆盖真实公式全链路像素或数学 caret/hit |
| `cargo lint --check` | 九阶段通过 | 最终文档修改后再确认 |
| `cargo format --check` / `git diff --check` | 通过 | 最终文档修改后再确认 |
| 锁定 `lupdate qml native -recursive` 与 QM 加载 | 119 个消息源；四个 locale 翻译完成，QM 加载通过 | 仅在本轮新增提示文案时相关 |
| `cargo coverage` / `cargo coverage native` | Rust 全局函数/行 85.91%/86.99%；native 产品函数/行 89.47%/85.22%，门槛通过 | native 有 4 个函数 coverage data mismatch 警告；门槛按产品范围汇总 |
| 性能基线 / 三平台 / RTL 混排 | 未完成 | 044 暂停点，恢复后补齐 |

## 当前未解决风险

- 行内 U+FFFC 替换与 U+200B shaping 在 RTL/混合方向、对象选区和 caret/hit 映射上未验证。
- 块级公式尚未进入真正的 display 页面布局；现有段落分页不能直接表达不可拆分的数学块，需要先明确页尾、整页过高/超宽及失败回退策略。
- 块级公式快照的缓存应绑定内容、字体、文本 shaping 和布局上下文；重分页只更新页面位置，不应重塑未变化的公式。
- Session checkpoint 会克隆会话/历史/快照；输入热路径的成本没有基准。QSG 图元也会在受影响帧重建，均不得视为已优化。

## 决策与评审记录

- `caret-math` crate 边界已由此前架构评审确认合理；转换持久数学树由 engine 持有，不让 parser crate 依赖 `caret-doc`，不为文件拆分新增 crate。
- parser span 审计建议在 patched parser 构树时传递与 AST 同形的 range sidecar，不能事后按相同节点值或源码搜索猜跨度；须覆盖 group rewrite、嵌套 `\text` 与 Align cell 重基。此设计尚未实现。
- 块级公式审计建议使用显式 DisplayMath 布局输入，复用 `layout_math_tree(..., Equation)` / `layout_math_align` 和现有 page glyph 管线；不把块公式伪装成可编辑段落。此设计尚未实现。
- Terra/medium 对公式布局边界、provenance、正文/数学字形分流、错误诊断、checkpoint/reuse 和本轮 QSG page submission generation 分别做过只读复核；发现项已修复或记录为验收缺口。
- 之前的 Sol/medium 评审只覆盖较早的完整 diff，不能视为当前终审。当前分支合并前对 `main...HEAD` 及本文件最终改动重新执行独立 Sol/medium 只读评审。

## 暂停点

当前分支合并后按用户要求暂停 044。恢复时从上述“后续实施顺序”第 1 项继续；在 044 实际完成并取得验证证据前，状态保持 `in-progress`，不得将本任务或 [011](011-impl-math.md) 标为完成。
