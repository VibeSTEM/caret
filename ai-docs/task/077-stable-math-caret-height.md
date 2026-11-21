# 077 — 数学宿主行框与光标高度统一

- 状态：implemented
- 依赖：044、074
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-21

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
- Section、Equation、Align 使用统一块边界停靠逻辑：块前外侧 → 块内起点/终点 → 块后外侧四种状态均可到达；外侧状态不靠预插空段落表示。Left/Right 使用外侧停靠步进进入/离开公式；处于块外侧停靠时，Up/Down 按共享页面视觉行候选仲裁，不能复用水平键的同向重入。居中公式块外停靠 caret 位于所在页面内容行的行首/行尾，Align 垂直对齐首/末行。按 Enter、输入文字或提交 IME 才在块同级物化段落。前后已有段落不被改写，插入、焦点和撤销保持原子。
- Equation/Align 通过同一块边界实现接入；块类型只提供 ID、父级路径、页面边界几何和同级插入适配。Session 状态、键盘仲裁、快照光标、IME 预编辑/提交与撤销必须共享实现，后续 block 直接接入同一扩展点，不建立平行状态字段或重复 enter/exit 逻辑。
- 相邻正文行与块公式之间沿用段落的视觉边缘/宿主边界路由；块外侧停靠状态上的 Up/Down 使用共享页面视觉行候选，按候选几何进入最近的正文行或其他块，不复用 Left/Right 的公式重入。Equation 单行，Align 按可见行定位；相邻行内公式不能成为垂直目标。Left/Right 仍按块边界停靠规则进入/离开公式。
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
- [x] 视觉结构槽内 caret 与结构选区高度按结构深度每层减半（1/2、1/4……，最小 1pt），收进槽位布局盒并垂直居中：上标位于基线上方、下标位于下方，嵌套结构继续缩小；空槽使用槽位布局盒（含占位框）定位，不再伸出宿主行框。结构深度由公式排版随布局盒与文本岛请求提供，透明/格式化包装（Group、Row、Accent、Delimited、Color、Style）与行内格式化环境不增加深度；根层与块边界 caret 保持正文基础行高。
- [x] Equation 单格和 Align 多格使用 `caret-grid-layout` 的统一装配与逐行行框；相同内容的 Equation 单行和 Align 单行具有相同行高、基线和 caret 高度。
- [x] Exactly 固定行距按每一行固定推进，行内溢出裁切不会吞掉后续行；Natural/Multiple/AtLeast 按公式行内容扩展。
- [x] 同一基础高度与扩展规则覆盖行内公式、Equation、Align 单元格和空公式占位符；同类内容不因容器实现不同而重复计算高度。
- [x] caret 的水平位置、上下标所在视觉行、双向文本亲和性及上下导航候选不因高度统一而回归。
- [x] 不同数量的数学行遵循一致行框/光标度量；通过现有 Enter/Undo 路径或多行 fixture 验证行数变化。
- [x] Section、Equation、Align 均复用相同的块前/块后外侧停靠状态与键盘转换；块前、块内起/终点、块后均可到达，外侧状态支持 Enter、文本与 IME 创建同级段落。
- [x] Left/Right 从相邻段落到达公式时先停在块前/块后，再次同向进入；离开公式也先停外侧，反向重入和同向越过有效。Up/Down 从块外停靠按共享视觉行候选移动，不触发水平重入；Equation/Align 外侧停靠向相邻正文行移动，Align 内部仍按首/末行几何定位；行内公式不误入垂直导航。相邻正文行进入公式继续使用正文视觉边缘路由。
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
| 2026-11-21 | `cargo test --locked --offline -p caret-engine --lib`；`cargo test --locked --offline -p caret-math` | 结构深度减半、槽位盒居中/收拢、透明包装不缩小、空槽与根指数窄槽、深度下限、MathText、选区、Align 的行为与公式布局回归 | 通过：caret-engine 931 项、caret-math 182 项通过；新增 12 项行为测试全部通过 |
| 2026-11-21 | `cargo test --locked --offline -p caret-app -p caret-ffi`；`cargo clippy --locked --offline -p caret-engine --all-targets -- -D warnings`；`cargo fmt --all --check` | 应用/FFI 消费者、静态检查与格式 | 通过：caret-app 353 项、caret-ffi 18 项；Clippy 无告警；Rust 格式通过 |
| 2026-11-21 | `cargo test --locked --offline --workspace`（exit 0） | 全 workspace 回归与 native 聚合 | 通过：caret-engine 931 项、caret-app 353 项、caret-ffi 18 项、`caret-tests` native 聚合（内部 CTest 与 qmllint）通过，无失败 |
| 2026-11-21 | `cargo format --check`；`cargo lint --check` | 跨语言格式与 9 阶段静态检查 | 通过：format 通过；lint 9/9 通过（含 clang-tidy、include-cleaner、cppcheck） |
| 2026-11-21 | 第一版按 MATH 字号的等比缩放被维护者桌面截图验收否定 | 空分式分子槽：正文行高 16.25pt 的 caret 在 8.4pt 分子槽中向上伸出公式行框 4.26pt | 重做为结构深度减半并在槽位布局盒内居中；同一空分式分子槽的 caret 变为 8.13pt 且完全落在槽位/行框内 |
| 2026-11-21 | 领域评审（重做版，只读 diff 哈希 `aa46b899…`） | 行为、深度语义、测试、文档 | 发现 2 项 P2：① 仅按 `Operand` 路径计数会把 Group/Accent/Delimited/Color/Style 等透明包装当作结构层，`\hat{x}`、`\left(x^2\right)` 等根层光标被错误减半；② 验证命令不可复现、决策记录残留旧方案说明。修复：结构深度改由公式排版在布局盒与文本岛请求中提供（仅分式操作数、上下标、根指数等视觉结构槽增加），透明/格式化包装不计数；高度收进槽位盒并保留 1pt 下限；补充透明包装、根指数窄槽、深度下限与 MathText 深度用例；文档按最终方案重写。复审结论见后续行 |
| 2026-11-21 | 最终独立评审（只读完整 diff） | 077/037/证据收口整体 diff、行为、分层、测试、文档与 PR 就绪度 | P1 未发现；3 项 P2：077 决策记录残留旧方案、验证命令不可复现、037 的“三平台 CI 通过”需待 PR CI 覆盖新增用例。前两项已在本轮修正；037 状态待 PR CI 全绿后归档。另修正透明包装深度 P2（见领域评审行） |
| 2026-11-21 | 最终评审修复后的增量复审（只读） | 透明包装深度修复、槽位盒收拢、文档修正 | 通过：未发现新增 P1/P2；P2-A（上下限/空可选槽深度）确认修复，定向用例 PASS；提出 P3：077 验证行不应写不存在的 `\lim` 用例、`OverUnder`/`CancelTo` 未单独用例、空可选槽位置仍锚定 flow 原点（既有近似）、`Converter.slot_depth` 注释可补上下限。除空槽位置记为限制外均已修正 |
| 2026-11-21 | 最终评审增量复审发现的新增 P2：上下限/重音上下标/可选占位空槽未计结构深度（`\sum_{}^{}`、`\lim_{}`、`\overset/\cancelto`、空根指数仍按正文行高） | 这些槽位与已计深度结构同级，需由排版统一声明 | 修复：`big_operator` 上下限、`OverUnder` top/bottom、`CancelTo` target 使用槽位深度；空可选占位操作数（根指数、和/积/积分与上下限占位）按槽位深度+1 记录；零高度槽按深度缩放到 1pt 下限而不是退回正文行高。新增求和上下限与空上下限/空根指数行为测试（`OverUnder`/`CancelTo` 走同一 `with_slot_depth` 机制，静态复核，未单独用例）；caret-engine 931 项、caret-math 182 项通过 |
| — | 可用目标桌面环境验证真实 caret 显示 | 所有数学宿主光标高度稳定、结构槽内光标位于槽位内 | 重做后待维护者复验；本分支未运行桌面交互验证 |

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
- 2026-11-21：维护者验收发现结构槽内 caret 仍是正文基础行高：上标/下标/分式内部光标没有随槽位缩小。决策改为“槽位有效字号相对公式基础字号等比缩放并围绕槽位基线定位”，比例来自排版实际字号（MATH 脚本比例，STIX 两级约 70%/50%），不采用逐层减半的固定分数，避免 caret 明显短于同槽字形；字号未缩小的槽（显示公式分子分母）不改变正文 caret 高度。结构选区沿用同一槽位字号，避免高亮与 caret 高度不一致。根层 caret、块边界 caret 与命中容差仍使用正文基础行高。
- 2026-11-21：维护者用空分式截图指出槽内光标仍不合理并怀疑定位设计：显示公式空分子槽高 8.4pt，而字号比例方案给该槽 100% 正文行高光标（16.25pt），向上伸出公式行框 4.26pt。维护者建议按结构层级直接减半（上标上半部分、下标下半部分、嵌套 1/4）。决策改为“结构深度每层减半 + 垂直居中于该槽布局盒”：深度按路径中进入数学节点操作数的步数计算，格式化环境不计数；空槽用占位框所在的槽位布局盒，MathText 用文本行框；结构选区同规则。原字号比例方案与相应测试作废，评审在重做后的 diff 上重跑。
- 2026-11-21：领域评审后修订（重做版）：结构深度不再由 engine 按路径 `Operand` 计数，而由公式排版在转换时随 `MathNodeBox` 与文本岛 `MathTextRequest` 提供；只有分式操作数、上下标槽、根指数等视觉结构槽增加深度，Group、Row、Accent、Delimited、Color、Style 与行内格式化环境不计数。caret 高度收进槽位布局盒并保留 1pt 最小可见高度；未缩放几何 helper 注释明确只用于根层、块边界与命中容差；补充透明包装、根指数窄槽、深度下限与 MathText 结构深度用例。
- 2026-11-21：最终评审增量复审补齐结构槽清单：和/积/积分与 `\lim` 的上下限、`OverUnder` 的 top/bottom、`CancelTo` 目标同样使用结构深度；空可选占位槽（根指数、和/积/积分上下限）按槽位深度+1 记录，零高度空槽按深度缩放到 1pt 下限而非退回正文行高。补充求和上下限与空上下限/空根指数行为测试。

## 完成摘要

仓库实现和自动验证已完成：数学宿主复用 `caret-grid-layout` 行框，Equation/Align 共享表格装配；外侧块停靠、逐行 Align 基线、段落物化和空行退格复用同一 Session 状态与历史事务；视觉结构槽内的 caret 与结构选区按排版提供的结构深度逐层减半、收进槽位布局盒并居中，透明/格式化包装不缩小，空槽与文本岛同样适用。workspace 测试、native 聚合、格式、lint 与独立评审通过。任务状态为 `implemented`，仅保留目标桌面上的实际 caret 视觉确认；确认后可转为 `done`。
