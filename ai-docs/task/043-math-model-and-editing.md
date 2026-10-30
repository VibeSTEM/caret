# 043 — M3：公式文档模型与编辑

- 状态：in-progress
- 依赖：010、042
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-10-31

## 目标与背景

为可编辑公式建立版本化 Caret 数学树，并实现公式创建、内部编辑、位置和键盘导航。公式树属于 `.cdoc` 稳定文档模型；`latex-rust` AST 只能存在 `caret-math` 适配边界。043 创建低层 `caret-math` parser crate，并更新 Rust 分级与依赖图；044 在此基础上扩展布局 API。总体验收由 [011 M3 公式](011-impl-math.md)管理。

## 必读

- [公式与表格](../architecture/editor/math-and-table.md)、[文档模型](../architecture/editor/document-model.md)、[结构导航](../architecture/editor/structure-navigation.md)
- [命令输入](../architecture/editor/command-input.md)、[快捷键](../architecture/interface/keyboard-shortcuts.md)
- [存储格式](../architecture/editor/document-model/document-storage.md)、[测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

实现 `InlineMath` 段内原子对象、块级 `Equation` 单行显示公式、块级 `Align` 多行对齐公式的 Caret 持久化类型、节点位置、占位符和公式内 `MathPos`。首版无自动编号、无 `\label` / `\ref`；支持的分隔符、行分隔和失败保留规则以 011 为准。保留 `defaultMathFontFamily`，新文档默认 STIX Two Math，首版只支持该字体。

本任务不实现 Qt 绘制和大范围布局优化，不写入第三方 AST，不实现表格行列编辑。

## 前置条件与待决策

010 结构导航和 042 后端验证需完成。`caret-math` 位于 Rust L1，不依赖其他 workspace crate；`caret-engine` 到 `caret-math` 的允许边已登记，`caret-doc` 的持久化 schema 不依赖它或第三方类型。043 在 parser/Token/AST 边界提供源码字节范围与预处理偏移映射，供失败导入定位；成功导入后以 Caret 树为权威。失败导入保留原公式源与可定位错误；成功解析/规范化后是否保留原始字节由 schema/导出契约决定，不假设源字符串与 Caret 树一一对应。布局盒到 Caret 数学节点的 provenance 由 044 实现，与 parser 源码范围分开。

## 实施步骤

1. 更新 Rust crate 分级和依赖图，在 L1 创建不依赖 workspace crate 的 `caret-math` parser 边界；为 parser/Token/AST 增加源码字节范围及预处理偏移映射。`caret-engine` 调用它解析后转换为 Caret 自有 schema，schema 不存第三方 AST。
2. 定义 schema 版本、`InlineMath`/`Equation`/`Align` 序列化表示、稳定 ID、源片段保存方式、MathPos 和 32 层树深度/输入长度上限；验证 Caret 树与 parser 递归预算的转换边界，比较树节点布局并测量内存占用。
3. 实现公式插入命令、公式内部 AST 编辑、占位符导航、左右上下移动、退出公式与错误恢复；LaTeX 命令由 `caret-math` parser 处理，不触发正文命令模式，`\\` 仅在 `Align` 中表示公式行分隔。
4. 接入原子事务与撤销/重做；解析或编辑失败不部分更改正文，未支持命令保留原输入并定位错误。
5. 覆盖新建、编辑、保存重开、旧/未知字段、异常深度、跨段选区边界和位置映射的行为测试。
6. 更新 011 与架构文档中的具体 schema/迁移决策，独立评审后复验。

## 预计改动

新增 `crates/caret-math`（L1 parser 边界）；调整分级/依赖图；`crates/caret-doc` 数学节点和验证/序列化，`crates/caret-engine` 公式命令与 MathPos/导航，`caret-app` 只在存储或命令边界确有需要时调整。044 扩展 `caret-math` 的 layout API。

## 验收标准

- [ ] 三种公式类型在 `.cdoc` 中为 Caret 自有 schema，往返后节点、源内容、ID 与位置语义稳定。
- [ ] 数学树避免每个符号单独分配字符串与堆对象；使用典型、深层及大量公式样本记录节点数、堆分配数和驻留内存后再确定紧凑布局。
- [ ] 自动编号、编号版 `equation`/`align`、label/ref 明确不支持且不会静默丢失源文本或编号语义。
- [ ] 键盘进入、内部移动、占位符跳转、离开和插入点落位符合 011。
- [ ] 失败原子；超深/过长输入不能导致进程崩溃；未知 `defaultMathFontFamily` 原样保留，实际回退到 STIX 时有诊断。
- [ ] 表格内公式可以由稳定块/行内结构表示，但与表格的可视集成留给 044。
- [ ] 命令级/模型行为测试、格式和 lint 通过；独立评审通过。

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
| —         | 高节点数预算与合并后故障注入回滚测试    | 预算边界及失败后正文/选区/历史保持不变  | 未执行                                               |
| 2026-10-31 | `cargo test --locked --offline -p caret-doc -p caret-engine` | MathTree payload/内容预算、导入返回校验、编辑后端节点/深度预算 | 通过：`caret-doc` 44 项、`caret-engine` 224 项；不覆盖 Session 数学焦点、撤销/重做、公式导航或整公式回写 |
| 2026-10-31 | `cargo lint --check` | workspace lint、依赖、CMake/QML/native 检查 | 未完整通过：第 1 阶段 Clippy 和第 2 阶段未使用依赖检查通过；第 4 阶段 CMake layering fixture 启动 `/usr/bin/c++` 探测时 ABI 配置失败，检查提前退出；第 3 阶段通过，5–9 阶段未运行 |
| 2026-10-31 | `cargo fmt --all -- --check`、`git diff --check HEAD` | Rust 格式与补丁空白检查 | 通过 |

## 风险与回退

公式 schema 一旦发布需通过能力标识和版本迁移保持兼容；先验证必要字段和未知能力策略，再稳定持久化形式。布局器的库 AST 变化不能要求 `.cdoc` 迁移。

## 决策与工作记录

- 2026-10-29：从 011 拆出文档模型、编辑和导航任务；以 020/011 的产品语义为准。
- 2026-10-31：新增 L1 `caret-math`，锁定 `latex-rust 3.0.1`，执行 1 MiB 源码限制、32 层 parser 预算，确保 `Align` 仅接收 `aligned` 树，并通过 AST 检查拒绝自动编号环境以及嵌套的 `\\label`、`\\ref`、`\\tag`、`\\nonumber` 和 `\\notag`。失败对象借用并保留原始输入，当前位置范围目前只能精确到整段公式。
- 2026-10-31：`latex-rust 3.0.1` 不暴露 token/AST 字节范围；细粒度源跨度及预处理映射仍需上游 adapter patch，本阶段没有自行实现第二套 tokenizer/parser。解析结果到 Caret 数学树的转换、MathPos 与公式编辑事务尚未实现。
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

未完成。
