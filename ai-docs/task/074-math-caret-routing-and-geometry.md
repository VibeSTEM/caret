# 074 — 数学光标候选与统一仲裁

- 状态：done
- 依赖：071、072
- 优先级：P1
- 创建 / 更新：2026-11-08 / 2026-11-09

## 目标与背景

所有方向键焦点与选区状态由 `caret-engine::Session` 权威管理，具体实体键经 `Session::run_editor_key` 和 `keyboards/` handler 仲裁。`caret-app` 从同代布局快照生成带 generation 的视觉候选。数学结构（尤其分数、上下标、根式和嵌套槽位）需要 MathLayout 的基线、缩放、节点盒及结构路径来计算 caret 候选；数学树不应另建一份光标状态。

MathFlow 内的普通 TextFlow 应复用正文文本的视觉移动、选区折叠、affinity 和 preferred-x 规则，只适配 `MathTextPosition` 与宿主路径。当前 Qt `math` 子命令 mode 解码已在 072 修复；用户反馈 `\frac` 上下方向键不切换，现有 Engine MathTree 垂直分支没有明确的结构候选提交，需要按统一键盘入口补齐。

MathTree 聚焦时不启用输入法；`\text{...}` MathText 文本岛复用正文输入法。OS 输入法提示、跨平台表现及数学结构中的 Unicode 标点/缺字 fallback 需要独立的平台与输入策略验收，已转入 [076](076-per-key-state-arbitration.md)，不作为本任务未完成项。

## 必读

- [验证与评审](../standards/validation-and-review.md)、[分层规范](../standards/layering.md)、[数学公式架构](../architecture/editor/math.md)、[键盘管理架构](../architecture/interface/keyboard-shortcut-architecture.md)、[键盘路由](../architecture/interface/keyboard-routing.md)、[071 宿主焦点协议](071-host-focus-and-key-routing.md)、[072 空公式宿主占位](072-empty-math-host-placeholder.md)

## 范围与非目标

- 保持单一光标/选区状态与按键仲裁：所有实体方向键最终进入对应 `EditorKey` handler；命令查询、IME 等更高优先级状态在底层几何边缘返回前获得处理。
- MathTree 结构导航使用 MathLayout 几何生成候选，正确跨分数分子/分母、上/下标、根式和 Align cell；不同脚本字号和基线决定 caret 几何高度。
- 普通正文文本沿 Parley 正文路径；MathFlow TextFlow 复用相同文本导航算法，只适配树位置与宿主身份，不复制状态转换规则。
- 数学结构的 MathTree/Source 分支保持 IME 禁用；`\text{...}` 是已实现的 MathText 文本岛，继续复用正文 IME。Qt 的 `ImhPreferLatin` 只是平台提示，不能保证锁定 OS 输入法；hint 的跨层校验及 MathText 命令行 enabled 状态一致性转入 076。
- 在 InlineMath 根流中用 `\text{...}` 输入正文时，必须复用根层 TextFlow→Paragraph 提升，不得把根 TextFlow 直接提交为 InlineMath。拆分后的正文使用正常 IME；删除中间文本后，若两侧公式相邻，应保留内容和元数据地合并为一个 InlineMath。
- 不让 Engine 复制页面像素模型或依赖 Qt/QML；不重构无证据表明重复的 MathTree 结构语义。

## 前置条件与待决策

- 依赖 071 的 `EditorKey` 单入口与 072 的可命中空宿主节点盒。
- 分子/分母与脚本之间的上下移动候选排序、边缘跨结构行为和 shift 扩选规则已由几何候选及逐键 handler 实现并覆盖测试。
- MathTree/Source 与 MathText 的输入法区别已明确；OS 输入法提示、中文标点/符号显式插入和缺字 fallback 转由 076 进行平台验收。

## 实施步骤

1. 绘制 command line、MathTree、MathText 与正文的方向键输入、候选、handler 和状态迁移图。
2. 添加分数上下导航、脚本高度/基线 caret、MathText mixed-bidi 边缘与命令查询活动时的复现测试。
3. 保留 072 已实现的活动 command line 优先提交实体方向 `EditorKey` 规则，补齐 MathText mixed-bidi/边缘回归；其余数学结构候选经同一 editor-key route 提交。
4. 抽取普通正文与 MathText 可共享的文本候选算法；MathTree 保留确实需要的结构几何适配，不做伪统一。
5. 注册并验证 Math 命令中的 `\text` 文本岛；确保 InlineMath 根流经安全提升与合并事务，更新架构文档并独立 review。平台输入法提示及 Unicode 标点/fallback 已转至 076。

## 预计改动

`crates/caret-app/src/document_view/navigation.rs`、`crates/caret-engine/src/editing/session/keyboards/{left_arrow,right_arrow,up_arrow,down_arrow,home,end}.rs`、`crates/caret-engine/src/layout.rs` 数学几何查询和对应 Rust/app/native 测试；如输入法策略确认，触及 `crates/caret-app/src/document_view/snapshot.rs`、`native/canvas/` 与 QML 输入法上下文属性。

## 验收标准

- [x] 命令查询 Math mode 的反斜杠输入、候选选择和方向键不会被底层 MathText/bidi 几何分支提前返回；状态更新完整且无卡键。
- [x] `\frac` 分子/分母间 Up/Down 可移动；分数、上/下标、根式及 Align cell 的结构导航和 caret 高度反映实际 MathLayout 几何。
- [x] 普通正文和 MathFlow TextFlow 使用相同文本移动/扩选/preferred-x 规则，并正确映射到各自主机位置。
- [x] 过期或非法候选拒绝后文档、焦点、选区和撤销历史不变；所有焦点状态仍由 Engine 权威持有。
- [x] MathTree/Source 禁用 IME，MathText `\text{...}` 使用正文输入法；OS 输入法提示与 Unicode 符号/标点及缺字 fallback 的平台验收转入 076。
- [x] 在 InlineMath 根流选择 `\text` 可安全拆分成公式与可输入正文的 TextFlow，不出现 `InlineMath root TextFlow must be promoted to Paragraph`；删除中间文本后只合并本次删除触及的相邻公式，并将焦点留在合并后的 MathTree 接缝，使方向键继续工作。选区替换、撤销/重做及空公式起始均保留正确位置。
- [x] 键位处理仍由 `keyboards/` 文件管理；相关 Rust/app/native 行为测试、格式/lint 和独立 final review 通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-08 | `cargo test --locked -p caret-engine` | 结构几何正确，过期候选原子拒绝 | 通过；662 tests passed。覆盖 MathTree 候选、上下标/空槽退格与焦点恢复 |
| 2026-11-08 | `cargo test --workspace --locked` | app/native 输入与方向键行为无回归 | 通过；Rust workspace tests passed，native C++/QML integration 10/10 passed。随后新增的上下标端到端用例由下方定向命令单独验证 |
| 2026-11-08 | `cargo test --locked -p caret-app subsup_pointer_focus_routes_backspace_and_mod_backspace_by_operand_path -- --nocapture` | 命中图1的上标字符时保持上标 path；普通 Backspace 和 Mod+Backspace 只按当前 operand/path 转换焦点 | 通过；布局光标位置经 `hit_test_math_structure` 回到 SubSup operand 2/offset 1，普通 Backspace 删除上标字符后在空槽退回根 offset 3，Mod+Backspace 降级后光标仍在 `^{a}` 内；Undo 恢复上标 path |
| 2026-11-08 | `cargo test --locked -p caret-app inserting_before_superscript_base_then_mod_backspace_succeeds -- --nocapture` | 在 `x^a` 上标基底起点输入 `v` 后，插入提升到整个脚标前；Mod+Backspace 可安全降级，空基底输入仍保留原行为 | 通过；真实 `place_caret_at_page_json` 命中基底 operand 后输入 `v`，焦点迁移到根 offset 1，源码为 `vx^{a}`，Mod+Backspace 无错误 |
| 2026-11-08 | `cargo test --locked -p caret-app typing_at_empty_superscript_base_fills_its_placeholder -- --nocapture` | 空上标基底的输入替换 Placeholder，不提升到脚标外 | 通过；命中空基底后输入 `v`，光标仍处于 base operand，槽位变为符号 `v` |
| 2026-11-08 | `cargo test --locked -p caret-engine typing_a_batch_at_script_base_start_keeps_new_script_outside -- --nocapture` | 批量输入 `v^b` 与逐键输入保持相同脚标附着范围 | 通过；先将 `v` 提升到外层，再创建其上标，得到 `v^{b}x^{a}`；Mod+Backspace 成功 |
| 2026-11-08 | `cargo test --locked -p caret-engine script_base_start_insertion_lifts_for_script_kinds_and_grouped_base -- --nocapture` | 上标、下标、双脚标及一层/多层显式 Group 的基底起点采用同一提升规则 | 通过；插入后焦点落在脚标节点外，源码降级与 Mod+Backspace 均通过 |
| 2026-11-09 | `cargo test -p caret-engine math_text_command_splits_inline_math_and_accepts_ime_input --locked` | 从 Math 命令面板选择 `\text` 后提升为正文 TextFlow，可输入中文；删除中间文本合并公式并将焦点留在接缝，左右键仍可导航 | 通过；验证 `xy` 拆分、中文输入、删除后根 offset 1、左右键往返及 Undo 恢复拆分结构 |
| 2026-11-09 | `cargo test -p caret-engine math_text_command_replaces_root_selection_and_undo_restores_its_tree_focus --locked`、`math_text_command_splits_an_empty_inline_math_into_ime_text --locked`、`math_text_command_enters_ime_text_in_equation_and_align_blocks --locked` | 覆盖正反树选区、空 InlineMath、Equation/Align 的 MathText 输入法分支和 Undo 焦点 | 通过；实际命令面板接受 `\text` 后进入对应正文/MathText 输入分支 |
| 2026-11-09 | `cargo test -p caret-engine deleting_unrelated_paragraph_text_does_not_merge_an_untouched_math_seam --locked` | 删除公式之外的段落文本不合并无关空 TextFlow 与公式 | 通过；仅被本次删除清空且可安全合并的文本岛会触发公式合并 |
| 2026-11-09 | `cargo test -p caret-app inserting_before_superscript_base_then_mod_backspace_succeeds --locked` | 鼠标真实命中上标基底起点后输入并执行 Mod+Backspace | 通过；命中后的完整 path/offset 断言和光标提升回归通过 |
| 2026-11-09 | `cargo test --workspace --locked` | 验证 MathTree 焦点、`\text` 输入岛、IME、撤销/重做、Qt/QML 集成及仓库其他行为无回归 | 通过；Rust workspace 与 native/QML integration 10/10 均通过 |
| 2026-11-09 | `cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all` | 最新完整工作区静态检查和格式化 | clippy、fmt 均通过 |
| 2026-11-09 | `cargo format --check`、`cargo lint --check`、`git diff --check` | 跨语言格式、静态分析及补丁空白检查 | 全部通过；CMake、QML、C++ 和翻译检查无错误 |
| 2026-11-08 | `cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all`、`git diff --check` | 最新 workspace lint 与格式检查通过 | clippy、fmt 与 diff 检查均通过 |

## 风险与回退

数学结构没有与普通段落相同的视觉行格；强行复用段落布局会丢失脚本缩放、基线和操作数路径。共享范围限定在 TextFlow 文本候选语义与状态提交，MathTree 继续采用结构布局候选。平台输入法提示不保证系统切换；若 LatinOnly 破坏必要符号输入，则回退为 PreferLatin 并保留应用内显式 Unicode 输入。

## 决策与工作记录

- 2026-11-08：072 的 PR #116 已合并到 main；从同步后的 main 创建 `yuki/task-074-math-caret-routing-and-geometry` 开始实施。

- 2026-11-08：用户确认数学结构需保留上下标相关 caret 几何，普通文本复用标准导航，并补充 `\frac` 上下方向键未切换、数学模式 IME 与中文标点/缺字体验问题。MathTree 纵向候选由当前布局提供，输入法提示不能替代结构化 composition。
- 2026-11-08：按用户要求，Up/Down 的上下文优先级留在各自文件；完全相同的布局候选校验/提交移到 `keyboards/helpers/`。其他按键统一迁移另列 076。结构路径筛选覆盖嵌套分数和 Align 空结构槽；必需 Placeholder 始终只生成 offset 0，TextFlow→MathTree 首次跳转保留源 caret x。
- 2026-11-08：用户复现必填空槽 Placeholder 后可落光标并在其后插入文本。修正在通用 MathTree 位置处理：鼠标/旧命中仅将紧邻 Placeholder 后的边界归一到其前；左右键跨空操作数时不产生占位符后的边界；输入在旧非法位置会替换占位符。占位符后的其他既有内容仍保留路径和编辑顺序，TextFlow 纵向目标也按树结构路径筛除相邻公式的文本岛。
- 2026-11-08：补充验证双脚标（SubSup）及根式/分数空槽。删除脚标前文会移动整个脚标原子；删除基底后保留脚标结构及空基底槽。修复 Tree 原生 `Mod+Backspace` 将空 MathFlow 降级为 `{}` 源码时的语义比较与光标映射，光标落在花括号内部，Undo/Redo 恢复树焦点；真实 opaque 元数据仍按原规则拒绝。
- 2026-11-08：只读评审还发现输入法字段跨层校验、MathText 数学命令行状态一致性和全键盘旁路问题；这些不属于本次 Up/Down 直接修复，已补入 076。
- 2026-11-08：用户确认上下标内外的光标环境不同。对照其 MathTree JSON，根 `d` 与 `SubSup` 是相邻兄弟；脚标基底/下标/上标分别处在同一 `SubSup` 的三个 operand。Terra 只读排查确认先前 engine 回归手动设置 path，未覆盖布局命中与 app handler；新增 DocumentView 命中几何、`focus_math_structure_hit` 和 `handle_editor_key_json` 的端到端用例。测试 fixture 按真实 JSON 保留 `atomClass: ord`，验证图1上标 offset 1 与图2根序列 offset 5 的路径不同；在真实树元数据下 Mod+Backspace 光标映射通过。未知/不一致元数据仍由可文本化检查拒绝。
- 2026-11-08：用户进一步复现 `x^a` 内部基底起点输入 `v` 后 Mod+Backspace 报 `edit.opaque_content_boundary`。真实鼠标路径复现发现树变为 `Superscript(base=[v,x])`，这会改变脚标附着范围。普通文本在非空脚标基底起点输入时现在提升到脚标节点前，得到 `vx^{a}`；显式复合基底仍保留其 Group 语义。空基底及其 Placeholder 仍由输入替换填充；批量输入按首个非空字符选择提升规则，确保后续 `^`/`_` 作用于新插入的外层字符。增加真实鼠标→输入→Mod+Backspace、空槽和批量输入回归。
- 2026-11-08：用户确认输入法范围：数学结构通过英文键盘进入后保持结构输入不启用 IME；`\text{...}` 已实现为普通 MathText 文本岛，仍允许输入法组字。保留这两类状态的区别，不把 Qt 的拉丁提示描述成系统级锁定。
- 2026-11-08：用户复现 `\text` 命令提交时报 `InlineMath root TextFlow must be promoted to Paragraph`。目标是沿现有根层提升路径拆成可用 IME 的正文文本；删除中间文本后相邻公式应无损合并。此项并入 074 验收。
- 2026-11-09：注册 `\text` 公式命令并补齐本地化标签。InlineMath 根路径走源映射事务和既有提升器，保留正反选区与撤销前 MathTree 焦点；Equation/Align 及嵌套操作数直接进入 MathText 分支。仅当本次删除完整清空未带样式/未知元数据的文本岛且两侧公式元数据安全时才合并，焦点恢复到合并公式接缝，方向键继续由 MathTree handler 管理；其他无关正文删除不会触发合并。
- 2026-11-09：收尾时将 OS 输入法提示、中文标点/符号显式插入和字体缺字 fallback 的平台验收边界正式移交 076；074 的其余验收与跨层验证均有回归或检查记录。

## 完成摘要

074 已完成并通过 PR [#117](https://github.com/VibeSTEM/caret/pull/117) 合并至 `main`。根层 `\text` 拆分、正文 IME 输入、被删除文本岛触发的相邻公式安全合并、合并接缝焦点及左右方向键导航均有回归覆盖；平台输入法提示和 Unicode 缺字体验由 076 接续验收。
