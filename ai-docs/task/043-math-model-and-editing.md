# 043 — M3：公式文档模型与编辑

- 状态：done
- 依赖：010、042
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-10-31

## 目标与背景

为公式建立版本化 Caret 数学树，并实现基础 `InlineMath` Session 编辑、位置和键盘导航。三种公式都具有持久化 Caret schema 和 parser 导入；显示公式/Align 的交互式创建与编辑、上下标命令及已提交内容的删除需等 044 接通可见布局和 caret 投影后开放。公式树属于 `.cdoc` 稳定文档模型；`latex-rust` AST 只能存在 `caret-math` 适配边界。043 创建低层 `caret-math` parser crate，并更新 Rust 分级与依赖图；044 在此基础上扩展布局及 adapter API。总体验收由 [011 M3 公式](011-impl-math.md)管理。

## 必读

- [公式与表格](../architecture/editor/math-and-table.md)、[文档模型](../architecture/editor/document-model.md)、[结构导航](../architecture/editor/structure-navigation.md)
- [命令输入](../architecture/editor/command-input.md)、[快捷键](../architecture/interface/keyboard-shortcuts.md)
- [存储格式](../architecture/editor/document-model/document-storage.md)、[测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

实现 `InlineMath` 段内原子对象、块级 `Equation` 单行显示公式、块级 `Align` 多行对齐公式的 Caret 持久化类型及 parser 导入；为 `InlineMath` 实现 Session 创建、文本/分数/根式插入和 `MathPos` 导航。`Equation`/`Align` 的交互式创建编辑、上下标插入、已提交节点删除和 app 命令在 044 显示布局可用后接入。首版无自动编号、无 `\label` / `\ref`；支持的分隔符、行分隔和失败保留规则以 011 为准。保留 `defaultMathFontFamily`，新文档默认 STIX Two Math，首版只支持该字体。

本任务不实现 Qt 绘制和大范围布局优化，不写入第三方 AST，不实现表格行列编辑。

## 前置条件与待决策

010 结构导航和 042 后端验证需完成。`caret-math` 位于 Rust L1，不依赖其他 workspace crate；`caret-engine` 到 `caret-math` 的允许边已登记，`caret-doc` 的持久化 schema 不依赖它或第三方类型。当前锁定的 parser 只提供整段公式 source range，失败输入保留原文并能定位到整段；细粒度 token/AST 字节范围与预处理偏移映射需在 044 的 adapter patch 中补齐，不作为 043 已完成能力。成功导入后以 Caret 树为权威，schema 保留原始公式源；布局盒到 Caret 数学节点的 provenance 由 044 单独实现。

## 实施步骤

1. 更新 Rust crate 分级和依赖图，在 L1 创建不依赖 workspace crate 的 `caret-math` parser 边界；`caret-engine` 调用它解析后转换为 Caret 自有 schema，schema 不存第三方 AST。当前仅记录整段公式源范围。
2. 定义 schema 版本、`InlineMath`/`Equation`/`Align` 序列化表示、稳定 ID、源片段保存方式、MathPos 和 32 层树深度/输入长度上限；验证 Caret 树与 parser 递归预算的转换边界，比较树节点布局并测量内存占用。
3. 实现 `InlineMath` 插入命令、公式内部树编辑、占位符导航、左右上下移动、退出公式与错误恢复；Equation/Align 的交互命令留给 044。LaTeX 输入由 `caret-math` parser 处理，不触发正文命令模式，`\\` 仅在 `Align` 中表示公式行分隔。
4. 接入原子事务与撤销/重做；解析或编辑失败不部分更改正文，未支持命令保留原输入并定位错误。
5. 覆盖新建、编辑、保存重开、旧/未知字段、异常深度、跨段选区边界和位置映射的行为测试。
6. 更新 011 与架构文档中的具体 schema/迁移决策，独立评审后复验。

## 预计改动

新增 `crates/caret-math`（L1 parser 边界）；调整分级/依赖图；`crates/caret-doc` 数学节点和验证/序列化，`crates/caret-engine` InlineMath 命令与 MathPos/导航。Equation/Align 命令、app 焦点投影和 adapter layout API 由 044 接入。

## 验收标准

- [x] 三种公式类型在 `.cdoc` 中使用 Caret 自有 schema；保存重开保留节点、源内容、ID 与位置语义。
- [x] 普通连续 `ord` 字符合并为一个 Text-bearing node，MathNode 子节点存储在父节点 `Vec` 中而不是逐节点 `Box`；典型、深层和 1,000 个公式样本已记录节点数、堆分配数及内存，并决定本阶段不额外建立运行时 arena。后续若真实公式文档显著超过 1,000 个，或完整布局接入后 RSS 超预算，再评估 schema v2/arena。
- [x] 自动编号、编号版 `equation`/`align`、label/ref 明确不支持；parser 拒绝时保留原公式源，不静默丢弃编号语义。
- [x] 插入行内公式后 Session 进入 MathFocus；支持文本/分数/根式插入、公式内左右/上下移动、Tab 跳转后续占位符、离开和正文插入点落位。末尾 Tab 不循环，容器边界有效 MathPos 可导航，暂存输入下拒绝 Tab 且保留状态。
- [x] 解析/编辑失败原子；超深/过长输入受预算限制；未知 `defaultMathFontFamily` 值在文档模型中原样保留。
- [x] 命令级/模型行为测试、格式和 lint 通过；独立评审通过。

## 后续任务交接

- [044：公式布局、画布渲染与表格集成](044-math-layout-and-rendering.md) 承接显示公式/Align 的交互式创建与编辑、`^`/`_` 上下标插入、已提交节点的 Backspace/Delete、从正文方向键重新进入既有公式焦点、细粒度 parser source range adapter patch，以及未知 `defaultMathFontFamily` 回退到 STIX 时的诊断和公式在表格中的视觉集成。
- [045：表格模型与交互](045-table-model-and-editing.md) 验收单元格通用块序列及公式节点的保存重开；044 只接手该单元格内容的视觉和交互集成。

## 验证计划与结果

| 日期       | 环境 / 命令                            | 预期                                   | 实际结果                                             |
| ---------- | -------------------------------------- | -------------------------------------- | ---------------------------------------------------- |
| 2026-10-31 | `cargo test -p caret-math --locked --offline` | Parser 适配、后端无关语法树、深度/节点/长度边界、错误源保留 | 通过，16 项单元测试；尚未覆盖语法树到持久化 Caret schema 的转换 |
| 2026-10-31 | `cargo test --locked --offline -p caret-doc -p caret-engine` | 数学属性安全预览、模型校验、行内/显示/Align 转换及 `.cdoc` 往返 | 通过：`caret-doc` 43 项、`caret-engine` 200 项；Align cell 源码范围仍不可用 |
| 2026-10-31 | `cargo clippy --locked --offline -p caret-math --all-targets -- -D warnings`、`cargo fmt --all -- --check` | crate lint 与格式 | 通过 |
| 2026-10-31 | `cargo test --locked --offline -p caret-doc -p caret-engine` | 数学 DTO、保存重开、正文原子偏移、Enter 与历史状态 | 通过：`caret-doc` 42 项、`caret-engine` 194 项 |
| 2026-10-31 | `cargo check --locked --offline --workspace` | 所有 crate 能识别新增持久化节点 | 通过；数学绘制和块级布局仍由 044 实现 |
| 2026-10-31 | `cargo lint --check` | Rust、依赖、分层、CMake/QML/native lint | 通过全部 9 个检查阶段 |
| 2026-10-31 | `CARGO_HOME=/private/tmp/caret-cargo-audit-home cargo-deny --offline check` | 新增依赖后的 advisories、版本、许可与来源审计 | 通过；使用本地 RustSec 数据库快照 `2295466`（2026-10-30），当前沙箱无法刷新在线数据库 |
| 2026-10-31 | `cargo lint --check` | crate lint、依赖和分层、CMake/QML/native lint | 通过（最终代码改动后复跑） |
| 2026-10-31 | `cargo test --locked --offline -p caret-doc -p caret-engine` | typed 属性与树形校验、导入、矩阵单元格边界及存储往返 | 通过：`caret-doc` 44 项、`caret-engine` 201 项 |
| 2026-10-31 | `cargo test --locked --offline -p caret-engine` | MathPos 路径与节点偏移边界原语 | 通过：205 项；不覆盖公式内导航、编辑事务、命中或修改后的路径映射 |
| 2026-10-31 | `cargo check --locked --offline --workspace`、`cargo lint --check`、`cargo fmt --all -- --check`、`git diff --check HEAD` | MathPos 导出、MathNode 文本约束及全仓构建/lint/格式 | 通过：workspace check 与 lint 9 阶段通过 |
| 2026-10-31 | `cargo check --locked --offline --workspace`、`cargo clippy --locked --offline -p caret-doc -p caret-engine --all-targets -- -D warnings`、`cargo lint --check`、`cargo fmt --all -- --check`、`git diff --check HEAD` | 全工作区构建、直接消费者 lint、分层/native/QML 与格式检查 | 通过；cargo lint 9 阶段全部完成 |
| 2026-10-31 | `cargo test --locked --offline -p caret-doc -p caret-engine` | Session 插入、MathFocus 输入/撤销重做、分数槽、失败原子性、空 Run 元数据、组合符边界、环境内公式删除及正文/IME 隔离 | 通过：`caret-doc` 44 项、`caret-engine` 231 项；数学候选因 app 尚无公式绘制/焦点投影保持禁用，MathPos 键盘导航仍未覆盖 |
| 2026-10-31 | `cargo clippy --locked --offline -p caret-doc -p caret-engine --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check HEAD` | Session 数学编辑实现 lint、格式和补丁检查 | 通过 |
| 2026-10-31 | `cargo check --locked --offline -p caret-app` | 确认新增 engine 命令不会破坏直接应用消费者 | 通过 |
| 2026-10-31 | `cargo test --locked --offline -p caret-doc -p caret-engine`、`cargo clippy --locked --offline -p caret-engine --all-targets -- -D warnings`、`cargo check --locked --offline -p caret-app`、`cargo fmt --all -- --check`、`git diff --check HEAD` | 公式内左右/上下移动、分数/根式槽切换、边缘退出与直接消费者编译 | 通过：`caret-doc` 44 项、`caret-engine` 233 项；目标 Clippy、app check、格式及 diff 检查通过 |
| 2026-10-31 | `cargo test --locked --offline -p caret-doc -p caret-engine`、`cargo clippy --locked --offline -p caret-doc -p caret-engine --all-targets -- -D warnings`、`cargo check --locked --offline -p caret-app`、`cargo fmt --all -- --check`、`git diff --check HEAD` | Parser-backed math 输入、原始 source 溯源、固定槽嵌套结构、插入点与导航修复 | 通过：`caret-doc` 44 项、`caret-engine` 241 项；Clippy、app check、格式及 diff 检查通过 |
| 2026-10-31 | `cargo test --locked --offline -p caret-doc -p caret-engine`、`cargo clippy --locked --offline -p caret-doc -p caret-engine --all-targets -- -D warnings`、`cargo check --locked --offline -p caret-app`、`cargo fmt --all -- --check`、`git diff --check HEAD` | Parser-backed math 输入与结构插入、普通符号中间位置拆分、固定槽与组合字素、未知扩展保护、原始 source 溯源、插入点与导航 | 通过：`caret-doc` 44 项、`caret-engine` 247 项；Clippy、app check、格式及 diff 检查通过 |
| 2026-10-31 | `cargo test --locked --offline -p caret-math -p caret-doc -p caret-engine`、`cargo clippy --locked --offline -p caret-math -p caret-doc -p caret-engine --all-targets -- -D warnings`、`cargo check --locked --offline -p caret-app`、`cargo fmt --all -- --check`、`git diff --check HEAD` | 逐键 LaTeX 前缀缓冲、命令名前缀歧义、错误可见性、长度预算原子性、Backspace/Escape 恢复、粘贴历史与单次撤销提交 | 通过：`caret-math` 17 项、`caret-doc` 44 项、`caret-engine` 255 项；Clippy、app check、格式及 diff 检查通过 |
| 2026-10-31 | `cargo test --locked --offline -p caret-math -p caret-doc -p caret-engine`、`cargo clippy --locked --offline -p caret-math -p caret-doc -p caret-engine --all-targets -- -D warnings`、`cargo check --locked --offline -p caret-app`、`cargo fmt --all -- --check`、`git diff --check HEAD` | parser 三态输入分类与错误优先级、未知命令/尾反斜线拒绝、定界符完整控制词/顺序、退格修复合法输入并进入撤销历史 | 通过：`caret-math` 17 项、`caret-doc` 44 项、`caret-engine` 257 项；Clippy、app check、格式及 diff 检查通过 |
| 2026-10-31 | `cargo test --locked --offline -p caret-engine parser_expansion_budget_failure_keeps_session_document_and_history_atomic -- --exact editing::session::tests::parser_expansion_budget_failure_keeps_session_document_and_history_atomic` | Parser 展开节点预算临界值上的合并失败；确认正文、选区、MathFocus、历史和 generation 不变 | 通过，1 项；该预算边界失败路径已验证 |
| 2026-10-31 | `cargo test --locked --offline -p caret-math -p caret-doc -p caret-engine --quiet`、`cargo clippy --locked --offline -p caret-math -p caret-doc -p caret-engine --all-targets -- -D warnings`、`cargo check --locked --offline -p caret-app`、`cargo fmt --all -- --check`、`git diff --check HEAD` | 输入错误恢复与 parser 展开预算临界失败的完整目标验证 | 通过：`caret-math` 17 项、`caret-doc` 44 项、`caret-engine` 258 项；Clippy、app check、格式及 diff 检查通过 |
| 2026-10-31 | `cargo build --locked --offline --release -p caret-engine --example math_tree_memory`；独立运行 `typical`、`deep`、`many` | Caret 节点布局、导入分配调用/字节、保留堆字节与峰值 RSS | 通过；结果与决策见下方工作记录 |
| 2026-10-31 | `cargo test --locked --offline -p caret-engine math_tab -- --nocapture` | MathFocus 下 Tab 移至后续参数槽、嵌套分支与容器边界排序、末尾不循环；暂存输入拒绝导航且保留状态 | 通过，3 项；全公式布局投影仍由 044 负责 |
| 2026-10-31 | `cargo test --locked --offline -p caret-math -p caret-doc -p caret-engine --quiet`、`cargo clippy --locked --offline -p caret-engine --all-targets -- -D warnings`、`cargo check --locked --offline -p caret-app`、`cargo format --check`、`git diff --check HEAD` | 最终三 crate 行为回归、Tab 路由 lint、应用消费者和格式检查 | 通过：`caret-math` 17 项、`caret-doc` 44 项、`caret-engine` 261 项；Clippy、app check、格式与 diff 检查通过 |
| 2026-10-31 | `cargo lint --check` | 全工作区 9 阶段 Rust、依赖、CMake、QML/native lint | 通过，9/9 阶段 |
| 2026-10-31 | Terra/medium 数学模型领域只读复核；Sol/medium 最终完整 diff 只读复核 | 检查状态/结构边界、Tab 占位符顺序与预算、错误原子性、性能口径、任务交接与最终差异 | 通过；修复的发现均已复验，无未解决阻断项 |
| 2026-10-31 | `cargo test --locked --offline -p caret-doc -p caret-engine` | MathTree payload/内容预算、导入返回校验、编辑后端节点/深度预算 | 通过：`caret-doc` 44 项、`caret-engine` 224 项；不覆盖 Session 数学焦点、撤销/重做、公式导航或整公式回写 |
| 2026-10-31 | `cargo lint --check` | workspace lint、依赖、CMake/QML/native 检查 | 首次运行受 `/usr/bin/c++` ABI 探测失败影响；后续全仓复验通过，见上方最新记录 |
| 2026-10-31 | `cargo fmt --all -- --check`、`git diff --check HEAD` | Rust 格式与补丁空白检查 | 通过 |

## 风险与回退

公式 schema 一旦发布需通过能力标识和版本迁移保持兼容；先验证必要字段和未知能力策略，再稳定持久化形式。布局器的库 AST 变化不能要求 `.cdoc` 迁移。

## 决策与工作记录

- 2026-10-29：从 011 拆出文档模型、编辑和导航任务；以 020/011 的产品语义为准。
- 2026-10-31：新增 L1 `caret-math`，锁定 `latex-rust 3.0.1`，执行 1 MiB 源码限制、32 层 parser 预算，确保 `Align` 仅接收 `aligned` 树，并通过 AST 检查拒绝自动编号环境以及嵌套的 `\\label`、`\\ref`、`\\tag`、`\\nonumber` 和 `\\notag`。失败对象借用并保留原始输入，当前位置范围目前只能精确到整段公式。
- 2026-10-31：`latex-rust 3.0.1` 不暴露 token/AST 字节范围；细粒度源跨度及预处理映射仍需上游 adapter patch，本阶段没有自行实现第二套 tokenizer/parser，后续由 044 承接。解析结果到 Caret 数学树的转换、MathPos 与基础 InlineMath 编辑事务已在本任务完成。
- 2026-10-31：`caret-doc` 加入版本 1 的自有数学树与 `InlineMath`、`Equation`、`Align` DTO；math 节点范围、深度、源码长度和文档全局 ID 在保存入口统一校验，三种节点通过独立 `.cdoc` 能力标记声明。已补保存重开测试。`caret-layout` 暂以原子占位符表达行内公式，块级公式布局返回未支持；正式数学布局、MathPos、解析器到 Caret 树的转换及事务编辑待续。
- 2026-10-31：`caret-math` 增加独立模块中的临时 `MathSyntaxTree`，不暴露 `latex-rust` 类型，覆盖当前 parser AST 节点、矩阵行/单元格、文本样式和可选槽位。迭代式预算检查按转换后的层级计入矩阵行、intertext 和占位节点，限制 32 层及 65,536 节点；尺寸保留精确有理数，定界符编码保留字符与命名类型差别。尚未提供细粒度源跨度。
- 2026-10-31：`caret-engine` 增加 parser 结果到 `caret-doc` schema 的导入桥，创建文档 ID，并将 kind 和受校验的参数写入正式 `MathNode.kind/attributes`；新语义要求 `caret.math.syntax.v1`，未知 kind/属性值转只读安全预览。连续同类普通原子合并后才创建持久化 ID。InlineMath、Equation 和普通 Align 行已转换并通过 `.cdoc` 往返；Align 完整源放在容器，因 3.0.1 无 cell 源范围，cell 树当前源码为空；含环境级 `intertext`/`hline` 行会明确拒绝，错误携带类别、完整源及整公式字节范围。紧凑 arena 与典型/大量文档内存测量仍未完成。
- 2026-10-31：独立 schema/import 复核后，typed 数学属性补齐必需字段与 delimiter allowlist，schema 校验同时约束节点文字、操作数槽位和 Hline/Intertext 的 Matrix 父级；不完整或结构非法数据进入只读预览。Ord 字符序列才可合并，Bin/Rel/Punct 等原子保留节点边界，Matrix 行不跨 cell 合并；新增矩阵单元格回归和无效 schema 预览测试。能力扫描改为迭代遍历，避免保存前递归栈风险；所有导入错误都携带完整输入和 parser 可提供的范围。权威 schema 文档现列出节点槽位、必需属性和 Ord 合并含义。复验 `caret-doc` 44 项、`caret-engine` 201 项、workspace check、目标 clippy、全 lint 和格式检查通过。
- 2026-10-31：Sol/medium 最终只读复核曾指出 MathNode 可伪造自动编号 matrix style；移除 `align/equation/gather/multline`，补充只读预览用例和 schema 文档 allowlist。最终复核通过，未发现阻断问题；最终改动后重跑 `caret-doc` 44 项、`caret-engine` 201 项、目标 clippy、格式和 diff 检查通过。workspace check 与全 `cargo lint --check` 在 allowlist 变更前已通过；本次仅改变 Rust 枚举 allowlist、文档和安全预览测试。
- 2026-10-31：首版 `MathPos` 在 `caret-engine` 提供稳定 NodeId 路径解析和节点偏移校验：Ord/TextContent 按本叶 extended grapheme cluster，命名数学/文本符号按单原子 0/1 边缘，结构节点按子节点边界，Placeholder 固定 offset 0；拒绝空/失配路径和路径上的重复兄弟 ID。保留分数等结构操作数两侧各自的位置。公式内部移动、命中、结构编辑后的映射、线性 Row 等价位置规范化、跨叶组合字素与 bidi 视觉 affinity 仍未实现。`caret-engine` 205 项测试通过，覆盖 UTF-8/字素边界、分数两槽端点、命名符号、占位符及重复 ID。
- 2026-10-31：Terra/medium schema 与编辑边界只读复核发现并促成修复：未知 `sourceRange` 字段往返保留；Align 行进入文档节点预算；公式能力登记；未来 schema/节点 kind 转安全预览；U+FFFC 按 UTF-8 字节计长；公式边界可插软换行；跨段 Enter 失败恢复文档、选区、索引和缓存；数学扩展字段纳入历史大小估算。复验覆盖 schema 源码/深度限制、三种数学节点未来 schema、公式后的跨段 Enter、历史扩展字段估算。仍未验证高节点数预算裁剪及实际 MathPos/布局交互。
- 2026-10-31：新增无副作用 `apply_math_edit` 原子编辑原语，支持 Row/普通 Ord/TextContent/参数槽的文本插入，以及 Row 或 Placeholder 中插入分数、根式；候选树在克隆前及返回前执行 schema 深度/节点/payload 预算验证，失败不改变输入。输入须由 `latex-rust` 解析为普通 Ord 且拼接后与原字节完全一致，避免定界符、空白、命令归一化被误存为字符；编辑后的源与范围按导入溯源保留，不代表当前编辑树的 LaTeX。覆盖累计/单次文本预算及插入后组合字素光标边界。尚未接入 Session 命令、历史状态或键盘导航，结构插入限制在 Row/Placeholder，不能据此宣称公式交互完成。
- 2026-10-31：追加编辑预算交叉检查：Ord 字符合并后按 parser 展开节点计数，多字符 Ord 结构操作数计入必要 Row 包装，防止数学树可保存但超过锁定 parser 的 65,536 临时节点/32 层上限；文档 payload 预算也包含 ID、属性、sourceRange 与未知扩展值，并独立限制节点文字总量为 1 MiB，限制超长字段及多次 TextContent 插入在编辑克隆前放大内存。schema v1 尚随 043 保持未合并、未发布，故本次在稳定前确立每公式 65,536 节点/8 MiB payload 上限；若之后需提高，必须新增 schema/能力迁移，不改变已发布 v1。扩展 JSON 深度受文档最大嵌套限制；导入桥在返回前校验持久化树，避免解析成功后才在保存时失败。`atomClass` 保持 parser 导入时的权威分类；存储层不复制字符分类规则。
- 2026-10-31：对齐模型能力边界：编辑树仍执行 parser 结构展开节点/深度预算，但当前没有 Caret 树到 LaTeX 的序列化器；保留原始 source 与 sourceRange 作为溯源，节点文本限额不声称等价于含命令、定界符和转义后的 1 MiB parser 源码上限。当前编辑基础尚不能满足整公式导出或 Session 交互验收。
- 2026-10-31：Session 增加 InlineMath 插入、受限数学文本/分数/根式事务、MathFocus 历史状态、输入合并和 Escape 退出；正文 Selection 停在 U+FFFC 右侧，Undo/Redo 恢复公式树与 MathPos。数学焦点下正文移动/删除/选区/IME/命令面板暂返回 UnsupportedMathTarget，避免编辑落到公式外或焦点悬空；command registry 中 `\\math`、`\\frac`、`\\sqrt` 仍不可用，因为当前 app/layout 只显示 U+FFFC，尚无公式内容及内部光标投影，直接开放会产生不可见编辑。TextMap 将 Math 对象两侧作为强制字素边界，并保留空 Run 字段。新增数学节点时暂克隆并校验完整 Document 以维护全局节点预算；已有数学叶内追加文本不触发全文校验，但整体成本仍未量测。内部左右/上下导航、删除参数槽、公式命令输入和跨层焦点投影仍待实现。
- 2026-10-31：增加 MathDirection 公式内左右/上下定位：左右沿有序可编辑位置移动并在公式左右边缘分别退出到 U+FFFC 前/后；上下在 Fraction 参数槽及 Radical 指数/被开方数槽之间移动，MathFocus 变更通过 selection_changed 通知调用方。普通文本叶偏移使用 grapheme 边界；命名/非 ord 原子仅提供 0/1 两端；在 Row 原子边缘、普通 `ord` 符号叶的内部字素边界及固定 Fraction/Radical 槽中都可插入 parser chunk 或结构命令，必要时拆分或用 Row 包装并保留已有节点 ID；拆分遇到节点未知扩展或带未知范围扩展时拒绝。每个完整数学输入 chunk 交给 `caret-math` 解析并通过 `math_import` adapter 转成 Caret 节点，因此能保留 parser 原子分类并接受完整 `\\alpha` / `\\frac{a}{b}` chunk。`MathTree.source` 保持导入时的原始源码；新增解析节点及修改过的叶清除无法映射的 `sourceRange`，但范围带未知扩展字段时会拒绝编辑以免静默丢弃；不保存编辑后源码或逐键源日志。逐键 LaTeX 命令缓冲/未完成输入恢复还未实现，逐键 `\\alpha` 目前会在不完整前缀时失败；app 侧数学候选仍禁用，直到 044 接通公式渲染和焦点投影。Session 以编辑前后公式节点数差决定是否全量校验 Document 预算；增加节点时仍需克隆并校验完整文档，性能尚未量测。深公式导航成本、重复可视边界规范化、bidi 侧边和跨层 caret 同步仍待测量或实现。

## 完成摘要

- 2026-10-31：接入逐键公式输入缓冲：不完整反斜线命令和开放参数组暂存于 Session，不改树或 history；控制词等到非字母分隔符再解析，避免 `\\sin` 抢先于 `\\sinh`；parser 接受完整 chunk 后一次提交。Backspace 按 grapheme 删除暂存尾部并重算未完成状态，Escape 优先清除缓冲，Undo/Redo 首次清缓冲且不回退文档；语法错误原文保留，effect 暴露 `math_input_error`，Session getter 提供输入文本。长度超限返回错误且保持先前缓冲/error 状态不变；非语法编辑错误仍按原错误返回且恢复先前缓冲。数学粘贴走统一事务和独立撤销历史；无 MathFocus 时数学输入命令拒绝且不创建缓冲。对 parser 拒绝的输入仅使用 `caret-math` 边界分类识别可能前缀，语义仍由锁定 parser 判定。新增 `\\alpha`、逐键 `\\frac{x}{y}`、命令名歧义、粘贴历史、超限原子性和输入错误恢复测试。
- 2026-10-31：独立复核后将 `caret-math` 输入状态显式分成 Complete/Incomplete/Invalid；Unknown/Unsupported 命令不会因闭合或开放花括号被误判为可补全，尾部控制词与反斜线只有此前缀本身可解析或明确不完整时才等待。UnmatchedDelimiter 按控制词边界、转义反斜线及左右顺序检查，避免把 `\\leftarrow` 当 `\\left`。Backspace 将错误缓冲修正为合法公式时，通过 `Session::run(InsertMathText)` 提交到文档和撤销历史；添加未知命令、尾部命令词与反斜线优先级、定界符顺序及修复后 Undo 回归。Terra/medium 领域复核已确认修复闭环；保留风险为 parser 的 `Malformed` 文字是输入缺参判定的一部分，及预算边缘的非语法提交故障注入尚未覆盖。
- 2026-10-31：Session 层新增 parser 展开节点预算临界失败用例：构造恰在预算内的 Ord 叶并尝试合并一个字符，验证 `InvalidMathTree` 返回后文档、选区、MathFocus、history id、generation、undo/redo 可用性、merge epoch 与输入缓冲均不变；测试从含预算边界公式的合法 Document 建立 Session，避免修改已有历史快照。单测通过；当时未覆盖大规模文档性能和节点分配测量，后续追加 release 内存探针。
- 2026-10-31：release 数学树导入探针测量于 macOS 27.0 arm64 (Apple T8132), Rust 1.99.0。典型公式为 54 B 源码、23 nodes/深度 6、155 次导入分配/15,889 B 累计、11,319 B 保留堆、6,553,600 B 进程峰值 RSS；10 层嵌套分数为 101 B、21 nodes/深度 11、177 次/19,838 B、16,454 B 保留堆、6,438,912 B RSS；重复典型公式 1,000 次为 23,000 nodes、154,001 次/15,817,000 B、11,271,000 B 保留堆、19,988,480 B RSS。`MathNode` 为 168 B，`NodeId` 为 24 B；根和 child `Vec` 中的节点内联存储，无逐节点 `Box`，普通 `ord` 字符合并；保留堆差值在 parser parse 及首次 import warm-up 后采集，包含外层公式 Vec 容量和内联根节点，不含预解析的 `ParsedMath`。分配字节为 allocator 请求大小，不含其元数据；RSS 为进程启动以来的历史高水位，包含解析与预热阶段，运行间可能波动。此组数字修正了早期未计入外层 Vec 的测量。决定沿用可读、树形 `.cdoc` schema 和 `Vec` 子节点布局，不增加重复 arena；Arena 会增加运行时表示与持久化 DTO 的转换/双份存储，而 1,000 个典型公式当前 RSS 约 19 MiB。完整排版与真实文档数据未进入测量；超过 1,000 公式或 044 布局集成暴露内存预算后再评估 schema v2/arena。
- 2026-10-31：Session 的 `EditorKey::Tab` 在 MathFocus 下遍历 Caret 树，移动到树顺序中的下一个 Placeholder；当前位置与候选节点通过子索引和容器 offset 比较，扫描不枚举普通文字字素、不在每个节点上重新解析 MathPos，仅在命中后复制路径。不存在后续占位符时不循环；暂存 LaTeX 输入时拒绝导航并保留文档、焦点及输入缓冲。测试覆盖普通/嵌套槽、根 Row 非零边界、最后槽和不完整输入。
- 2026-10-31：043 的 schema/parser 导入与基础 InlineMath Session 验收完成；三个 crate 测试 17/44/261 项通过，目标 Clippy、app check、cargo format、cargo lint 9 阶段及 diff 检查通过，Terra 领域复核和 Sol 最终评审通过。Equation/Align 命令编辑、`^`/`_` 上下标、已提交节点删除、正文重新进入焦点和细粒度 parser source range patch 留给 044；未知数学字体回退诊断也由 044 验收，表格通用模型及公式保存往返由 045 验收，见“后续任务交接”。

完成。
