# 083 — 跨宿主选区、编辑事务与几何投影

- 状态：done
- 依赖：071、074、077、081、082
- 优先级：P1
- 创建 / 更新：2026-11-09 / 2026-11-11

## 目标与背景

数学块与 InlineMath 目前不能完整进入文档级选区；Shift+方向键部分路径无操作，跨块范围、删除恢复、复制和高亮几何缺少单一协议。数学块删除可能跳过相邻块，Section 前 Backspace 可能产生 `edit.invalid_position`。083 使用 081 的三态焦点和 082 的文档位置/顺序，统一选区、编辑事务及同代布局投影。

## 必读

- [块宿主交互协议](../architecture/editor/document-model/block-interaction.md)
- [文档位置与限制](../architecture/editor/document-model/document-position.md)
- [结构导航](../architecture/editor/structure-navigation.md)
- [行内与块级环境视图](../architecture/editor/document-model/environment-views.md)
- [画布与交互背景](../architecture/interface/canvas-highlighting.md)
- [验证与评审标准](../standards/validation-and-review.md)

## 范围与非目标

- 让 Paragraph、Section/Custom、Equation/Align、InlineMath、InlineEnvironment 与数学文本流共用 anchor/focus 范围、方向、替换/删除目标和位置映射；局部坐标由 adapter 映射。
- Shift+四方向键构造同一文档顺序下的选区；数学树/源码/文本岛内部按本地语义移动，跨宿主边界转换为可操作的结构范围。
- 选区删除、替换、复制、Undo/Redo 与失败原子性消费相同范围，不泄漏 U+FFFC、不静默丢失节点或恢复过期视觉选择。
- 修复连续 Equation/Align 删除落点、Section 标题/首段邻接 Backspace 失效及块/Inline 选区无法表达等 080 原始问题。
- 完整快照、交互刷新与 Qt 几何组合复用共享 Environment/selection projection；允许 Rust/Qt API 经评估调整，不预设具体 rect 数组结构。
- 审计 `current_inline_environment()` / `InlineEnvironmentRange` 与 `CurrentEnvironmentView` 的职责重叠、公共消费者和兼容边界；先统一语义投影，再按调用方证据决定合并、弃用或保留适配 API。
- 审计 `selected_block_environment` 与 `selected_block_environments` 的写入路径、外部消费者和空块外删除特例；将单值从多选权威状态派生，或保留有明确语义的独立状态，并覆盖失败回滚。
- 审计数学 projection identity 在历史合并与重绑中的比较；只合并重复比较，不改变 Undo/Redo 归组或目标宿主恢复。
- Table/Figure 仅遵循共享协议扩展点；不实现单元格网格选择或图形编辑。保留 079 已定义的 Align 行首退格规则。

## 前置条件与待决策

- Task 081 的三态焦点仲裁和 Task 082 的宿主全序/定位已完成。
- 复制/替换数学对象的公开命令语义按现有产品契约核对；不得因 UI 暂无命令而将选中状态与可编辑范围拆成两套权威。

## 实施步骤

1. 将 Shift+Left/Right/Up/Down、鼠标选择及选区折叠映射到 082 的共享文档位置；统一普通、MathText、MathTree/Source 与结构范围的 anchor/focus。
2. 让结构删除/替换、复制和历史恢复消费同一范围；保持单个公式对象删除与结构选区删除各自的业务目标，但共享位置映射与事务底座。
3. 修复块数学连续删除落点和 Section 邻接 Backspace；验证结构状态从真实宿主路径解析，不使用隐藏 Paragraph 锚点。Escape 离开块级公式时落到当前块外后停靠，重复 Escape 保持该停靠；对象删除只移除目标对象，不复用 Escape 的边界转换。
4. 让活动 Inline/Block 环境、高亮、命中与选区矩形由同一语义投影生成，并由完整快照、增量刷新和 Qt 使用同代结果。
5. 覆盖跨 Paragraph、Block、InlineMath、嵌套 Section、反向选择、空槽位、连续公式链 Escape 与单对象删除、失败/Undo/Redo 的黑盒和领域用例。
6. 运行影响范围内验证，委派 Terra/medium 分域评审；修复后由未参与实现的 Sol/medium 对最终 diff 独立只读评审。

## 预计改动

预计涉及 `caret-engine` Session selection/transaction/history、`caret-structure` 范围消费、`caret-app` 快照与 hit/navigation 候选、`caret-layout` 几何映射及 Qt selection/highlight 投影和跨层行为测试。不要把导航仲裁复制到 Qt/QML，不为每个结构 kind 新建平行选区实现。

## 验收标准

- [x] Shift 四向键可在 Paragraph、复杂 Block 和 InlineMath 两侧扩选，anchor/focus 方向和文档范围一致；内部 MathTree/MathText 位置可正确映射。
- [x] 显示选中的范围可被删除/替换/复制，U+FFFC 和节点数据不丢失；成功、失败、Undo、Redo 后 caret、selection、历史状态有效。
- [x] 连续 Equation/Align 删除只移除目标宿主并落到最近有效边界；Section 邻接 Backspace 不产生 InvalidPosition，拒绝时无副作用。
- [x] Escape 离开块级公式时进入当前宿主外后停靠；连续 Escape 不跳过相邻 Block，方向键才继续按文档顺序移动。删除单个公式仅删除目标且采用独立的最近有效位置恢复。
- [x] 完整快照、增量刷新与 Qt 绘制复用同一 Environment/selection 语义投影和 document generation；Inline/Block 几何来源由 adapter 保留。
- [x] 公开环境查询 API 的调用方已盘点；兼容性与迁移路径有依据，不保留重复权威语义，也不因内部整理破坏外部消费者。
- [x] Block 环境单值/多值选择状态及 Math projection identity 的消费者和历史语义已审计；无丢失空块删除、失败回滚或 Undo/Redo 行为。
- [x] 不存在可见选择但空文档范围、隐藏 selection、相邻数学块被跳过或旧 generation 高亮覆盖新状态。
- [x] 081/082 的定位和三态协议无重复权威；新 Block 通过 adapter 接入而不复制键盘/选区仲裁。
- [x] 相关行为测试、格式/lint 和独立评审通过；覆盖率门槛由 nightly CI 承担、本地未执行，Qt/平台手工验收与覆盖率均已在验证表明确记录为未验证项。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-11 | Shift 四向键跨 Paragraph/Section/Equation/Align/InlineMath 测试（`range_extension/tests.rs`、`selection/tests.rs`、`math_edit`/`navigation` 测试） | anchor/focus 与结构范围一致，内部位置可映射 | 通过：覆盖横向/纵向、反向选区、BlockEdge 端点、相邻公式链、Align cell 边界整块转换、纯文本步骤不安装结构选区、鼠标跨界晋级 |
| 2026-11-11 | 删除/替换/复制/Undo/Redo 跨宿主测试（`structure/tests.rs`） | 同一范围，无数据丢失 | 通过：Section/Equation/Align 组合、相邻公式链、空文档补位、对象复制 U+FFFC、失败/替换回滚、Undo/Redo 精确恢复 |
| 2026-11-11 | 同代 Environment/selection 几何测试（`layout/tests.rs`、app `document_view` 测试） | 完整/增量投影一致，旧 generation 不覆盖 | 通过：文本+环境+公式对象矩形、BlockEdge 起止、full vs delta 相等、旧 generation/缺失公式几何整体拒绝 |
| 2026-11-11 | `cargo test --workspace --locked` | 全 workspace 通过 | 通过（Engine 797、App 296、workspace 1719；其余 crate 与 native/QML 聚合通过，exit 0） |
| 2026-11-11 | `cargo clippy --workspace --all-targets -- -D warnings` | 无警告 | 通过 |
| 2026-11-11 | `cargo fmt --all -- --check`、`git diff HEAD --check` | 无格式/空白错误 | 通过 |
| 2026-11-11 | PR #136 评审修复回归（`cargo test --locked -p caret-engine`、`-p caret-app`） | 公式扩选/删除合并/InlineMath/折叠行为通过 | 通过：Engine 819 passed、App 298 passed，0 failed（含新增回归） |
| 2026-11-11 | `cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check` | 无警告/格式错误 | 通过 |
| 2026-11-11 | `cargo test --workspace --locked` | 全 workspace 通过 | 通过（exit 0，含 native/QML/CTest 聚合；Engine 823、App 298、native/QML 聚合均通过） |
| 2026-11-11 | 分域只读评审（行为/状态/测试、架构/分层/文档） | 无阻断发现 | 通过；建议项与修复见“独立评审” |
| 2026-11-12 | 桌面测试补漏：`cargo test --locked -p caret-engine`、`-p caret-app` | 公式内 Shift 高亮与 InlineMath 上下键回归 | 通过：Engine 837 passed、App 308 passed，0 failed；`cargo test --workspace --locked` exit 0；clippy/fmt 通过 |
| 2026-11-12 | 复查反馈修复：`cargo test --locked -p caret-engine`、`-p caret-app` | 文本岛边缘 Shift 转整公式、Align 首/末 cell 与多文本岛回归 | 通过：Engine 846 passed、App 314 passed，0 failed；`cargo test --workspace --locked` exit 0；clippy/fmt 通过 |
| — | nightly `cargo coverage` 85% 门槛、Qt 桌面手工/平台验收 | 覆盖率与真实绘制/输入 | 未执行：本地未运行 nightly 覆盖率与 Qt 手工交互；记录为未验证项 |

## 风险与回退

文档位置和布局快照代际不一致会使范围落错对象；结构删除的焦点恢复还可能跨越相邻块。每次修改前校验 generation、宿主身份和可排序范围；映射失败则原子拒绝并保留文档与有效状态。

## 决策与工作记录

- 2026-11-09：从 080 拆分 Issue #126 的选区、删除、环境投影问题及原始三类用户缺陷；依赖共享三态和全序定位，不做逐类型补丁。
- 2026-11-11：确定结构选区端点载体 `DocumentRangeEndpoint::{Text, BlockEdge}`。文本位置是常规端点；范围边界落在复杂 Block 对象两侧时保留 Block 外侧边界（稳定 ID + 侧向），只在 caret 落点、命中与几何投影时规范到相邻文本位置。文档契约同步为“保留端点、投影时归一”，不再承诺安装时改写为段落位置。
- 2026-11-11：`selected_block_environment` 改为从 `selected_block_environments` 派生；新增 `selected_math_blocks`。结构选区、两个列表和端点只通过单一安装入口写入，清除只通过单一清除入口；历史回滚是唯一快照式写入例外。
- 2026-11-11：块环境视为范围原子：扩选从环境内部越到外前/外后时把另一端扩展到环境相对边界；鼠标/键盘晋级共用同一判定，环境提升同时规范端点为整棵环境。纯文本步骤保持普通 `Selection`，不安装结构选区；纵向扩选要求段落边缘，不用字素步长伪造视觉行。
- 2026-11-11：公式内部扩选先走本地 MathTree/MathText 语义；文本岛只在线性序列首/末岛转换，操作数内部岛保持本地导航（根级 InlineMath 文本岛按文档契约不存在，代码保留防御分支）。Source 分支没有本地选区，Shift+方向键保留非扩选光标移动，记录为已知限制。
- 2026-11-11：结构范围删除/替换使用单一删除计划与一个 `DocumentPatch::Compound`：节点删除 patch 按同级下标降序，段落 `before` 取自节点移除后的文档、`after` 取自最终文档；替换复用既有插入入口，失败时恢复文档、索引、缓存、选区、亲和性、math_edit、结构状态、history state id / merge epoch / generation / pending style。
- 2026-11-11：剪贴板仍为纯文本，块级 Equation/Align 与 InlineMath 一致输出 U+FFFC；Undo/Redo 使用文档 patch 精确保留 MathTree，不恢复结构高亮。
- 2026-11-11：公开查询 API 审计：`current_environment_view` 是生产权威（app 高亮、格式化、Mod+Backspace）；`current_inline_environment` 无生产消费者，改为 view 投影并保留给既有测试（记录为测试兼容投影）；`structure_selection_geometry` 降为私有，`structure_selection_rectangles` 是唯一几何入口；`structure_selection_range` 有 app 测试消费者而保留 public；`structure_selection`/`structure_selection_anchor` 仅保留给全文本投影，生产代码改用 `has_structure_selection` 与 `structure_selection_text_bounds`。
- 2026-11-11：Math projection identity 只合并重复比较（共享 `projection_identity`/`projection_identity_matches` 助手），不改变 Undo/Redo 归组、投影重绑或目标宿主恢复。
- 2026-11-11：Table/Figure 扩展：`caret-structure` 提供 `is_range_block`/`is_math_block`，Session 用 `block_range_unit` 统一单元端点。剩余按 `Block` 变体的固有分派（公式邻接、删除 patch 身份、环境内容判定、覆盖递归、Align cell 判定等）列在未来 adapter 接入清单；现阶段不引入 trait adapter，也不宣称 Table/Figure 已接入。
- 2026-11-11：性能未做基准：端点夹取与覆盖派生在每次显示/扩选时扫描段落/块，复杂度随文档增长；记录为待验证项，未宣称性能提升。
- 2026-11-11：环境没有可用的 gpt-6.1-terra/sol；分域与 final review 使用 deepseek-v4.1-flash subagent，保持只读、未参与实现的独立角色，并按同一检查项输出报告。
- 2026-11-11：PR #136 评审拍板改变结构范围删除语义：跨 Block 结构删除与纯文本跨段删除统一为“两端文本段落合并、中间普通段落移除”，不再保留“中间段落只截断不合并”的旧语义。实现改为在节点移除后的文档上调用 `paragraph_ops::merge_paragraph_range` 合并两端段落，被删除段落全部进入 `Paragraphs` 补丁 `before` 快照（`after` 只含幸存段落），历史仍是单个 `DocumentPatch::Compound`；两端不都是文本（含 BlockEdge）时保持对象删除语义但把中间普通段落改为整段移除。同步更新 `block-interaction.md` 删除契约与回归测试（含“abc|Eq|mid|xyz → 单段 ayz”、多中间段、段落样式不兼容失败原子性、Undo/Redo）。旧评审中“中间段落只截断”的结论由本条取代。
- 2026-11-11：PR #136 高危问题修复：公式内 Shift 扩选进入嵌套结构后，Tree 分支焦点路径偏离 anchor，后续扩选返回 `InvalidPosition` 并卡死。改为扩选单层移动（嵌套 Math/Environment 作为原子步进），焦点离开 anchor 所在序列或到达 root/Align cell 边界时转换为整公式宿主结构范围，保持 focus 与 anchor 同路径、可被历史恢复；非扩选导航仍可下钻。补前进/反向/嵌套锚点/非扩选下钻回归测试。
- 2026-11-11：PR #136 中危问题修复：InlineMath 越界曾安装空覆盖结构选区（`math=[]`），产生隐藏范围并吞键。`install_structure_selection` 成为空覆盖唯一守卫（写状态前拒绝）；InlineMath 越界改写普通段落文本 Selection 覆盖 U+FFFC，下一次方向键按普通文本扩选；Align cell 边界扩选统一转换为整块 Align 结构范围，不再逐 cell 游走。同步 navigation/range_extension 旧断言。
- 2026-11-11：PR #136 低危项处理：结构选区下普通 Left/Right 改为折叠到文档顺序起点/终点并消费按键（新增 `collapse_structure_selection`；BlockEdge 端点复用块外停靠；app `move_visual_caret_json` 在候选计算前下发无候选按键并补 app 行为测试），不再从焦点端多移动一个视觉步长；清理 `selected_block_environment`/`selected_math_blocks` 已被 `has_structure_selection` 包含的冗余判断与 `delete_selected_block_environment` 单层包装；实证空 Equation/Align 会排出正高度 `math_fragment`、零高度退化在当前布局契约下不可达，保留防御分支并补空公式几何回归测试。
- 2026-11-11：结构范围对象删除语义的中间段判定改为几何“完全位于范围内”：BlockEdge 端点投影到的相邻段落只是切口载体，不能被当成端点段落；[Eq1,p,Eq2] 双块外侧边界删除时 p 整段移除，BlockEdge 起点后、文本焦点前的段落整段移除。补三条 BlockEdge 中间段回归测试。
- 2026-11-11：PR #136 修复的分域只读评审（独立 deepseek-v4.1-flash subagent，行为/状态/测试与架构/分层/文档两组）结论均通过、无阻断。按建议收敛实现与文档：结构范围合并可行性只由 `paragraph_ops::merge_paragraph_range` 判定，Session 仅解析端点位置并移除无效文档克隆；`convert_math_host_to_structure_range` 更名为 `convert_math_host_to_document_selection` 并注明 Block/InlineMath 两种结果；`park_at_range_endpoint` 移入 `document_position.rs` 作为共享端点落点；`block-interaction.md`、`structure-navigation.md` 补齐不可合并回落、MathText 局部扩选与 Align cell 边界转换、普通 Left/Right 折叠规则；加强扩选锚点固定/范围非空的断言与空覆盖守卫的既有结构选区前置断言，并补环境内段落合并、ExplicitBreak 不可合并回落、IME 提交替换结构范围、Align 反向多 cell 边界测试。
- 2026-11-11：保留项与未验证风险：结构删除新增“几何完全包含”判定按候选段落做端点比较、`remove_range_paragraphs` 做定位排序，属删除路径非逐字符热路径，未做基准（与既有覆盖派生同阶，纳入 041 基准范围考虑）；`selected_block_environment()` 清理后已无生产消费者，保留为单值派生/测试查询（同 `current_inline_environment` 先例）；无段落文档在 `Session::new` 的文档校验阶段即被拒绝，普通左右键折叠无需额外降级分支；MathTree→TextFlow 迁移时 Tree 锚点被替换、已累积 Tree 选区静默丢弃属既有行为，非本次引入，记录为后续单独评估项。
- 2026-11-11：brooks-review 门禁发现的两条 Warning 已处理：结构选区块外侧边界锚点在 Home/End 与纵向候选扩展时按 `text_endpoint_clamped` 侧向投影为文本锚点（新增 `structure_selection_text_anchor`，横向/纵向/Home/End 共用，删除两处重复投影回退），补 Shift+Down/Home/End 覆盖公式对象回归与块外前边界的普通 Left 折叠测试；MathText 文本岛转换规则按实现语义收敛文档——同侧再无其它文本岛即转换为整公式对象（即使之后仍有符号/环境等非文本内容），补 `x\text{ab}y` 正向转换测试与 `\text{ab}` 块公式用例；InlineMath 根文本岛在文档校验阶段即被拒绝（必须提升为段落文本），不作为有效转换场景。
- 2026-11-12：复查反馈的公式内 Shift 文本岛边缘问题修复：MathText 分支在文本岛内没有同方向后续时，原先退化为不扩选的 `MoveCaret` 并丢失已累积锚点；现与 Tree 分支统一——离开当前层即调用 `convert_math_host_to_document_selection` 转为整个公式宿主选区（Align 中间 cell、操作数内部文本岛、多个 `\text{}` 中间岛均如此）。删除不再使用的 `math_text_at_host_edge`/`text_island_is_host_edge`，更新两条旧的“保持本地导航”断言，并补 Align 首 cell 文本岛边缘 Shift+Right 转为整块 Align 的回归。复查同时确认设计取舍：公式内扩选按当前层逐项，离开该层直接选中整个公式，不在中间结构层级停靠；本 PR 的 086/087 追加实现为桌面测试反馈后的同分支补丁，任务卡与提交均独立标注。

- 2026-11-12：桌面测试小瑕疵补漏（083 范围）：①`\math` 内 Shift+Right 到右边界转换后光标落在对象左缘（段尾场景）：`inline_math_slot_bounds` 前后亲和性写反，改为 before=(start,Downstream)/after=(end,Upstream)，与 `exit_math_edit_for_navigation`/停靠路径统一；补 app 段首/段中/段尾与 Shift/非 Shift 一致性回归。②左右键无法进入连续 Equation/Align、Shift 一次选中整条公式链：水平候选跨多个纯公式块时改为逐个文档单元（非扩展先停靠再进块，扩展走 `extend_structure_selection`），补 engine/app 回归。③集成期回归：32490721 的非严格“中间段完全包含”判定会把与块外侧边界重合的零宽空段落当作范围内段落一并删除；改为“至少一个严格位于范围内的段落”才算中间段，补 7 条回归（含真实 Shift 手势与 Undo/Redo）。
- 2026-11-12：本轮另有 2 项非 083 范围的小瑕疵按用户要求在同分支修复并独立任务记录：InlineEnvironment 普通 Backspace 清理见 [089](089-inline-environment-backspace-cleanup.md)；跨公式块/块环境拖选实时预览见 [090](090-drag-selection-live-preview.md)。PR 范围扩大的原因与 086/087 相同（桌面测试后用户要求同分支继续），任务卡与提交信息逐项标注。
## 独立评审

- 行为/状态域（Terra/medium，只读）：发现 1 个 Critical（结构选区 Delete 被 `delete_empty_paragraph_after_math` 抢先，删错空段落并残留悬空选区）、1 个 Warning（app 仍用 `structure_selection()` 判断，BlockEdge 范围误画外层 Section 高亮）、1 个 Suggestion（空文本替换仍提交删除计划）。全部修复并加回归测试；同时修复纯文本步骤误装结构选区、纵向无候选伪造字素范围、鼠标晋级端点未规范、公式文本岛过早转对象范围、替换回滚未覆盖 history/generation/pending style。
- 架构/分层域（Terra/medium，只读）：无 Critical；6 条 Warning（app 守卫、环境覆盖双规则、覆盖派生重复、安装写回三份、块 kind 硬编码、块树 helper 归属）与 7 条 Suggestion。已修复单一安装写路径、端点规范化、共享停车/亲和性助手、`is_range_block`/`block_range_unit` 集中分派、app 复用 `CurrentEnvironmentView`、几何单入口与私有可见性、`is_current_for` 复用、文档矛盾；保留项（按 Block 变体的固有 adapter 清单、端点扫描性能、`current_inline_environment` 测试兼容投影）已记录。
- brooks-review（PR 门禁，只读）：终审前复评 Health Score **97/100**，无 Critical/Warning；上一轮 3 条 Warning（鼠标/键盘晋级规则、落点/亲和性重复、安装写路径）已收敛，剩余 3 条 Suggestion（删除计划函数偏长、覆盖/端点二次扫描、`Direction` 兼任块外侧边）记录为后续重构项，其中扫描复杂度建议纳入 041 基准。终审随后发现空 BlockEdge 范围 Critical（见下），修复后全量复验通过。
- brooks-test（测试质量，只读）：Health Score 81/100；3 条 Warning（相邻数学宿主删除缺回归、旧快照只测私有守卫、Align 遍历测试空转）全部补齐真实行为用例；Suggestion 中的失败错误种类、Escape 公共路径、陈旧几何断言、替换回滚均修复；测试夹具重复保留为测试维护性建议。
- Sol final review（未参与实现的只读 subagent，最终树）：结论 needs fixes。发现 1 个 Critical：公式内 Shift+Left 转整公式范围后按 Shift+Right 会安装 `[After, After]` 空 BlockEdge 范围，导致选区不可见、Backspace/Delete 永久 no-op，并掩盖空段落退格路径；修复为“覆盖为空时不安装结构范围，清空并回落普通选区/停靠”，删除计划为空时回落普通删除，块边界输入前清理结构态，并加精确手势回归测试。另发现 1 个 Warning：IME 提交不消费结构范围，修复为 `CommitComposition` 走同一替换事务并加测试。第 2 个 Warning（范围内中间段落只截断不合并）确认为预期语义并写入 block-interaction.md。3 条 Suggestion（冗余派生谓词、BlockEdge 注释、`math_block_geometry` 找到标记顺序）中注释与几何顺序已修，冗余谓词保留为防御性检查。修复后 `cargo test --workspace --locked` = 1719 passed / 0 failed。另一份并行只读终审在修复前结论为 mergeable，未覆盖该空范围手势；以发现并修复 Critical 的终审为准。
- PR #136 修复轮分域只读评审（deepseek-v4.1-flash 独立只读 subagent，行为/状态/测试与架构/分层/文档两组）：均通过、无阻断；建议项已收敛（合并前置条件单一权威、共享文本锚点、注释/架构文档与实现对齐、加强断言并补环境内合并/ExplicitBreak 回落/IME 替换/Align 反向多 cell 测试），详见决策与工作记录。
- PR #136 修复轮 brooks-review（PR 门禁，只读，balanced）：Health Score **83/100**（完整 PR diff 抽样：未提交修复全部生产代码 + 基线高风险模块）；2 条 Warning、2 条 Suggestion。Warning 1「块外侧边界锚点的结构选区在 Shift+Home/End/Down 带候选时丢失覆盖对象」已修复：新增 `structure_selection_text_anchor` 按边界侧向投影文本锚点，横向/纵向/Home/End 候选共用，补垂直与 Home/End 覆盖回归；Warning 2「MathText 文本岛边界转换文档与实现不一致」按实现语义收敛文档（同侧无其它文本岛即转整公式对象）并补正向转换测试；Warning 3「正向转换/纵向扩展缺行为测试」已补 4 组测试；Suggestion R4（锚点解析重复）随共享 helper 消除。R1（删除计划函数偏长、补位段落逻辑重复）保留为后续重构项。
- 2026-11-12：桌面测试补漏（083）：公式内部 Shift+方向键扩选没有蓝色选区高亮。新增 engine `DocumentLayoutSnapshot::math_tree_selection_geometry`（同 host/path、逐被选 item 取 child 盒、caret 行带、Align `clip_range_for_row`、嵌套结构原子；空范围空矩形、跨 path/越界 None）与 `Session::math_tree_selection_context`（暴露 anchor/focus，revision 过期 None）；app snapshot 新增 `math_tree_selection_json` 替换 full/interaction 两处空数组并按页 clamp。补 Equation/InlineMath/Align/嵌套环境与 app full-vs-interaction 几何回归。
- 2026-11-12：桌面测试补漏（083）：InlineMath 内 Up/Down（含 Shift）在无内部垂直候选时静默 no-op。engine `exit_vertical_math_block` 允许 Inline 目标复用 `exit_math_edit_for_navigation`（落 U+FFFC 前/后侧），Block 非扩展保持、Block Shift 保持 no-op、pending source draft 仍 `InvalidMath` 且状态不变；app `move_vertical_caret_json` 记录 Inline 目标并在本次按键退出后同按键续派发普通文档纵向导航（保留 preferred x，无递归回路）。补 engine 8 + app 8 行为测试并更新旧 no-op 断言；同步 structure-navigation.md/math.md。
- 2026-11-12：桌面测试补漏的增量独立只读评审结论 mergeable；按建议补 `canvas-highlighting.md` 的公式树内部选区矩形权威说明（同代布局、逐 item 盒 × caret 行带、Align 行裁切、空范围不发布）。S2 中 app 层 InlineMath Source 分支入口在公共命令下不可构造（空 InlineMath 默认进入 Tree 分支），由 engine Source 退出测试 + app 通用 Inline 分支覆盖，记录为测试入口限制；Text 文本岛 Shift 垂直扩展与 clamp 分支仍属推理覆盖，留待 069 平台验收补充。S3（几何 None 导致整份快照发布失败）在不变量下不可达，按既有文本岛投影同一约定处理，不改降级策略。
- 2026-11-12：桌面测试其余两项分流，不并入 #136：行内对象边界选区/高亮覆盖相邻 InlineMath 的根因在 Parley 0.12 inline box advance 与 caret-layout 对象插入点，main 已存在，由 [086](086-inline-box-selection-geometry.md) 跟踪；空 InlineMath 隐藏宿主盒宽进入正文 advance、聚焦高亮按 U+FFFC 跨度而非插入槽，由 [087](087-empty-inline-math-advance-and-highlight.md) 跟踪（072 契约补充）。两项均需独立修复与桌面验收，不属于跨宿主选区/事务范围。
- 本环境没有 gpt-6.1-terra / gpt-6.1-sol，使用 deepseek-v4.1-flash 的独立只读 subagent 承担相同分域与最终评审角色。
- 保留的后续项：`plan_structure_range_deletion` 仍为 140 行编排（阶段 helper 已分离）；结构范围覆盖/文本端点派生仍按块/段落线性扫描，复杂度未做基准，建议纳入 041；`BlockEdge.side` 仍复用 `Direction`，可后续引入 `BlockSide`。这些不改变当前行为，不作为本任务的阻断项。

## 完成摘要

083 完成。结构选区现在以统一的文档范围端点（文本位置或块外侧边界）表达，Paragraph、Section/Custom、Equation/Align 与 InlineMath 共用同一 anchor/focus 顺序、覆盖派生、删除/替换/复制与历史事务；块公式作为原子对象进入范围，公式内部仍保留 MathTree/MathText 本地扩选。新增单一安装/清除入口和单一写路径，`selected_block_environment` 从多选列表派生，Math projection identity 比较收敛。结构范围删除/替换使用一个原子事务并在失败时完整回滚（含 history state/epoch/generation/pending style），相邻 Equation/Align 链、Section 邻接 Backspace、Escape 停靠与对象复制均有行为测试。完整快照、增量刷新与交互投影使用同代布局；选中公式对象的整栏矩形进入选区层，缺失公式几何或旧 generation 结果整体拒绝而不是静默清空。架构文档同步了端点、环境提升、纵向扩选、Escape 优先级、几何层与实现文件边界。未执行项：nightly 覆盖率门槛与 Qt 桌面手工/平台验收；Source 分支无本地选区、Table/Figure 仍需按 adapter 清单接入、端点扫描未做性能基准，均已在任务卡与文档中记录。
