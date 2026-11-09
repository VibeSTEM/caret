# 077 — 数学宿主行框与光标高度统一

- 状态：implemented
- 依赖：044、074
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-09

## 目标与背景

维护者实测发现，数学块宿主高度会随公式内容变化，数学光标也可能随焦点附近的字符或结构高度跳变。期望的默认宿主行框与正文文本一致：普通数学字符不应各自改变行高；上标、分数、根式等结构超出正文基准行框时，按正文行距类型决定扩展或固定推进，不允许为了稳定高度而压缩公式。

问题包含两个相关但独立的几何结果：数学块/行的宿主行框，以及各数学编辑分支的光标矩形。只统一块高度不会自动统一光标高度。行内公式、Equation、Align、MathTree 与 MathText 应使用一致的基础行度量和扩展规则；公式布局仍负责结构内的 x/y 定位、脚本基线、分数槽位和 Align 行/单元格位置。

初步代码审计显示，普通段落由 `caret-layout` 的行框计算结合字体行网格、行距和行内对象确定高度，正文光标消费所属行高度；Equation/Align 当前直接用 MathLayout 的 ascent + descent 作为 MathFragment 高度。MathTree 光标按相邻结构盒生成，MathText 光标按 Parley 文本岛光标盒生成，二者绕过正文 caret 行框。实现前须核实实际行为并确认共享度量的权威边界，不假定当前已有 text-only caret 度量。

## 必读

- [数学模型与排版](../architecture/README.md)
- [分层规范](../standards/layering.md)
- [测试规范](../standards/testing.md)
- [验证与评审](../standards/validation-and-review.md)
- [空公式宿主占位](072-empty-math-host-placeholder.md)
- [数学光标候选与统一仲裁](074-math-caret-routing-and-geometry.md)

## 范围与非目标

- 核对普通文本 caret 高度的权威度量和所有数学 caret geometry 生成路径。
- 复用正文行网格、行距与光标基础高度策略；相同正文基础样式、字号和行距下，数学普通字符使用与正文一致的默认宿主高度，Math/Body 字体角色可不同。
- 数学宿主复用统一表格网格与行框算法：Equation 是单行单格，Align 是多行多格；两者的基础行框来自正文，行/列定位共用同一网格组装入口。
- 数学内容超出基础行框时按正文行距类型扩展或固定推进；上标、下标、分数、根式等保留行距规则允许的结构空间，不为了稳定光标而压缩数学内容。
- 通用行框计算与行列定位放入现有 `caret-grid-layout`；不创建只转发已有能力的 `caret-table` crate。`caret-layout` 和 `caret-math` 通过该共享纯几何层消费同一行度量算法。
- 统一行内公式、Equation、Align、MathTree、MathText 和源码分支的基础光标高度契约，同时保留各数学结构所需的 x/y 位置、脚本定位和行/单元格选择。
- 删除重复的数学专用高度策略；Qt/ViewModel 只消费 Rust 产生的几何，不再引入第二份高度推导。
- 覆盖基线字形、上下标、`\frac`、`\sqrt`、`\text`、空公式占位符及 Align 多行；固定行距必须逐行保持推进并限制行溢出，不能用整块裁切吞掉后续行。
- Equation 作为单行宿主，不为其增加多行编辑语义；已有持久化 SoftBreak 必须保留内容，不能在布局或迁移中静默删除。
- Section、Equation、Align 使用统一块边界停靠逻辑：块前外侧 → 块内起点/终点 → 块后外侧四种状态均可到达；外侧状态不靠预插空段落表示。Left/Right、Up/Down 从段落进入公式时先停在外侧，再次同向进入；离开公式也先停外侧。居中公式块外停靠 caret 位于所在页面内容行的行首/行尾，Align 垂直对齐首/末行。按 Enter、输入文字或提交 IME 才在块同级物化段落。前后已有段落不被改写，插入、焦点和撤销保持原子。
- Equation/Align 通过同一块边界实现接入；块类型只提供 ID、父级路径、页面边界几何和同级插入适配。Session 状态、键盘仲裁、快照光标、IME 预编辑/提交与撤销必须共享实现，后续 block 直接接入同一扩展点，不建立平行状态字段或重复 enter/exit 逻辑。
- Up/Down 从相邻正文视觉行进入公式首/末行，公式垂直边缘到达对应外侧停靠点；相邻行内公式不能成为垂直目标。Equation 单行，Align 按行选择入口/出口。Left/Right 仍可进入相邻行内公式并沿用现有候选逻辑。
- Equation/Align 的前后段落间距复用普通段落相邻间距折叠规则；公式块的 before/after 间距分别与相邻段落 after/before 取较大值，分页换页时不在新页顶部重复前页间距。
- 公式块后的空段落段首按 Backspace 时，删除该空段落并进入公式末端；撤销恢复空段落与原焦点，重做再次进入公式，不删除公式内容。
- 不改变数学结构内部排版、上下方向键候选、光标横向命中、占位符大小或普通文本 caret 行为。
- 不以系统输入法提示或光标闪烁/可见性控制代替几何高度契约。

## 前置条件与待决策

- 审计正文段落实际使用的行框和 caret 高度；结构导航架构只保留稳定行为契约，不记录字段名、实现快照或具体 task 追踪。
- 统一正文基准行框的来源和数学宿主扩展规则；行内公式、Equation 与 Align 使用相同策略，不因公式容器种类复制高度算法。Equation 作为一行一格、Align 作为多行多格，二者走统一网格装配逻辑。
- 数学内容超出正文基准行框时，按[段落行距](../architecture/editor/page-layout.md#行距与默认值)的 Natural、Multiple、AtLeast、Exactly 语义计算行框；不能用焦点字形局部 bounding box 作为整行高度或硬压缩公式。
- 光标高度和宿主行框需分别验证；保留基于 MathTree 结构与 MathText 插入位置的定位信息。调整光标中心或高度时，检查 Up/Down 候选排序和命中阈值。
- 用户补充：数学多行由换行符构成；Enter 可插入额外数学行，但 Backspace 当前不能删除该行边界。Terra 审计发现 Align MathTree Enter 创建结构行，Align Source 插入源码分隔符，MathText Enter 不支持换行，三者不能统称为 `MathContent::SoftBreak`。行边界删除和行合并已拆分到[079 数学多行换行与删除事务](079-math-multiline-break-deletion.md)；本 task 仅在行数变化后验证宿主几何和焦点。

## 实施步骤

1. 绘制正文段落、InlineMath、Equation/Align、MathTree、MathText 和源码分支的宿主行框与 caret 数据流，标出 x、y、基线、行高的唯一来源及重复策略。
2. 增加相同正文基础样式、字号和行距下不同普通数学字符的宿主高度与 caret 高度对比用例，复现字符相关变化；数学字体角色可独立于正文。单独覆盖超出正文基准行框的上标、分数、根式。
3. 扩展现有 `caret-grid-layout`，使其同时拥有纯行框策略和网格定位；`caret-layout` 适配段落行距输入，`caret-math` 对每个顶层 Align 行应用正文基础度量，嵌套 matrix/cases 继续保留数学行距。
4. Equation 单格和 Align 多格共用数学表格构造入口。行身份、cell 路径、命中、渲染、caret、溢出裁切都消费相同的逐行几何；不能在展开为绘制项后按 y 坐标猜回行归属。
5. 抽取或复用正文行框度量作为数学默认值，并按行距类型处理数学内容需求；MathTree、MathText 等 caret 共享基础高度策略，但保留结构定位。
6. 覆盖 inline、equation、align、空宿主、MathTree、MathText、源码及不同数量行的几何，不负责定义行边界编辑事务；用现有 Enter/Undo 路径或独立多行 fixture 验证行数变化，覆盖不同字体/字号/行距、对象溢出、分页、相邻块和 Up/Down 候选。
7. 抽取 Section 已有的外侧块边界状态为所有 block 共用的一套状态/转换/段落物化逻辑；Section、Equation、Align 接入相同入口，block 只实现必要的几何和同级插入适配。覆盖四方向、首尾边缘、连续 block 与空相邻段落。
8. 外侧停靠点按 Enter、输入、IME 单行/多行提交时，在块前/后原子插入同级段落；覆盖已有相邻段落、文档首尾、连续公式块、撤销/重做和失败原子性。不能预插或重复空段落。
9. Equation/Align 的段前段后间距由普通段落样式提供，分页器按相同的相邻段间距折叠规则合并，覆盖前后段落、连续公式块与换页。
10. 同步更新光标高度和结构导航权威文档，删除过期接口描述和重复高度逻辑；运行相关 Rust/native/Qt 测试和可用桌面验证，再由 subagent 独立审查最终 diff。

## 预计改动

预计涉及 `crates/caret-layout/src/line_metrics.rs`、`crates/caret-engine/src/layout.rs` 的数学块与 caret 投影、`crates/caret-engine/src/layout/math_navigation.rs` 的垂直候选、`crates/caret-app/src/document_view/`、Qt ViewModel/Canvas 及对应测试。`page_glyphs.rs` 只在审计确认它是度量策略权威时才纳入；避免在布局、快照、Qt 各层重复存储或推导高度。

## 验收标准

- [x] 相同正文基础样式、字号和行距下，数学普通字符与正文使用一致的默认行框/光标高度；允许 Math/Body 字体角色不同，且仅字符不同不造成无理由的高度跳变。
- [x] 上标、下标、分数、根式和 `\text` 超出正文基准行框时，Natural/Multiple/AtLeast 按内容需求扩展；Exactly 保持固定推进并按段落行距规则处理溢出。结构内部 caret 仍定位于正确脚本/分数槽位。
- [x] Equation 单格和 Align 多格使用 `caret-grid-layout` 的统一装配与逐行行框；相同内容的 Equation 单行和 Align 单行具有相同行高、基线和 caret 高度。
- [x] Exactly 固定行距按每一行固定推进，行内溢出裁切不会吞掉后续行；Natural/Multiple/AtLeast 按公式行内容扩展。
- [x] 同一基础高度与扩展规则覆盖行内公式、Equation、Align 单元格和空公式占位符；同类内容不因容器实现不同而重复计算高度。
- [x] caret 的水平位置、上下标所在视觉行、双向文本亲和性及上下导航候选不因高度统一而回归。
- [x] 不同数量的数学行遵循一致行框/光标度量；通过现有 Enter/Undo 路径或多行 fixture 验证行数变化。
- [x] Section、Equation、Align 均复用相同的块前/块后外侧停靠状态与键盘转换；块前、块内起/终点、块后均可到达，外侧状态支持 Enter、文本与 IME 创建同级段落。
- [x] Left/Right、Up/Down 从相邻段落首次到达公式时先停在块前/块后，再次同向进入；离开公式也先停外侧，反向重入和同向越过有效。公式停靠 caret 位于内容行首尾，Align 对齐首/末行；行内公式不误入垂直导航。
- [x] 外侧段落创建在块前/后同级执行，已有相邻段落不被改写；插入、输入法提交、光标状态和 undo/redo 一致，失败原子。
- [x] 公式块后的空段落段首 Backspace 删除该行并进入公式末端，undo/redo 恢复对应文档和编辑焦点；非空公式不被误删。
- [x] Equation/Align 前后段落间距复用段落间距折叠与分页规则；公式块换页时不携带前页的段间距。
- [x] Rust geometry、快照序列化、ViewModel/Canvas 投影的行为测试覆盖成功和无效/缺失 geometry。
- [ ] 可用目标桌面环境验证真实 caret 显示；未运行的平台验证如实记录。
- [x] 相关文档、验证记录和独立评审完成，重复高度策略及过期实现已清理。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-09 | Terra/medium 只读数据流审计 | 找出正文与各数学分支的行框/caret 来源、重复策略及风险 | 已完成；确认 Equation/Align 宿主直接用 MathLayout ascent + descent，MathTree/MathText caret 独立取结构盒/文本岛盒；指出结构导航文档与正文 caret 现状不一致，并区分 Align Tree 行、源码分隔符与 SoftBreak；未执行行为复现或测试 |
| 2026-11-09 | Terra/medium 共享网格架构审查 | 选定正文表格与数学宿主可共用的行列几何层 | 已完成；确认复用现有 `caret-grid-layout`，不新增空壳 crate；Equation 单格和 Align 多格共用表格装配，正文行框策略逐行输入，嵌套 matrix/cases 保留数学布局；指出 Exactly 要求逐行溢出裁切与行身份贯穿绘制/命中/caret |
| 2026-11-09 | Equation/Align 方向键边界与段落间距 | 四向边界导航并沿用正文块间距规则 | Left/Right 两种光标 affinity 均从公式两侧进入；Up/Down 从段中视觉边缘进入/退出；相邻行内公式不被 Up/Down 误入；定向测试通过。上下键状态路由、候选提交和块公式入口复用 Session/helper；MathBlock 分页输入继承默认段前/段后间距并与相邻段落折叠 |
| 2026-11-09 | `cargo test --workspace --locked` | Rust、native CTest、QML lint 与文档测试通过 | 通过；包括 Rust tests、native 10/10 CTest（含 QML shell load / format）及 doc-tests |
| 2026-11-09 | `cargo fmt --all --check`; `cargo clippy --locked --workspace --all-targets -- -D warnings`; `cargo format --check`; `cargo lint --check`; `git diff --check` | 格式、Rust/native lint 与 diff 检查通过 | 通过 |
| 2026-11-09 | 块外停靠与退格回归用例 | 行首/行尾停靠、连续/嵌套公式退格、stop 不误删、undo/redo | 通过；覆盖 Natural 多行 Align 首末行几何、连续公式和嵌套 Section；从公式右侧真实退出后 Backspace 保留邻接空段 |
| 2026-11-09 | Terra/medium 领域复审与 Sol/medium 最终评审 | 检查最终 diff 的导航状态、焦点恢复、布局几何、文档和分层 | 通过；修复意见均复审关闭；独立 reviewer 未另行运行测试 |
| — | 桌面可视验证 | 所有数学宿主光标高度稳定且位置正确 | 待维护者在目标桌面环境手动确认；本分支未运行桌面交互验证 |

## 风险与回退

统一基础高度可能导致复杂公式超出当前行距容纳范围。按 `page-layout.md` 中各行距类型处理：Natural/Multiple/AtLeast 按内容需求扩展；Exactly 保持固定推进并允许溢出裁切。不能通过偷偷更改 Exactly 语义来隐藏溢出。

## 决策与工作记录

- 2026-11-09：按用户确认创建独立任务，覆盖全部行内与块数学环境及 MathTree/MathText/源码光标状态。
- 2026-11-09：维护者实测补充数学块宿主高度随字符/公式内容变化，希望基础高度与正文一致，脚本和分数等按实际内容扩展。Terra 初步只读审计确认 Equation/Align 的 MathFragment 直接采用 MathLayout ascent + descent，MathTree 与 MathText caret 使用独立几何；因此把宿主行框和 caret 高度列为分别验收的关联问题。
- 2026-11-09：Terra 发现原结构导航文档对正文 caret 的实现描述不符合当前代码；按维护者要求，架构页现只保留目标行为契约，077 实施时仍需从代码与行为测试确认可复用的正文基准。
- 2026-11-09：维护者补充 Align Enter 可创建额外数学行但 Backspace 无法删除行边界。Terra 只读审计确认 Tree 行边界由 MathAlignRow 表达，空 cell 退格当前只移动到相邻 cell；源码行分隔符已有删除测试。为保持职责清晰，删除/合并契约单独建入 079，077 只覆盖现有 Enter/Undo 路径或 fixture 上的行数变化几何。
- 2026-11-09：维护者明确 Equation 是单行、Align 是多行；两者本质为表格式宿主，基础行高应与正文一致。排版复用现有 `caret-grid-layout`，Equation 为单格、Align 为多格；表格行列快捷键归 045，编辑态白色虚线辅助分割线归 046。旧 Equation SoftBreak 内容须保全，禁止静默丢弃。
- 2026-11-09：提取 Up/Down 共用状态路由后，分出“相邻任意公式”与“相邻块级公式”的入口模式；上下键仅走块级入口，避免从行内公式同一视觉行边缘意外进入 MathEdit。垂直边界派发错误继续向调用方返回，不吞掉后跨段移动。
- 2026-11-09：Left/Right 在同一正文偏移发生视觉亲和侧切换时也先检查相邻公式，避免首个按键只切换 affinity、没有进入公式；入口仍由 Session 检查行内与块级目标。
- 2026-11-09：维护者补充 Section 的块前/块后外侧 caret 停靠点可单独停留，便于在块前/后创建段落。077 将抽取为通用块边界状态和段落物化流程，Section、Equation、Align 共用，后续 block 接同一机制；数学块在布局和交互快照中提供边界几何适配。
- 2026-11-09：维护者明确居中公式块外停靠点应位于内容行首尾；公式块后的空段落段首 Backspace 删除空行并进入公式末端。补入共享导航、布局几何、删除事务及撤销/重做验收；覆盖连续公式链、嵌套 Section 和虚拟 stop 的退格仲裁。

## 完成摘要

仓库实现和自动验证已完成：数学宿主复用 `caret-grid-layout` 行框，Equation/Align 共享表格装配；外侧块停靠、逐行 Align 基线、段落物化和空行退格复用同一 Session 状态与历史事务。workspace 测试、Clippy、格式检查及 Terra/Sol 独立评审通过。任务状态为 `implemented`，仅保留目标桌面上的实际 caret 视觉确认；确认后可转为 `done`。
