# 044 — M3：公式布局与画布渲染

- 状态：in-progress
- 依赖：027、043
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-10-31

## 目标与背景

通过独立 `caret-math` crate 适配 `latex-rust` 的数学盒布局，并呈现到 Caret 页面画布。绘制、命中测试、光标几何和选区共用同一 Rust 侧布局快照；Qt 只消费快照并绘制。公式在表格单元格中的模型/保存验收由 045 负责，布局、绘制、命中和导航集成由 046 负责；044 提供不依赖表格实现的公式快照。总体验收由 [011 M3 公式](011-impl-math.md)管理。

## 必读

- [公式与表格](../architecture/editor/math-and-table.md)、[页面画布与命中测试](../task/027-canvas-and-hit-testing.md)、[表格](../task/012-impl-table.md)
- [分层](../standards/layering.md)、[数值规范](../standards/numeric-values.md)、[测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

在 043 创建的低层 `caret-math` workspace crate 中扩展 `latex-rust` pin/patch、输入适配和 MathBox 快照 API；为 token/AST 增加细粒度源码字节范围及预处理偏移映射。接通块级 `Equation`/`Align` 的交互式创建、MathFocus 编辑和命令入口，补齐 `^`/`_` 上下标插入及已提交数学内容的 Backspace/Delete；确保可见布局可用后才开放候选。`caret-math` 不依赖 Qt、Parley 或 `caret-doc`，向 `caret-layout` 暴露调用方注入的 shaping provider 与稳定节点 provenance。实现 MathBox 到 Caret 布局快照的适配；包含 glyph ID/位置、规则线、基线、盒尺寸、文本片段映射及必要的字体资源身份。度量使用项目统一的整数表示。Qt 使用既有字形 atlas/QSG 路线，不把 QML 变为公式布局权威。表格模型/保存由 045 负责；044 仅提供可由 046 定位、裁剪和消费的通用公式快照，不直接依赖或接入表格模型。

覆盖段落行内公式、独占行 `Equation`、多行 `Align`，公式字体默认 STIX Two Math，并验证文档 `defaultMathFontFamily` 的唯一支持值与诊断回退；公式内 `\text{...}` 使用 Caret 正文 shaping。提供独立于表格结构的公式布局、绘制、命中和光标几何，供 046 做单元格集成验收。

不实现编号公式、字体切换 UI、SVG/PNG-only 可编辑渲染、独立 GPU 基准或大范围段落排版优化。

## 前置条件与待决策

依赖 043 创建的 parser crate、整段公式 source range 和稳定公式树，以及 042 的后端边界结论。027、043 已完成。表格内集成须等 045、046 提供表格模型与页面几何；公式布局、正文 shaping 与公式焦点可先独立实施。需在 1.99 工具链验证补丁包和依赖树，增加细粒度源码范围映射、正文 shaping provider 与布局盒到 Caret 节点的 provenance。Caret 树到 parser/layout 的深度预算映射须验证；超过上游默认预算的导入或编辑须原子拒绝。输入采用 `\[...\]` 和 `\[\begin{aligned}...\end{aligned}\]`；不为未发布的公式语法增加星号环境 parser patch。如果上游 patch 不能满足契约，暂停集成并复核 patch 范围，不另写 TeX parser/layout，也不用 SVG/PNG 绘制绕开契约。

## 实施步骤

1. 定义缓存键和 generation：公式树、字体、字号、语言、缩放/分辨率中哪些影响布局、绘制及 atlas 缓存。
2. 把数学盒输出映射到 Caret 整数坐标与字形/rule 快照；确认 `\text{...}` 混合正文 shaping 时尺寸与基线一致。
3. 通过适配层 patch 暴露 token/AST 细粒度源码范围和预处理映射；用错误位置用例验证原始输入定位。
4. 接通 `\equation`/`\align` 插入命令和显示公式 MathFocus 编辑，MathFocus 路径覆盖行内与块级结构。
5. 实现 `^`/`_` 上下标插入和已提交数学节点的 Backspace/Delete，定义结构边界、占位槽、撤销及失败原子行为。
6. 接入画布绘制、光标几何、命中测试与选择范围；让正文方向键在相邻的行内公式边界进入既有公式 MathFocus，并由同一布局快照选择入口 MathPos；验证左右两侧边界、连续移动、退出及正文光标投影一致，输入更新不会重建未受影响的公式/段落快照。
7. 补充具有行为断言的布局/渲染/缓存测试，做性能对比；独立架构与代码评审并处理发现。

## 预计改动

`crates/caret-layout` 数学布局适配，`native` 画布 glyph/rule 绘制，必要的 FFI 快照传输与 Qt 节点绘制；限制为公式内容所需的最小边界。

## 验收标准

- [ ] `InlineMath` 与块级 `Equation`/`Align` 可显示；多行对齐、基线、括号/分数等伸展符号使用数学布局度量。
- [ ] `\math`、`\equation`、`\align` 均可创建并进入相应公式焦点；`\frac`、`\sqrt` 以及行内/块级 `^`、`_` 编辑候选可用。候选只在对应公式渲染和焦点投影可用时开放，创建与编辑可撤销。
- [ ] 行内与块级公式均支持 `^`/`_` 上下标插入，以及已提交文字/节点的 Backspace/Delete；结构边界行为明确，撤销与失败原子测试通过。
- [ ] parser token/AST 提供 UTF-8 源码字节范围和预处理偏移映射；错误位置能映射回原始输入，并覆盖转义、规范化和边界位置。
- [ ] 绘制、命中、光标几何引用同一 generation/几何快照，旧异步结果不能覆盖新布局。
- [ ] 正文方向键可从行内公式两侧进入已有公式，入口 `MathPos` 由布局几何确定；公式左右边缘退出后正文 caret 与快照一致，覆盖保存重开、连续公式及相邻文本边界。
- [ ] 英文、中文 `\text{...}` 按正文字体与语言 shaping 显示；缺失字体/字形有定义明确的 fallback/诊断；未知 `defaultMathFontFamily` 保留文档原值，实际回退到 STIX 时可观测并可测试。
- [ ] 同一公式快照在缩放与三平台检查中不依赖临时栅格图；页面内编辑没有显著重复重排，性能结果记录基线和测试输入。
- [ ] 公式布局快照可由 046 在表格单元格的内容区域内定位并应用裁剪；单元格行为归 045/046 验收。
- [ ] 无 SVG/PNG-only 交互路径；测试、格式、lint、独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-31 | `cargo test --locked --offline --workspace` / 覆盖率测试套件 | 全 workspace 行为测试、native CTest 与文档测试通过 | workspace 全量运行通过：`caret-engine` 275 项、`caret-layout` 105 项、`caret-math` 37 项及其他 workspace 测试通过，native CTest 9/9 通过。macOS 安全退出测试改动后，`cargo coverage` 再次运行全 Rust 测试，崩溃 handler 定向用例也通过。未覆盖公式的 Qt glyph 绘制、MathFocus/QML 交互或跨平台矩阵 |
| 2026-10-31 | `cargo format --check` / `git diff --check main` | Rust、CMake、QML 格式与补丁空白检查通过 | 最新变更后均通过 |
| 2026-10-31 | `cargo lint --check` | workspace Clippy、依赖、CMake/native/QML 静态检查和分层通过 | 最新变更后 9 个 lint 阶段均执行完毕；包含 unused Cargo dependency、Rust/CMake 分层、qmllint、clang-tidy、include-cleaner 和 cppcheck |
| 2026-10-31 | `cargo coverage` / `cargo coverage native` | Rust 与 native C++ 函数、行覆盖率分别达到 85% | 最新 Rust 门禁通过：全局函数 85.23%、行 87.18%；`caret-engine` 89.19%/87.11%，`caret-math` 76.98%/71.18%（crate 指标仅供参考，门槛按全局计算）。native 两次重跑均未达行门槛：函数 89.70%，行 84.92% / 84.82%；4 个函数覆盖数据映射警告。native 产品源码本分支无改动，未调整覆盖统计范围；该门禁状态待解决。性能基线尚未测量 |
| 2026-10-31 | Terra/medium 数学布局 adapter 只读评审 | 检查 provider shaping、provenance、source range、预算与失败处理 | 修复嵌套/连续文本 shaping、公开语法树和多字符展开预算、align 根 provenance 及 provider 失败回滚；第二轮发现并修复 `\textsf`/`\texttt` 通用族映射、未提交资源提前转移、inline box 高度溢出。parser 目前只提供公式根范围，token/AST 细粒度源范围、文本叶 provenance、UI/FFI 绘制仍未完成。 |
| 2026-10-31 | Terra/medium provenance 透明性复核 | 检查 wrapper 对 glyph 查找、script/accent 布局和 phantom 行为的影响 | 修复后未发现明确布局差异；复核建议覆盖可伸缩符号、phantom、矩阵横线与嵌套 provenance，新增对应无/有 provenance 快照等价用例。矩阵行目前由 `EnvRow` 表示而非独立 MathBox，未产生行 provenance 几何 |
| 2026-10-31 | Terra/medium 正文与数学字形分流复核 | 检查真实 Parley provider 是否仅接管正文文本，数学字母命令仍由数学字体排版 | 分流符合预期；按建议加强真实 provider 回归，逐项比较完整 `MathLayoutItem` 序列，覆盖 `\mathbb`、`\mathcal`、`\mathfrak`、`\mathscr` |
| 2026-10-31 | Terra/medium 错误诊断覆盖率复核 | 检查错误文本、上游错误映射与测试的行为价值 | 未发现问题；公开展示契约逐项断言，真实触发和源码范围由解析行为测试覆盖 |
| 2026-10-31 | Sol/medium 最终只读评审 | 检查修复后相对 main 的完整 diff 与最新验证记录 | 多轮只读终审检查完整 staged/unstaged diff、架构改动和 macOS 测试专属退出路径；最新发现验证表中覆盖率数字过期，已同步为 Rust 85.20%/87.15% 与 native 行覆盖未达门槛；修正后终审确认无剩余文档不一致。评审代理未运行测试或覆盖率 |
| 2026-10-31 | `cargo test --locked --offline -p caret-engine math_layout` 和 `cargo test --locked --offline -p caret-layout math_text::tests` | 持久树 provenance、中文 shaping、事务资源及行内基线盒行为成立 | 1 + 3 项通过；provider 测试不代表布局快照已经被页面渲染消费 |
| 2026-10-31 | `cargo test --locked --offline -p caret-layout measured_inline_box_reserves_formula_geometry_in_parley_lines` | 验证 Parley 行内对象向 Caret 输出稳定的 ID、U+FFFC UTF-8 范围、视觉行、行局部位置和基线；拒绝普通文字、尾部索引及重复索引 | 本次新增断言在复验后通过；对象 y 由最终 Caret 行基线计算，未实现页面快照、对象命中/光标或 RTL 行为 |
| 2026-10-31 | `cargo test --locked --offline -p caret-layout` / `cargo clippy --locked --offline -p caret-layout --all-targets -- -D warnings` / `cargo fmt --all -- --check` / `git diff --check main` | 检查 layout crate 行为回归、新增公共类型的编译/lint、Rust 格式与补丁空白 | 105 项测试与 doc-tests 通过；Clippy、格式和 diff 空白检查通过 |
| 2026-10-31 | `cargo test --locked --offline -p caret-engine inline_math_layout_is_bound_to_its_paragraph_and_page_snapshot` / `cargo clippy --locked --offline -p caret-engine --all-targets -- -D warnings` | 验证两个 InlineMath 参与段落行宽/基线布局，并与页面定位及各自正文 provider 资源绑定 | 定向行为用例与 engine Clippy 通过；覆盖两个 provider 均从句柄 1 开始且资源表隔离。FFI/QSG 仍未消费公式绘制项 |
| 2026-10-31 | `cargo test --locked --offline -p caret-layout text_math_style_shapes_with_the_exact_inherited_parley_font_instance` / engine 回退、checkpoint 与 app 失败注入用例 | `Text` 的真实 Parley 字体实例/粗斜体合成与正文一致；空槽、不支持树和负尺寸有效树生成可定位占位快照；布局失败后文档、选择、快照与历史恢复；generation 仅在有过渡态文档修改时递增以拒绝旧结果 | 之前记录的 6 项通过；本轮改为按事务是否推进代际来区分。Session checkpoint 会克隆完整 Session 和历史；键入热路径开销未测量，不能宣称无性能回退，需基准后决定是否保留或改用轻量事务日志 |
| 2026-10-31 | `cargo test --locked --offline -p caret-engine paragraph_reuse_requires_matching_session_and_layout_context` / `cargo test --locked --offline -p caret-app ime_commit_reuses_unchanged_paragraph_layouts` | 普通布局拒绝跨 Session 或不同页面几何上下文复用；IME 预览通过显式候选入口保留未变段落 | 两项通过；全局字体/语言和 base style 也绑定到 reuse context，字体目录代际单独校验 |
| 2026-10-31 | `cargo check --locked --offline -p caret-app` / app、engine、layout Clippy / fmt / diff check | 检查跨 app、engine、layout 的回滚接口、样式适配、格式与 lint | 编译、三个受影响 crate 的 Clippy、格式和 `git diff --check` 均通过 |
| 2026-10-31 | Terra/medium 行内对象几何与输入契约复核 | 检查 y 坐标契约、长段落精度、U+FFFC 范围、防止普通文字误标及测试边界 | 修正坐标契约为行局部、仅接受唯一 U+FFFC 索引，并由 Caret 行基线推导 y；补充非法/重复索引和第二行 Exactly 行距测试。余留 RTL、混排、命中/caret 未验证 |
| 2026-10-31 | Sol/medium 当前完整 diff 终审 | 只读检查 `main...HEAD` 与工作树，核对输入校验、坐标、行高、错误路径、测试和任务记录 | 未发现可执行问题；确认 page glyph 仍拒绝 inline box，页面/QSG/命中/焦点与 RTL 能力仍作为后续验收。评审未运行测试、格式或 lint |
| 2026-10-31 | `cargo test --locked --offline -p caret-math layout::tests::node_provenance_does_not_change_math_placement_or_draw_primitives` | provenance 不改变尺寸、绘图项，并保留可表示节点 ID | 通过，覆盖上标、普通/可伸缩重音、phantom 和矩阵横线；矩阵 `EnvRow` 无独立 MathBox provenance，故行 ID 不在节点几何中 |
| 2026-10-31 | 公式 public error diagnostic tests | parser、conversion 和 layout 错误格式及上游错误映射保持稳定 | `cargo coverage` 中全 workspace 测试通过；覆盖全部公开 parse/layout error variant 的可观察错误文本 |
| 2026-10-31 | Terra/medium 044 完整性与分层缺口审计 | 对照剩余验收核对布局快照、Parley inline box、分页/glyph/QSG、MathFocus、块事务和失败原子性 | 审计当时确认 `layout_math_tree` 尚未进入文档布局；随后开始接入 InlineMath。`Style` 仍明确拒绝以避免静默丢失样式语义；单子透明 `Group` 折叠到孩子并保留来源别名，多子 `Group` 转为有序行。块级布局、MathFocus、页面绘制和失败原子性仍待完成。 |
| 2026-10-31 | Terra/medium parser source-range adapter 审计 | 核对上游 token/AST、预处理、重写规则、错误及 Align cell 偏移 | 上游 token、AST 与错误均无跨度；group rules 会删除/合成/重排 token，`\text` body 有独立词法和嵌套 parse。需要带范围 API 和兼容旧入口，并为 Align cell 源码切片后重基准；未实现。 |
| 2026-10-31 | Terra/medium MathFocus/命令状态审计 | 核对 Equation/Align 创建、编辑、MathPos、方向/点击、历史与错误恢复 | 确认 Equation/Align、上标/下标及已提交删除尚未实现；现有命令候选保持关闭是正确的，须等可见布局、焦点投影与块历史支持后再开放。 |
| 2026-10-31 | Align 持久模型布局适配 | 从持久 Align 合成后端无关的多行布局树，并检查预算和 provenance | 新增 engine `layout_math_align`：先以有界预扫检查源长度和合成节点/实际复制 payload 预算，再复用 `caret-doc` Align 联合结构与 ID 校验，保留公式根与 cell 来源 ID，并调用 `aligned` 后端布局；与 parser 对照的多对齐点/不等长行完整绘制项和尺寸测试、失败前无 shaper 副作用、真实包装深度边界测试通过。后端 `EnvRow` 没有可观测独立行几何，因此不伪造行 ID 几何；行 hit/focus 仍需上游或 Caret 快照扩展。 |
| 2026-10-31 | `cargo test --locked --offline -p caret-engine math_layout::tests` 与 `cargo test --locked --offline -p caret-math align_layout_budget_allows_its_two_model_wrapper_levels` | 持久 Align 合成、失败原子性及后端深度边界正确 | engine 定向 math layout 测试 8 项通过；caret-math 对齐 wrapper 深度边界测试通过。 |
| 2026-10-31 | `cargo test --locked --offline --workspace`、`cargo coverage`、`cargo lint --check` | 全 workspace、Rust 覆盖率与跨语言 lint 通过 | workspace 全量 Rust、doc 和 native CTest 9/9 通过。最新 Rust 覆盖门槛通过（全局函数 85.23%、行 87.18%）；lint 9 阶段通过。native C++ 覆盖行门槛未通过，且未因本次纯 Rust 改动重跑 native 覆盖。 |
| 2026-10-31 | checkpoint / composition preview 回归修复：`cargo test --locked --offline -p caret-engine -p caret-app`、`cargo fmt --all --check`、`git diff --check`、`cargo lint --check` | 检查 IME 预览身份、checkpoint 溢出恢复、QML 分页失败隔离与跨语言静态验证 | engine 296 项、app 202 项通过；格式与 diff 空白通过；`cargo lint --check` 九阶段通过。覆盖 Qt Quick 分页失败/恢复的行为测试尚缺；栅格器仍拒绝含公式页面，公式尚无页面绘制。 |
| 2026-10-31 | `cargo test --locked --offline -p caret-layout chinese_math_text_uses_parley_and_committed_resources_move_to_snapshot` / `cargo test --locked --offline -p caret-engine page_glyphs::tests` / 双 crate Clippy | 验证稳定行绘制投影的 Parley 字体实例、视觉顺序、局部基线和错误分类 | layout 定向用例与 engine 页面 glyph 11 项通过；两个 crate Clippy 通过。超长段落组合附标断言保持与原有 f64 局部偏移一致；未测试 QML/FFI/QSG 公式绘制。 |
| — | caret-layout 页面数学盒绘制与源位置映射测试 | 坐标、基线、字形与源位置映射一致 | 页面渲染消费尚未接入，未执行 |
| — | parser adapter token/AST source range 测试 | UTF-8 字节跨度及预处理偏移映射能定位原始输入 | 未执行 |
| — | `\equation` / `\align` 命令与块级 MathFocus 测试 | 两种显示公式可创建、编辑、撤销并与布局快照同步 | 未执行 |
| — | 上下标和公式删除命令测试 | 行内/块级结构编辑、光标位置、撤销及边界失败一致 | 未执行 |
| — | Qt 页面画布人工/组件验证 | 呈现、命中和光标位置吻合 | 未执行 |
| — | 公式布局基准 | 记录输入、构建与缓存条件 | 未执行 |
| 2026-10-31 | app IME 预览帧、布局失败回滚、普通输入错误和 IME reuse；engine preview provenance、paragraph reuse、InlineMath snapshot、负 Strut 与 page rasterizer 定向用例 | 验证返回帧保留活动 composition、无变更错误不推进 generation、有过渡态失败回滚推进 generation、预览身份/来源不可伪造以及公式页显式拒绝 | 定向用例通过；workspace Clippy 通过。栅格器错误尚未在 QML 呈现，不能称为用户可见错误或公式已绘制；页面渲染、命中和焦点验收仍未通过 |

## 风险与回退

字体缺字、MATH 度量差异、Qt atlas 生命周期或文本 shaping 混排可能造成错位。失败时保留公式源与错误状态，不能退回无法定位和选择的图片对象。性能优化限定在测量证明的热路径。

## 架构边界复核

- 保留 `caret-math`：它有独立、稳定的数学解析/布局适配职责和输入预算，隔离 pinned `latex-rust` 及其字体/布局依赖；不依赖 `caret-doc`、Parley 或 Qt。相比把上游 AST/MathBox 依赖放入 engine/layout，这减少了向文档与 UI 层泄漏第三方类型的风险。代价是维护 Caret 自有适配 API、上游 patch、版本升级重放和额外构建依赖；约 1.5 MiB vendor 目录主要是保留上游源码及字体资产，不等于 Caret 自行重写。当前不拆更多 crate；`layout/flatten.rs` 的职责拆分留在 crate 内部模块。
- Cargo 依赖保持分层：`caret-engine → caret-doc/caret-layout/caret-math/caret-structure`、`caret-layout → caret-doc/caret-math`、`caret-structure → caret-doc`、`caret-math → latex-rust`。文档模型是持久化权威，engine 承担持久树与后端无关语法树的边界转换，layout 提供正文塑形与段落布局，math 封装 TeX 后端。三种树的字段映射有维护成本，但代表不同兼容/生命周期边界，不为消除 `match` 而合并 schema。
- 当前仍不能将公式当作页面能力交付：engine 已提供 `layout_math_align`，从持久 `Align` 多行/多 cell 权威模型生成 `aligned` 数学快照并保留根与 cell 的 provenance；`InlineMath` 已进入同代段落布局和内部 page glyph 快照，页面对象携带公式行裁切、公式布局及独立正文资源表。空公式和单公式布局失败会生成有根 provenance 的占位绘制快照，但 FFI/QSG 尚未绘制；失败不再拿可能过期的 `MathTree.source` 伪装当前树。公式原语仍未栅格化、经 FFI/QSG 绘制，也未参与命中/焦点。后端 `EnvRow` 尚无独立行几何。模型组合由 engine 承担，不让 `caret-math` 依赖文档模型或重新解析源码。
- `MathLayoutItem::ExternalText` 的句柄仅在创建它的 provider 命名空间有效；`take_resources()` 返回 provider 局部资源表。多个 provider 的公式布局不能直接合并同名句柄；每个公式快照须与对应正文资源绑定，或携带明确的资源命名空间身份。段落复用现在要求相同 session、font generation、全局设置、PageSection、resolved text settings、base style，以及 Session 签发的恰好对应上一代、仅一个已变更段落证明；错误段落 ID、多段更改、回滚或换会话都会拒绝复用。IME 更新通过封闭候选对象只读暴露 preview/range，候选安装同时验证 base 身份、文档代际和交互 revision；预览布局自身身份/代际与主文档发布身份/代际分开保存，避免 composition preview 冒充主布局或让 Qt codec 拒绝增量。公式级缓存与绘制 atlas generation 校验仍待页面消费链实现。
- `caret-layout` 通过 Caret 自有 `InlineObjectPlacement` 隔离 Parley 输入类型，并以 `PositionedInlineObject` 输出 ID、行号、U+FFFC 源范围、段落局部 x、行局部 y 和基线；输入只能把唯一盒绑定到唯一 U+FFFC。行内盒固定按基线对齐，y 从同一 Caret 行的最终基线推导，避免用长段落全局 f32 坐标相减。`paragraph_inline_math_anchors` 在同一树遍历中输出公式 UTF-8 范围和段落/嵌套环境样式；`\text` 保留该正文样式，显式 `Rm` 才清除粗斜体和请求字体。逻辑 U+FFFC 替换为 U+200B 的 bidi、对象选择/caret/hit 映射尚未验证；页面快照现记录正文 glyph run 与公式对象的视觉交错顺序，但 FFI/QSG 尚未消费。engine 的 `page_glyphs` 已改用 `caret-layout` 的稳定 `PositionedParagraphDrawItem` 投影，保留正文 glyph 与 inline object 的视觉顺序、真实字体面/轴/synthesis 和 f64 行局部基线偏移；Parley item 解析留在 layout 层。公式 primitive 展开及 FFI/QSG 页面消费仍未接通。
- 持久 `Align` 由多行、多 cell 和独立 ID 组成；engine 现在负责将该权威模型组合成后端无关的对齐语法树。cell 与公式根进入快照 provenance；上游将行折叠为无身份 `EnvRow`，所以行几何与行 ID 映射仍是已知缺口，不能用 cell 几何冒充。
- 页面重用现有段落快照的路径有 composition 预览的跨会话复用特例。接入公式缓存时，缓存键须绑定文档/session 身份、字体代际和全局排版设置；需区分可复用的受信任预览与一般布局，避免把旧 geometry 标成新代。
- 行内环境与直接下划线 Run 是不同格式权威，Rust 负责边界策略和命令，TextMap 是可失效重建的派生索引，app 仅投影几何；目前没有 Qt/QML 重复持有这些领域规则的情况。

## 决策与工作记录

- 2026-10-29：从 011 拆出公式布局、画布渲染和与 012 的最终集成验收。
- 2026-10-31：开始布局适配，`caret-math` 将 pinned `latex-rust` MathBox 转成整数 glyph/rule/节点几何快照；`\text{...}` 由调用方 provider 预先 shaping，且 shaping 句柄支持失败回滚。中间树和持久树可保留整段公式根范围，parser 尚无 token/AST 细粒度跨度；页面渲染链尚未接入。表格模型/保存归 045，表格中的公式布局/绘制/命中/导航归 046，044 只提供可复用公式快照。
- 2026-10-31：加入 `caret-layout` Parley `\text{...}` shaper、STIX font Blob 与 inline baseline box 度量；Parley builder 已可放入调用方测量的 inline box，Caret 每行度量也计入 in-flow box 的 ascent/descent 并扩展段高。事务资源仅在成功提交后可转交。修正 `\textsf`/`\texttt` 的通用字体族解析。provenance wrapper 对 TeX glyph/script/accent 判断保持透明，phantom 也保留被隐藏子树的身份；布局等价回归覆盖上标、hat/widehat/箭头重音、phantom 与矩阵横线。串行 workspace 测试、native CTest 和 `cargo lint --check` 通过（本次 inline box 行高定向测试另行通过）。公式布局尚未并入分页、page glyph FFI/QSG、命中/焦点；parser 细粒度 spans 和表格集成也未完成，044 保持进行中。
- 2026-10-31：正文 shaper 不再接管 `\mathbb`、`\mathcal`、`\mathfrak`、`\mathscr` 等数学字母命令；真实 Parley provider 的测试与不提供正文 shaping 的基线逐项比较完整绘图项，验证 STIX 数学字形仍可用。
- 2026-10-31：完整性审计确认页面布局与现有公式适配 API 尚未连接；InlineMath 的 Parley UTF-8 位置映射、公式原语的有序绘制、块级布局/焦点、parser 细粒度范围和布局失败事务均保留为阻塞验收项。表格集成已由 045/046 承接。
- 2026-10-31：本轮 Terra/medium 完整 diff 复核修复了数学布局后端多层透明 Group 的叶与组 provenance 丢失、Group 别名范围过宽、普通段落 shaping 文本重复分配、Caret 公共 API 泄漏 Parley/latex-rust 类型、分层图缺边及 vendored manifest 中缺失的测试/benchmark target 和无用开发依赖。正文 U+FFFC 占位盒在 RTL/混合方向的 bidi、对象选择/caret/hit 映射仍无行为验证；当前没有页面消费者，接入前必须补齐并验证。性能变化仅由代码路径推断，没有基线或测量，不宣称已提升。
- 2026-10-31：架构专题复核确认保留 `caret-math` 的边界和单向依赖合理，没有新增 crate 的收益；将两处透明 Group provenance 递归与微点换算常量收敛为单一实现，并移除上游 shaper trait 中无人调用的事务 hook，让 Caret backend 作为唯一资源事务权威。**当时**尚需补齐 Align 模型组合入口、provider 资源命名空间绑定、稳定的行内对象几何投影及页面绘制/命中/焦点快照消费；本轮随后提供了 `layout_math_align`，其余集成项仍待完成。
- 2026-10-31：当时工作区验证：`cargo format --check`、`cargo lint --check` 通过；`cargo test --locked --offline --workspace` 全量通过，native CTest 9/9（在 macOS 崩溃测试子进程安全退出改动前）；当时 `cargo coverage` 在该测试改动后重跑全部 Rust 测试，Rust 全局函数/行门槛通过（85.20% / 87.15%），崩溃 handler 定向测试也通过。`cargo coverage native` 两次完整执行 CTest 9/9，但 native 产品行覆盖为 84.92% 和 84.82%，未达 85% 门槛（函数 89.70%，4 个 LLVM 函数映射警告）；本分支没有 native 生产源码改动，不通过缩小统计范围掩盖该缺口，标记为待解决/待验证。上游 `lupdate` 118 项四 locale 对照无变化，QM 加载 CTest 通过。macOS 崩溃记录子进程测试现用 test-only `_exit(128+signal)` 避免 CrashReporter 弹窗；非 macOS Unix 仍断言原信号退出，正式构建路径不变。
- 2026-10-31：将 Parley 行内对象位置投影为 Caret 自有 `PositionedInlineObject`。对象必须引用唯一 U+FFFC，禁止把普通相邻文字或尾部插入点伪装成对象 source range；y 采用行局部坐标并从 Caret 行基线推导。补充多行固定行距基线、UTF-8 范围、非法索引和重复索引行为断言；定向用例复验见上表。该投影尚未被分页 page glyph、FFI/QSG、命中或 MathFocus 消费，RTL/混合方向也未验证。
- 2026-10-31：engine 文档布局开始为 InlineMath 生成 `MathLayoutSnapshot`，以测量结果构造唯一 U+FFFC 的 inline box 参与段高和分页；每个公式独立持有其 `ParleyMathTextShaper` 资源命名空间。page glyph 提取将 inline box 投影为内部页面公式对象，包含公式快照、页面基线和 provider 资源；另保存正文 glyph 与公式对象的 Parley 视觉顺序。定向测试用两个 `\text` 公式验证局部 handle 同为 1 时资源表不串用。块 Equation/Align 仍为源码预览；公式绘制、错误后的可见回退、命中/焦点、RTL/混合方向和 parser spans 仍未完成。
- 2026-10-31：修正公式 provider 对 `\text` 的上下文继承：段落语言、嵌套粗斜体及字体环境通过 `caret-layout` anchor 传入；普通 `Text` 真实塑形资源与正文 run 对照，显式 `Rm` 继续映射为数学正文字体。真实空树使用有宽度槽位；不支持树、几何溢出、负总 advance、负 ascent/descent 和后端初始化失败生成带根几何的占位快照与诊断，不再整文失败或把可能过期的 `MathTree.source` 当作当前树。页面对象现保留 Exactly 行距裁切和 run/公式交错顺序。`SessionCheckpoint` 拒绝跨会话恢复，并用于文本编辑、命令、撤销/重做及 IME 提交的布局失败回滚；增加 app 级有效文档布局失败注入用例，验证文档、选择、session ID、generation、快照和 undo/redo 行为。checkpoint 仍完整克隆 Session/历史和页面快照，热路径基准与轻量事务方案尚待完成。页面公式绘制、命中/焦点、RTL/混合方向和 parser spans 仍未完成。
- 2026-10-31：checkpoint 始终备份而不移走活动 composition 预览，使成功路径生成响应帧时仍能读取预览；失败路径可恢复已备份状态。Session 无文档变化的失败不推进 generation，曾有中间代文档修改的失败回滚则推进 generation 以拒绝迟到结果，并重绑定恢复的 preview 源身份与对主文档的发布身份。普通段落复用要求 generation 恰好相邻且 Session 签发的变更段落一致；composition 候选采用不可变封闭对象、独立预览身份与交互 revision，安装校验 owner/generation/revision，防止倒序候选或 selection 漂移覆盖当前状态。布局复用、检查点、IME 返回帧保留和 page rasterizer 明确拒绝公式页的定向用例均已复验通过。当前 page rasterizer 对有公式的页面返回 `layout.glyphs.unsupported_inline_box`，QML 现逐页记录失败并保留其他成功页帧，但尚未把 math glyph、rule、`\text` 正文片段合成到既有 glyph frame，公式页仍无可见回退提示，且没有 Qt Quick 行为测试；这仍是阻塞项，不能视作画布支持。性能基线、parser 细粒度 source spans、块 Equation/Align 文档布局、命中/MathFocus、RTL/混排仍未完成。`page_glyphs.rs` 目前读取 Parley positioned item 以识别对象顺序；这是 engine 对 Parley 布局细节的真实耦合，需在页面绘制接通前评估是否把稳定的有序绘制投影移至 `caret-layout`。

- 2026-10-31：Terra/medium 对渲染边界的只读复核建议将 inline math 画布接入作为完整切片：`caret-layout` 输出稳定的本地 glyph-run 几何，engine 按公式 provider 资源表做页面定位并展开公式原语，FFI 传输经验证的有序 DTO，QSG 以混合顺序节点处理 glyph、矩形与线段并统一应用页面/行裁切。`ExternalText` 保留真实 Parley font face、轴、synthesis 和局部 glyph y；STIX 数学字形使用固定 Blob。缓存键需含 session/document/font generation、缩放、DPR 相位、主题色；等宽公式替换也要使受影响页失效。失败页→恢复和逐页保留仍缺 Qt Quick 行为测试；公式绘制、源回退提示、命中/MathFocus 尚未完成。

- 2026-10-31：新增 `caret-layout::PositionedParagraphDrawItem` 稳定投影，将 Parley GlyphRun/InlineBox 按视觉顺序转换为 Caret glyph run 与对象 ID；engine `page_glyphs` 不再解释 Parley positioned enum。基线从 Parley 全局行坐标转换到 Caret 行局部 f64，保持长段落组合附标偏移；11 个 page glyph 行为测试与定向正文资源投影、layout/engine Clippy 通过。公式 primitives/FFI/QSG 和命中/焦点仍未实现。

## 完成摘要

未完成。
