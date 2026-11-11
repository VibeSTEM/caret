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
| 2026-11-11 | Shift 四向键跨 Paragraph/Section/Equation/Align/InlineMath 测试（`range_extension/tests.rs`、`selection/tests.rs`、`math_edit`/`navigation` 测试） | anchor/focus 与结构范围一致，内部位置可映射 | 通过：覆盖横向/纵向、反向选区、BlockEdge 端点、相邻公式链、Align cell 逐步、纯文本步骤不安装结构选区、鼠标跨界晋级 |
| 2026-11-11 | 删除/替换/复制/Undo/Redo 跨宿主测试（`structure/tests.rs`） | 同一范围，无数据丢失 | 通过：Section/Equation/Align 组合、相邻公式链、空文档补位、对象复制 U+FFFC、失败/替换回滚、Undo/Redo 精确恢复 |
| 2026-11-11 | 同代 Environment/selection 几何测试（`layout/tests.rs`、app `document_view` 测试） | 完整/增量投影一致，旧 generation 不覆盖 | 通过：文本+环境+公式对象矩形、BlockEdge 起止、full vs delta 相等、旧 generation/缺失公式几何整体拒绝 |
| 2026-11-11 | `cargo test --workspace --locked` | 全 workspace 通过 | 通过（Engine 797、App 296、workspace 1719；其余 crate 与 native/QML 聚合通过，exit 0） |
| 2026-11-11 | `cargo clippy --workspace --all-targets -- -D warnings` | 无警告 | 通过 |
| 2026-11-11 | `cargo fmt --all -- --check`、`git diff HEAD --check` | 无格式/空白错误 | 通过 |
| 2026-11-11 | Terra 分域（行为/架构）与 Sol final review | 无阻断发现 | 通过；发现项与修复见“独立评审” |
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

## 独立评审

- 行为/状态域（Terra/medium，只读）：发现 1 个 Critical（结构选区 Delete 被 `delete_empty_paragraph_after_math` 抢先，删错空段落并残留悬空选区）、1 个 Warning（app 仍用 `structure_selection()` 判断，BlockEdge 范围误画外层 Section 高亮）、1 个 Suggestion（空文本替换仍提交删除计划）。全部修复并加回归测试；同时修复纯文本步骤误装结构选区、纵向无候选伪造字素范围、鼠标晋级端点未规范、公式文本岛过早转对象范围、替换回滚未覆盖 history/generation/pending style。
- 架构/分层域（Terra/medium，只读）：无 Critical；6 条 Warning（app 守卫、环境覆盖双规则、覆盖派生重复、安装写回三份、块 kind 硬编码、块树 helper 归属）与 7 条 Suggestion。已修复单一安装写路径、端点规范化、共享停车/亲和性助手、`is_range_block`/`block_range_unit` 集中分派、app 复用 `CurrentEnvironmentView`、几何单入口与私有可见性、`is_current_for` 复用、文档矛盾；保留项（按 Block 变体的固有 adapter 清单、端点扫描性能、`current_inline_environment` 测试兼容投影）已记录。
- brooks-review（PR 门禁，只读）：终审前复评 Health Score **97/100**，无 Critical/Warning；上一轮 3 条 Warning（鼠标/键盘晋级规则、落点/亲和性重复、安装写路径）已收敛，剩余 3 条 Suggestion（删除计划函数偏长、覆盖/端点二次扫描、`Direction` 兼任块外侧边）记录为后续重构项，其中扫描复杂度建议纳入 041 基准。终审随后发现空 BlockEdge 范围 Critical（见下），修复后全量复验通过。
- brooks-test（测试质量，只读）：Health Score 81/100；3 条 Warning（相邻数学宿主删除缺回归、旧快照只测私有守卫、Align 遍历测试空转）全部补齐真实行为用例；Suggestion 中的失败错误种类、Escape 公共路径、陈旧几何断言、替换回滚均修复；测试夹具重复保留为测试维护性建议。
- Sol final review（未参与实现的只读 subagent，最终树）：结论 needs fixes。发现 1 个 Critical：公式内 Shift+Left 转整公式范围后按 Shift+Right 会安装 `[After, After]` 空 BlockEdge 范围，导致选区不可见、Backspace/Delete 永久 no-op，并掩盖空段落退格路径；修复为“覆盖为空时不安装结构范围，清空并回落普通选区/停靠”，删除计划为空时回落普通删除，块边界输入前清理结构态，并加精确手势回归测试。另发现 1 个 Warning：IME 提交不消费结构范围，修复为 `CommitComposition` 走同一替换事务并加测试。第 2 个 Warning（范围内中间段落只截断不合并）确认为预期语义并写入 block-interaction.md。3 条 Suggestion（冗余派生谓词、BlockEdge 注释、`math_block_geometry` 找到标记顺序）中注释与几何顺序已修，冗余谓词保留为防御性检查。修复后 `cargo test --workspace --locked` = 1719 passed / 0 failed。另一份并行只读终审在修复前结论为 mergeable，未覆盖该空范围手势；以发现并修复 Critical 的终审为准。
- 本环境没有 gpt-6.1-terra / gpt-6.1-sol，使用 deepseek-v4.1-flash 的独立只读 subagent 承担相同分域与最终评审角色。
- 保留的后续项：`plan_structure_range_deletion` 仍为 140 行编排（阶段 helper 已分离）；结构范围覆盖/文本端点派生仍按块/段落线性扫描，复杂度未做基准，建议纳入 041；`BlockEdge.side` 仍复用 `Direction`，可后续引入 `BlockSide`。这些不改变当前行为，不作为本任务的阻断项。

## 完成摘要

083 完成。结构选区现在以统一的文档范围端点（文本位置或块外侧边界）表达，Paragraph、Section/Custom、Equation/Align 与 InlineMath 共用同一 anchor/focus 顺序、覆盖派生、删除/替换/复制与历史事务；块公式作为原子对象进入范围，公式内部仍保留 MathTree/MathText 本地扩选。新增单一安装/清除入口和单一写路径，`selected_block_environment` 从多选列表派生，Math projection identity 比较收敛。结构范围删除/替换使用一个原子事务并在失败时完整回滚（含 history state/epoch/generation/pending style），相邻 Equation/Align 链、Section 邻接 Backspace、Escape 停靠与对象复制均有行为测试。完整快照、增量刷新与交互投影使用同代布局；选中公式对象的整栏矩形进入选区层，缺失公式几何或旧 generation 结果整体拒绝而不是静默清空。架构文档同步了端点、环境提升、纵向扩选、Escape 优先级、几何层与实现文件边界。未执行项：nightly 覆盖率门槛与 Qt 桌面手工/平台验收；Source 分支无本地选区、Table/Figure 仍需按 adapter 清单接入、端点扫描未做性能基准，均已在任务卡与文档中记录。
