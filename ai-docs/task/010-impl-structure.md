# 010 — M2：结构导航与结构命令

- 状态：done
- 依赖：009、020
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-30

## 目标与背景

让用户能通过结构命令和快捷键在文档块之间导航、创建和操作块环境，并能看见当前所在结构。Rust 文档树和编辑会话是领域行为的唯一来源；Qt/QML 负责按 Rust 提供的结构类别和布局范围绘制状态提示。

本任务使用[文档树与结构：页面层级与内容块形态](../architecture/editor/document-model/document-structure.md#页面层级与内容块形态)作为结构分类的唯一权威定义；[结构导航](../architecture/editor/structure-navigation.md)定义索引和几何投影。任务文档只记录实现范围与进度，不复制节点类别和语义。

当前文档模型中的 `Section` 与块级 `Custom` 是 `BlockEnvironmentKind` 的具体语义类型：二者共用可嵌套块容器；`Section` 额外承载独立标题和编号语义，`Custom` 是带可选实例显示名和主题定义引用的通用容器。Custom 的目标模型支持 Page、Block、Inline 三种可选 scope，嵌套顺序固定为 Page → Block → Inline；本任务当前仅实现 Block-scope 实例，其余 scope 和统一定义管理留待后续任务。两者归入通用 `MultiLine` 类别，但 Rust 保留具体领域 kind。新增环境应沿用可组合的容器与索引结构，不在 QML 为每个具体 kind 添加分支。`InlineMath` 是 Paragraph 内部的行内原子，不是独立块结构。

## 必读

- [结构导航](../architecture/editor/structure-navigation.md)、[文档模型](../architecture/editor/document-model.md)
- [快捷键](../architecture/interface/keyboard-shortcuts.md)、[键盘路由](../architecture/interface/keyboard-routing.md)
- [命令](../architecture/editor/commands.md)、[分层](../standards/layering.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

包括结构索引、段落/标题/块环境导航、块环境创建与嵌套、结构子树删除和显式提升、逐级扩大选区（覆盖段落和标题）、状态栏结构类别，以及分页布局范围映射。当前块环境背景高亮由 [031](031-environment-highlight.md) 定义；本任务只提供结构与同代布局几何。结构命令由 Rust Session 执行；快捷键和命令输入只传递稳定动作意图，不由 QML/C++ 修改文档树。列表项语义、编辑和导航由 050 定义并实现；010 不引入会与多段/嵌套列表模型冲突的临时结构。

普通标题可派生出大纲范围，但该范围不是可删除容器；删除标题只影响标题段落，不隐式删除其后续正文。本任务提供可复用的稳定结构 ID 与同源布局几何。当前环境高亮是 031 的视图叠加层，本任务不定义其颜色或行内环境高亮。

公式、表格、图形节点的模型与编辑分别由 011、012 及后续子任务接入；010 只定义通用索引/类别接口和当前已存在结构的行为，不提前伪造尚不存在的节点。公式内部移动、单元格内移动和新选区模型不在本任务范围。

## 前置条件与待决策

009 命令输入与 020 文档模型已完成。当前模型已有 Paragraph、Heading 样式、Section 与 Custom 块环境。当前仅 `\section` 是用户可执行的结构创建候选；空段落转换为 Section，非空段落后新建 Section。Custom 环境保留 Rust 领域 API，待 Customize 入口设计后再接入命令面板。更细标题命令由后续任务实现。块环境结构选区删除和 Mod+Backspace 保留内容解除已接入；环境边界导航、跨段选区、编辑行为和跨页几何均有 Rust 或 Qt 自动化行为测试。环境类型只通过 Rust 内置注册表扩展；界面以稳定语义 kind 本地化显示，不以标题文字或具体 QML 类型推断类别。

## 实施步骤

1. 用稳定节点 ID 为段落/标题和块环境构建结构索引；结构事务提交后从文档树完整重建索引，以统一逻辑重算标题派生范围。索引不写入文档、不进入布局缓存键；纯绘制或字体度量变化不重建索引。局部增量更新不属于当前实现，也没有基准证据要求引入。
2. 接入 Section/Custom 创建、嵌套、结构选区删除和显式提升；操作通过 Session 事务产生位置映射和可撤销历史。列表项结构由 050/051 设计与实现。
3. 将结构移动、跳出、逐级扩大选区和相关快捷键接入现有键盘路由；在命令输入、IME 和选区等上下文下不抢占其他按键行为。
4. 让布局快照对可导航块结构提供稳定 ID、页内范围和通用类别；跨页结构的范围由各页片段组成，不能用一个错误的单页矩形代替。
5. 画布通过 031 定义的视图状态消费当前块环境范围；滚动/缩放只改变投影，不触发文档编辑或结构重建。
6. 覆盖段落与块环境边界、嵌套环境操作、撤销、分页几何与键盘作用域；完成独立评审并处理发现。

## 文件结构设计

结构派生逻辑放在 `crates/caret-structure/`，该 crate 只依赖 `caret-doc` 及允许的基础类型，不依赖编辑引擎。保持“单一索引装配入口 + 按语义拆分的 atoms”：

```text
crates/caret-structure/src/
├── lib.rs                         # 公开稳定结构查询 API
├── index.rs                       # 从文档块树装配索引、查询结构栈和顺序
├── index/tests.rs                 # 索引的可观察结果测试
├── atoms.rs                       # 结构原子模块声明与内部导出
└── atoms/
    ├── paragraph.rs               # Paragraph 与标题语义
    ├── block_environment.rs        # 块环境容器语义
```

索引条目保留具体领域 kind、稳定 ID、父链和范围；`StructureClass` 是供状态栏/画布消费的通用类别投影。不要为每个具体 kind 建一套平行索引，也不要把正文复制到索引。

```text
crates/caret-engine/src/editing/session/
├── structure.rs                    # 结构命令分发、事务边界与导航
└── structure/
    ├── environment_mutations.rs    # 创建、嵌套、删除、提升
    ├── selection.rs                # 结构范围选择与位置映射
    └── tests.rs                    # 结构编辑行为测试

native/viewmodels/document/         # 仅传递 Rust 快照与结构命令
native/canvas/document_canvas/      # 绘制结构快照提供的范围，不持有结构真相
qml/Canvas/                         # 画布组合与主题颜色绑定
```

按仓库文件结构准则，先让 `structure.rs` 保持清楚；只有实现增长到多个独立职责时才新增子目录和文件，不预建空模块。新 Rust 源文件首行用中文功能注释。Qt/C++ 的按键映射和绘制职责分开，不把键盘输入、指针处理和结构布局塞回单一大文件。

## 验收标准

- [x] 段落、标题和块环境可按快捷键导航；快照提供稳定语义 kind，界面按当前 UI 语言显示本地化标签，Rust 保留具体结构语义。
- [x] 空段落执行 `\section` 时将该段转换为带独立标题的 Section；非空段落执行时保留原段，并在其后新建空 Section；结构变更有结构投影且可撤销/重做。Custom 容器创建保留为 Rust 领域 API，尚无用户命令入口。
- [x] Custom 保留可选实例显示名和主题模板引用；显示名须为非空、无控制字符且不超过 128 个 Unicode 字符，名称不需要注册表登记。
- [x] 块环境可嵌套、通过结构选区整体删除，或通过 Mod+Backspace 提升内容并解除外壳；失败命令不产生部分文档修改。
- [x] 删除普通标题只删除标题段落，不隐式删除由大纲层级派生的后续范围；逐级扩大选区可覆盖段落和标题。
- [x] 现有 Heading 大纲级别和环境标题进入索引后，派生范围与文档块顺序一致；由单一文档树构建逻辑计算。Heading 级别的交互编辑命令未纳入本任务，后续由 054 接入。
- [x] `at(pos)` 返回稳定 ID 构成的外到内结构栈；行内格式和 `InlineMath` 不产生块级停靠点。
- [x] 可导航结构在同一布局快照中有实际页内几何；跨页范围按页拆分，结构范围变化不会伪装成内容绘制变化。
- [x] 结构快照提供当前环境的稳定 ID 和同代页内几何，供 031 绘制当前环境高亮；滚动和缩放不改文档。Rust 布局测试覆盖多页片段，Qt Quick 场景图测试覆盖逐页显示与恢复完整范围。
- [x] 键盘路由不与命令输入、IME、选区和 UI 弹层冲突；Shift+方向键及鼠标拖选能选择块环境和连续多个结构，选中后 Backspace 删除完整子树；空 Section 后段首 Backspace 合并到标题末尾；测试覆盖撤销/重做恢复。
- [x] `caret-structure` 依赖方向符合分层规范；结构索引由单一 crate 持有，没有平行实现或新增无用依赖。
- [x] 结构索引不进入布局缓存键；结构事务及 Undo/Redo 文档恢复后从文档树重建，单纯绘制和分页几何变化不重建。
- [x] 按测试规范验证；fmt、lint、相关行为测试及独立评审通过。没有依赖未执行的人工验收作为完成证据。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-29 | `cargo test --workspace --locked --offline` | Rust 行为、CLI、QML/native 集成测试通过 | 既有验证通过；包含结构索引、段落命令、多行粘贴索引刷新、导航撤销边界、跨标题选区几何、Escape 与 IME 取消传播；native CTest 8/8。此记录早于本次环境高亮桥接改动。 |
| 2026-10-29 | 单页环境标题/正文布局与选区几何 | 标题和子块进入同一布局快照 | 自动测试通过；多页容器范围验证未执行 |
| 2026-10-29 | Section 标题/正文导航与首段 Backspace | story 跟随 Rust 段落 ID；首段 Backspace 合并到标题并可撤销 | caret-app 定向测试 2 项通过；caret-engine 119 项通过 |
| 2026-10-29 | `cargo check -p caret-app --locked --offline`、`cargo format --check`、`cargo lint --check` | 类型、格式与 lint 通过 | 通过；该次状态栏改动未运行行为测试、QM 加载或手工窗口验收 |
| 2026-10-30 | 结构索引更新策略核对 | 文档准确反映实现与复杂度决策 | 通过；结构事务完整重建索引，暂不引入未经性能基准支持的增量维护 |
| 2026-10-29 | `cargo test --locked --offline -p caret-tests --test native` | Rust/native 聚合测试通过 | 通过；CTest 8/8，含结构范围绘制、关闭清除和交互快照替换/清空测试 |
| 2026-10-30 | `cargo test --locked --offline -p caret-app block_environment_geometry_is_emitted_as_page_fragments` | 跨页 Section 几何按页面拆分且矩形有效 | 通过；环境跨越多个实际布局页，Rust 快照与内部布局范围页序一致 |
| 2026-10-30 | Qt Quick `ShellLoadTest::documentCanvasMapsPageCoordinatesAndHitsPages`（由 native CTest 聚合执行） | 多页高亮在单页可见范围内只绘制对应页，恢复范围后两个片段都可见 | 通过；场景图截帧逐页采样通过，CTest 8/8 |
| 2026-10-30 | GitHub Nightly run [40967290488](https://github.com/VibeSTEM/caret/actions/runs/40967290488) 根因复现与修复验证 | Miri 仅排除不支持系统字体/汇编的测试；Windows 持久化失败用例跨平台稳定 | 根因已修复；本地 Miri 与 macOS native 测试通过，Windows runner 需由本次 GitHub CI 实际确认 |
| 2026-10-29 | `cargo test -p caret-engine --locked --offline` | Section/Custom 创建、历史、索引和失败原子性通过 | Section 创建与撤销/重做、Custom 包裹与焦点索引、深层嵌套失败原子性通过；114 项通过（后续增加 APA 标题命令任务） |
| 2026-10-29 | `cargo test -p caret-doc --locked --offline` | 自定义环境校验与未知节点安全预览通过 | 29 项通过；覆盖 Unicode 显示名校验及未知块环境 kind 按通用规则进入只读安全预览并保留原始数据 |
| 2026-10-29 | `cargo verify-changed` | 本次工作树影响范围内的格式、Rust、QML/native 构建与行为验证通过 | 通过；workspace Rust 测试、QML lint、翻译 QM 加载及 native CTest 8/8 通过；命令候选快照断言随 `\\section` 可执行状态更新。首轮运行发现旧断言，修正后整体验证通过 |
| 2026-10-30 | `cargo test --workspace --locked` 与结构边界行为测试 | 环境边界输入、跨环境选区、空 Section 后退格合并及既有 workspace 行为通过 | 通过；workspace Rust、native/QML/翻译测试通过；定向行为覆盖边界快照、Shift+方向键、指针选区校验及方向焦点、Backspace Undo/Redo |
| 2026-10-30 | 跨页环境几何与当前环境高亮验收 | Rust 产出每页片段；Qt 场景图仅绘制可见页并正确清空/恢复 | Rust 多页快照测试和 Qt Quick 离屏场景图行为测试通过；未进行额外人工 GUI 走查 |

## 风险与回退

环境范围若有独立坐标来源会与分页/缩放错位；必须消费布局快照的结构范围。若几何或性能不满足要求，可暂时关闭视图高亮，但保留结构索引和命令行为。

## 决策与工作记录

- 2026-10-25：创建任务。
- 2026-10-29：确定通用展示类别为 Paragraph、SingleLine、MultiLine、Table、Figure；具体节点继续保留 Rust 领域语义。`Heading` 即使视觉折行仍是 SingleLine 结构；块级 Equation/Align 分别归 SingleLine/MultiLine；InlineMath 属于 Paragraph。
- 2026-10-29：校准任务文件边界为结构原子、索引装配、会话命令、Qt 桥接和 QML 展示；当前只有索引/查询、部分导航和状态栏已落地，不能将 `Section`/`Custom` 写成全部结构类型。
- 2026-10-29：独立评审覆盖状态栏结构投影和通用类别设计。发现的直接依赖问题改用 `caret_engine` 公开类型；文档对结构形态与公式类别的歧义已澄清。该轮未运行行为测试、QM 加载或手工窗口验收。
- 2026-10-29：将当前块环境范围从 Rust 布局快照传递到 Qt 画布；QSG 使用同代页内几何绘制高亮，离开环境或清空状态时移除旧范围。结构类别和页面语义收敛到 `document-model.md`，导航文档仅保留索引/交互规则。
- 2026-10-29：新增 `\\section` 创建 Section；Custom 容器保留 Rust 领域 API，等待 Customize 用户入口设计。结构环境的撤销记录仅保存原 Paragraph 一份，并在创建时缓存体积估算，避免历史变更期间重复遍历/序列化段落。APA 标题外观与 `\\subsection` 等标题层级命令另建后续任务，避免把样式逻辑塞进结构创建事务。
- 2026-10-29：模型审查确认 Section 标题命令必须与嵌套 Section 编号树一致；自定义定义归入文档内嵌主题快照，替换快照前验证所有被引用的 definitionId/scope，缺失引用时拒绝更新，不静默降级。`\chapter` 与 `\section` 的层级关系尚未定稿，暂不纳入 054。性能审查指出 JSON 打开、IME 预编辑和保存快照存在可测量的复制候选成本；未实测前不更换 JSON、引入 arena 或全树共享结构。
- 2026-10-29：移除未使用的旧块环境专属解析与能力声明；未知 kind 统一按通用安全预览规则处理，不保留任何旧格式专用分支。
- 2026-10-30：验收确认结构索引当前采用结构事务后的完整重建，导航文档不再把局部增量更新描述为现有实现；添加跨页 Section 几何与 Qt Quick 分页高亮自动化验收。Nightly 40967290488 的 Miri 过滤和 Windows 持久化失败测试修复已完成本地验证，Windows runner 结果留给本次 PR CI 确认。

## 完成摘要

完成。当前具备段落/标题/Section/Custom 结构索引、结构选区删除与扩大、Mod+Backspace 内容提升、命令创建、状态栏通用类别和环境范围几何桥接。环境首尾外侧停靠、跨段和多结构选区、删除/撤销、空 Section 后退格、跨页范围及 Qt 场景图分页绘制均有自动化行为覆盖。列表结构由 050/051 负责；Custom 的 Page/Inline scope、自定义入口和更细标题命令明确留待后续任务。
