# 043 — M3：公式文档模型与编辑

- 状态：in-progress
- 依赖：010、042
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-04

## 目标与边界

建立 Caret 自有的可编辑数学树，并实现公式创建、内部编辑、位置和键盘行为。数学文档模型、行内/块环境、宿主适配与持久化设计以[数学公式架构](../architecture/editor/math.md)和[文档模型](../architecture/editor/document-model.md)为权威；[011](011-impl-math.md) 管理总体验收。解析器和排版后端不得成为 `.cdoc` 的第二份状态。

043 覆盖 `caret-doc`、`caret-math`、`caret-engine` 的模型、TeX 适配和编辑事务。页面绘制、布局、命中及光标几何归 [044](044-math-layout-and-rendering.md)；表格单元格模型/编辑与画布集成归 [045](045-table-model-and-editing.md)/[046](046-table-layout-and-canvas.md)。首版不支持自动编号、`\label`、`\ref` 或 IME 数学源码输入；字体命令资源与 fallback 归 [065](065-math-text-font-environments.md)。

## 完成状态

- [x] 以 Caret 节点表示 InlineMath、Equation、Align 与 MathFlow；schema v3 和所需能力声明已接入。`.cdoc` 只写结构树、行列和格式，不写 TeX/sourceRange/sourceMap；旧双表示 schema 与已退役字段在存储边界拒绝，不迁移。
- [x] 公式 TextFlow 复用正文 Run、Run 属性和字素边界；单 TextFlow 输入/删除、Bold/Italic、Run underline、InlineMath 根层拆分及 `Mod+Backspace` 降级走宿主事务。engine 的 Align 导航与元数据匹配消费 `caret-math` 同次解析提供的范围。
- [x] MathEdit 投影绑定 session 与 document generation；历史快照共享 `Arc<MathSourceMap>`，恢复校验目标 ID、公式种类、光标和路径，失败回滚文档补丁。历史与普通重入只在原树副本上刷新可验证的瞬时范围，不替换持久化结构；源码分支允许规范序列化一致的写法差异，但锚点位置、TextFlow 映射或数学节点范围不一致时拒绝恢复。文本岛插入的空数学锚点立即进入树并有 Session undo/redo 用例。
- [x] InlineMath 根层 TextFlow 提升为 ParagraphContent，保留顺序、空文本、Run 属性及单层 InlineEnvironment 身份；固定 MathNode 操作数槽位不提升。
- [x] 从持久化 DTO 删除冗余的 `MathTree.source` 与 `Align.source`；公式源码仅由 MathEdit 投影持有，历史重开时从树生成规范源码。
- [ ] 将 serde 跳过的节点、环境和 TextFlow 源码范围字段迁出 DTO，让 `MathSourceMap` sidecar 成为唯一映射权威。当前 `.cdoc` 不持久化这些字段，但编辑器仍在 DTO 上桥接它们。
- [ ] 补齐源码草稿状态迁移：`MathSourceDraft` 已独立保存未完成源码与有界局部撤销历史；解析不完整或候选树元数据无法无损映射时，已提交树、焦点和文档 generation 保持原样，输入作为交互状态返回。局部撤销/重做优先于文档历史；草稿回到基线后才可进入文档历史。保存和异步打开拒绝丢弃未提交草稿，dirty 状态包含草稿；Escape 显式取消并恢复基线光标。关闭确认区分不可保存的草稿并本地化。草稿 pending 会隐藏 AutoSave/recovery deadline 并更新 timer generation，底层排期与在途快照保留；草稿结束后按当前状态重新暴露，主文档回到 clean 时取消未触发 AutoSave，recovery 仍可记录已回到保存点的修订。失焦、过期 revision 生命周期及真实 App 关闭验证仍未完成。
- [ ] 完成跨相邻 TextFlow 的选区/组合字素、数学文本 IME 与 Font 环境树编辑。当前数学 TextFlow 编辑已能按映射对树做局部事务，普通输入不必每次全量重解析；但它仍依赖源码区间映射、源码编码和 DTO 上的临时范围桥接，需与 `MathSourceMap` sidecar 迁移协同收敛到共享 TextFlow 树编辑语义。桌面 IME 还需 044 提供数学光标和预编辑几何，不能只解除 MathEdit 输入法开关。
- [ ] 保证 TeX 可无损表达且未超过源码预算的 MathTree 都能建立规范编辑投影；结构归一化须能处理相邻 TextFlow 合并并保留可映射身份，对不支持 Font 语义或超过源码预算的树明确拒绝且不改动文档/焦点。

## 验收

- [x] 三种公式载体使用稳定 ID、未知字段和版本化 Caret schema；paragraph/math content-flow 能力校验完整。
- [x] 数学节点的编码 schema 只含结构化树、行列和格式，不含 TeX/sourceRange/sourceMap；旧双表示和退役字段在入口拒绝。
- [ ] InlineMath、Equation、Align 的完整 `.cdoc` 保存/重开与 ID/未知字段往返；目前公式文档仍会进入只读预览，待保存能力链路启用后验收。
- [x] 编号环境、自动编号、`\label`、`\ref` 由解析器拒绝且不改动文档。
- [x] 普通 TextFlow 编辑、数学单 TextFlow 输入/删除/Bold/Italic/underline、InlineMath 根层 Math/Text 焦点拆分/降级、Align 范围与 undo/redo 有命令级行为用例；失败不提交部分树、位置或历史状态。
- [x] 清理持久化公式源码副本；MathEdit 历史投影以目标身份和历史源码恢复，规范源码不依赖 DTO 字段恢复。
- [ ] 清理 DTO 临时范围桥接，并让映射仅由与目标树状态绑定的 MathEdit sidecar 持有。
- [ ] 完成并测试源码草稿的失焦、关闭事件与过期 revision 状态迁移；AutoSave/recovery deadline 的隐藏/重现、旧 timer generation 作废、失败写入不重试、clean-but-pending recovery 保留已由 Rust 生命周期测试覆盖。Qt success/error interaction 与 edit response 均刷新状态和 timer；真实公式草稿取消后的 timer 写入仍待 Qt 黑盒验证。
- [ ] 完成跨 TextFlow 选区/IME、Font 数学环境及 044 依赖后的公式命令调用链。
- [x] Rust/native C++ coverage gate：全局函数与行覆盖率均超过 85% 门槛；实测结果见下方验证记录。
- [ ] 按验证规范完成最终全分支审查及尚未覆盖的黑盒检查。性能结论需另有可复现基线；当前不宣称性能提升。

## 当前验证与评审

| 日期 | 范围与检查 | 结果及限制 |
| --- | --- | --- |
| 2026-11-03 | `cargo test --locked -p caret-doc -p caret-math -p caret-engine --no-fail-fast -- --quiet` | caret-doc 58、caret-math 51、caret-engine 383 项通过；覆盖树形/范围错配拒绝、空数学锚点结构与 Session undo/redo、根层焦点拆分、失败原子性及元数据保留。 |
| 2026-11-03 | 三 crate Clippy、`cargo fmt --all -- --check`、`git diff HEAD --check` | 全部通过。 |
| 2026-11-03 | Terra/medium MathEdit 历史、源码映射评审；Sol/medium 完整 diff final review | 修复历史重投影覆盖持久树、未匹配元数据丢失、空数学锚点被 TextFlow 聚合吞掉及范围错配接受。历史 redo 后进入 TextFlow 编辑、插入/删除和 undo 由 Session 用例覆盖；非连续 sourceMap 的跨锚点选区目前仅有 helper 级拒绝测试。最终复核未发现其他 actionable finding。 |
| 2026-11-04 | `cargo test --locked -p caret-math --no-fail-fast`、`cargo test --locked -p caret-engine --no-fail-fast`、`cargo test --locked -p caret-app --no-fail-fast`；三 crate Clippy、`cargo format --check`、`git diff --check` | caret-math 52、caret-engine 390、caret-app 198 项通过。新增草稿保留/撤销/重做、未闭合定界符修复、自动配对大括号历史、元数据拒绝原子性、dirty/保存失败、异步打开保护及草稿期间设置命令拒绝。Terra/medium 草稿状态与模块边界 review 找到的问题已修复，包括未闭合定界符不可编辑、错误返回却改状态、草稿越界使用旧节点范围、显示公式基线范围错配及交互 dirty 未刷新。逐键快照已改为共享源码缓冲并省去撤销历史副本，但没有性能基线，耗时影响仍待验证。 |
| 2026-11-04 | `cargo test --locked -p caret-tests --test native -- --test-threads=1`、`cargo format --check` | native 聚合构建通过，9/9 CTest 项通过。ShellLoad 的默认保存路径测试使用可写临时 Documents 根目录，验证适配器创建目录与重名选择；URL 转换另由独立用例覆盖，不依赖宿主 Documents 权限。Sol/medium 最终完整 diff review 与 native 覆盖率 gate 尚未执行。 |
| 2026-11-04 | Terra/medium 草稿跨层与源码映射架构 review；设置命令回归测试；native 路径测试重构 | 修复草稿期间页面几何/文档字体设置会推进 generation、使草稿后续无法撤销或继续输入的问题：这些命令现在原子拒绝，修复草稿后仍可提交。路径适配器测试改用临时 Documents 根目录，验证适配器自身创建目录、重名选择和空目录拒绝；本地路径转换拆为独立用例，不再因 Documents 不可写而跳过。架构 review 指出源码范围 sidecar 仍未与树编辑事务、Align 单元格坐标和规范化树形绑定；未接入的范围树原型已移除，保留共享结构路径类型，sidecar 迁移仍是未完成项。 |
| 2026-11-04 | 修复 [CI run 41759737585](https://github.com/VibeSTEM/caret/actions/runs/41759737585)：`cargo lint (clang-tidy, includes, cppcheck, ubuntu)`；MathSourceMap sidecar 与 TextFlow 路径接线 | `shell_document_path_test.cpp` 缺少直接 `QIODevice` include，已补齐；`cargo lint includes --changed --check` 与 native CTest 9/9 通过。数学源码边界查找改用解析投影的结构 sidecar，Align 单元格坐标使用完整公式范围并保留语义范围边界夹取；普通树与 Align 映射在编辑事务中重建，失败不提交候选状态。InlineMath 根分裂产生的焦点片段会与新树绑定；零宽扩展锚点保留原顺序，Run 边界验证采用二分查找。历史恢复仍未对重建树显式核验 sidecar；绑定接口依赖事务调用方先完成结构/语义校验，路径校验、编辑和格式化仍通过 DTO 瞬时范围桥接，迁移未完成。 |
| 2026-11-04 | `cargo fmt --all -- --check`、`cargo test --locked -p caret-math -p caret-engine --no-fail-fast -- --quiet`、caret-math/engine Clippy、`cargo lint includes --changed --check`、`cargo test --locked -p caret-tests --test native -- --test-threads=1`；Sol/medium final review | caret-math 58、caret-engine 390 项通过；格式、Clippy、include lint 通过；native 9/9 CTest 通过。普通 `parse`/逐键 `classify_input` 路径跳过 sidecar 捕获和序列化签名，sidecar 只在 `parse_projection` 构造；完整性能基线仍缺，不能据此宣称端到端性能提升。Sol/medium 未发现 P1/P2；历史恢复 sidecar 校验、DTO 范围桥接和性能基线仍未完成。 |
| 2026-11-04 | 数学历史恢复 sidecar 范围核对；`cargo test --locked -p caret-math projection::source_map::tests`、`cargo test --locked -p caret-engine --no-fail-fast -- --quiet` | caret-math 新增 2 项映射等价边界用例；caret-engine 390 项通过。历史恢复现在同时比较重建 MathTree/Align sidecar 与历史 sidecar 的结构路径和源码位置；相邻线性文本片段先规范合并比较，空数学节点允许零宽光标与其包围定界符表示同一中心位置，错位锚点仍拒绝。此改动缩小了历史 sidecar 的未验证面，但范围仍暂存于 DTO，完整迁出未完成。 |
| 2026-11-04 | 数学编辑期间 IME 门控：Rust 快照 → Qt `ImEnabled` → Canvas 输入事件；`cargo test --locked -p caret-app --no-fail-fast -- --quiet`、native 聚合测试 | caret-app 199 项通过；native 9/9 CTest 通过。当前所有 MathEdit 分支（含数学命令查询）暂时禁用 IME：Qt 取消已有预编辑并通知平台更新 `ImEnabled`，忽略迟到事件；Rust 边界拒绝非空迟到输入且保留文档、generation、MathEdit 与命令行；离开公式后恢复启用。完整快照必须带 `inputMethodContext`，两条快照路径都要求布尔 `enabled`；缺字段或错类型整笔拒绝。失败原子测试覆盖有效高亮下无效交互与缺少 context 的快照，确认高亮、IME context、页面布局和通知保持不变。独立测试覆盖 Rust 失败原子性、Qt Canvas 查询/事件副作用及无效状态拒绝；没有直接 mock 平台 `QInputMethod` 验证系统级更新回调。TextFlow 分支复用正文 IME 的组字事务与 surrounding-text 映射尚未实现，不能据此标为 IME 验收完成。 |
| 2026-11-04 | `cargo test --locked -p caret-math -p caret-engine -p caret-app --no-fail-fast -- --quiet`、三 crate Clippy、`cargo fmt --all -- --check`、native 9/9、include lint、diff check；Terra/medium MathEdit history sidecar review；Sol/medium 完整 diff final review | caret-math 60、caret-engine 392、caret-app 199 项通过；Clippy、格式、native CTest、include lint 与 diff check 通过。历史恢复以解析重建的 Tree/Align sidecar 对照历史 sidecar，覆盖结构路径、范围与 TextFlow 分段映射；restore helper 的 Tree/Align 错配用例含同源码正向基线，确认拒绝时文档不变。Session `Undo` stale-sidecar 用例验证失败后文档、选区、MathEdit、generation、history state 与 undo/redo 可用性均保持原状。TextFlow 输入、删除、选择映射、Run 属性格式、行内环境格式与数学降级的路径/位置读取已改为先验证并消费 `MathSourceMap`；Align cell 的完整源码偏移在映射边界转换。树变换本身仍通过 DTO 瞬时字段更新映射，迁移未完成。审查未确认生产回归。跨相邻 TextFlow IME/选区和性能基线仍未完成。 |
| 2026-11-04 | 修复历史 sidecar 分段归一化后的继续编辑；`cargo test --locked -p caret-math -p caret-engine -p caret-app --no-fail-fast -- --quiet`、三 crate Clippy、`cargo format --check`、`cargo lint includes --changed --check`、native 9/9；Terra/medium follow-up review | caret-math 60、caret-engine 396、caret-app 199 项通过；Clippy、格式、include lint、native CTest 与 diff check 通过。历史恢复仅在 sidecar 形状不同但结构位置一致时重绑映射；相邻连续线性段可规范比较，零宽锚点保留顺序并逐个校验。Session 用例覆盖 Equation 和 Align 非首 cell 的历史恢复后输入；锚点位置失配有拒绝用例。Terra 未发现新的生产问题，建议锚点用例加正向基线，已补充。完整 DTO 范围迁出、覆盖率门槛、性能基线和跨 TextFlow IME/选区仍未完成。 |
| 2026-11-04 | `cargo coverage`、`cargo coverage native`；native CTest 9/9 | Rust 全局函数/行覆盖率 86.90%/88.63%，超过 85% 门槛；native 产品函数/行覆盖率 90.30%/85.17%，超过 85% 门槛。为了覆盖真实快照桥接失败恢复，新增有效/无效 JSON 快照安装后页面与 IME context 保持用例；补充命令行输入光标矩形属性经 Qt 查询返回的用例。 |
| 2026-11-04 | `cargo bench --locked -p caret-engine --bench math_edit_criterion -- --sample-size 10 --warm-up-time 1 --measurement-time 1` | 新增 Equation TextFlow 投影解析与中间单字插入的局部 CPU 基线；单个 ASCII Run。具体输入、测量区间、环境与证据限制见[性能范围](../../tests/performance/rust/README.md)。不代表完整公式编辑性能或优化收益。 |
| 2026-11-04 | Terra/medium 数学 TextFlow 组字架构评审 | 当前 IME 候选、替换与快照上下文都按段落位置工作；MathEdit 分支被整体拒绝组字。仅移除开关会把预编辑作用到外层段落。桌面 IME 需要包含公式目标/树路径的强类型位置、候选 revision 校验与 044 的数学光标/预编辑几何。数学 TextFlow 直接编辑已走局部树事务，但仍借助源码坐标映射，并需 DTO 临时范围参与投影维护；应和 sidecar 迁移协同收敛，再接入组字。评审只读，未运行测试。 |
| 2026-11-04 | `cargo test --locked -p caret-engine --lib redo_rejects_a_missing_math_edit_target_without_consuming_history`、`cargo test --locked -p caret-engine --lib`、`cargo test --locked -p caret-tests --bin caret-tests`、engine Clippy、格式与 diff check | 新增 Session `Redo` 失效公式目标行为用例；先验证有效 `Undo`/`Redo` 基线，再注入过期 math id，确认拒绝重放后文档、选区、MathEdit/revision、generation、history state 与 undo/redo 可用性保持不变。完整 engine 单测 397 项、测试运行器 52 项通过；Clippy、格式与 diff check 通过。 |
| 2026-11-04 | 相邻 TextFlow 规范投影与源码范围刷新；`cargo test --locked -p caret-engine --lib`、engine Clippy、格式与 diff check；Terra/medium、Sol/medium 投影评审 | 规范序列化合并相邻 TextFlow 时，树结构匹配和 MathSourceMap 范围刷新支持连续文本分组及唯一边界切分，包括 InlineEnvironment 子节点；降级映射单独核对 Run 语义，结构比较不误拒绝持久树上的格式/扩展字段。整体范围扩展字段无法无损拆分时拒绝刷新。命令级 Mod+Backspace、历史范围重绑、嵌套环境、非一致 Run 的降级映射拒绝和根层拆分保留未提升数学元数据用例覆盖；engine 403 项通过。DTO 范围桥接、全树可表达性证明及跨 TextFlow IME/选区仍未完成。 |
| 2026-11-04 | 未完成数学源码草稿的关闭与保存提示；Rust 状态 → Qt `LayoutViewModel` → QML；锁定 `lupdate`、Rust/Qt 验证 | 文档状态新增草稿标记；窗口关闭会先提示公式草稿不可保存，隐藏无效 Save，避免自动保存失败后显示内部错误码。直接保存失败映射为可操作的本地化提示。保留丢弃全部文档更改并关闭、取消并继续编辑；英文、简体/繁体中文和日文消息与实际 QML `lupdate` 的 120 条 source 同步。caret-engine 403、caret-app 199、caret-dslc 18 项测试及 native 9/9 通过；三 crate Clippy、Rust/QML 格式、include lint、QM 加载与 diff check 通过。QtTest 覆盖对话框草稿提示、Save 隐藏及四语言实际翻译，真实 App 关闭事件仍待验证。 |
| 2026-11-04 | 空 TextFlow 规范投影/history 恢复；草稿保存错误、状态与 locale；engine 404、app 199、native CTest 9/9、Clippy、Rust/QML 格式、include lint、diff check | Terra/medium review 发现 `[Text("a"), Text("")]` 对齐规范树时被快速路径拒绝，以及 `[Text("a"), Text(""), Text("b")]` 合并时丢失中间零宽位置。刷新现在将尾随空流纳入组映射，并给空 display flow 绑定独立 zero-width sidecar 锚点；测试通过规范重投影、sidecar boundary 查询、重新进入空流并插入。草稿保存错误码与打开替换提示分离；ViewModel 对状态 JSON 类型错误拒绝整笔更新，覆盖草稿标记双向迁移、重复快照无通知及未命名文档保存拒绝。AutoSave/recovery 生命周期缺口已由后续验证行记录的暂停/恢复修复覆盖；真实 Qt timer 与关闭路径仍未验证。 |
| 2026-11-04 | `cargo coverage`、`cargo coverage native`；Sol/medium 当前完整 diff final review | Rust 全局函数/行覆盖率 86.83%/88.61%，native 产品函数/行覆盖率 90.35%/85.19%，均超过 85% 门槛；native CTest 9/9 通过。Sol/medium 未发现新增可操作问题；该结论不代表 task 043 完成，未验证项和生命周期风险仍见本表。 |
| 2026-11-04 | `cargo coverage`、`cargo coverage native` | Rust 全局函数/行覆盖率 86.88%/88.61%，native C++ 90.32%/85.20%，两项均超过 85% 门槛。覆盖率测试运行通过；native 9/9 CTest 通过。 |
| 2026-11-04 | Sol/medium 完整分支差异 final review（`origin/main` 至当时工作树） | 未发现当时增量新增的阻断项。核对范围包括历史恢复/映射校验、失败状态保持、IME 禁用链、快照验证、crate/模块边界、测试与基准注册。未重跑测试；测试与覆盖率依据本地本轮实测及已有分支记录。此结果不代表 043 验收完成；剩余项见本表和下方风险清单。 |
| 2026-11-04 | Sol/medium 本轮完整分支差异 final review（`origin/main` 至当前工作树） | 只读检查生产代码、测试、基准注册与 task 文档；未发现本轮增量新增的阻断问题。未重跑测试。043 的 DTO 范围迁移、跨 TextFlow 编辑与 IME 等验收项仍未完成。 |
| 2026-11-04 | 草稿保存/替换错误路径、Qt 状态投影与翻译；`cargo test --locked -p caret-app --lib -- --quiet`、`cargo clippy --locked -p caret-app --all-targets -- -D warnings`、native CTest、Rust 格式和 diff check；Terra/medium Qt 状态评审 | 保存草稿返回独立稳定错误码；未命名草稿在 Save As 路径检查前被拒绝。QML 区分保存与替换文档提示，四种 locale 均已同步；无效 JSON 类型不改变 ViewModel 投影或发通知，草稿 pending 双向变化与重复快照有测试。App 草稿关闭事件仍未用真实窗口验证。Terra 曾发现 AutoSave/recovery deadline 在草稿期间失败后被清除；已在后续 Rust 生命周期与 Qt 快照响应中加入暂停、恢复及刷新，跨层集成见最新验证行。数学输入仍由 044 门控，真实桌面路径待 Qt 公式输入启用后验证。|
| 2026-11-04 | 草稿与 AutoSave/recovery 生命周期联动；caret-app 生命周期测试 9 项、caret-ffi 13 项、native CTest 9/9；Clippy、Rust/native/QML 格式、include lint、diff check | 草稿 pending 时 Rust 隐藏两类 deadline、递增 generation 并保留底层排期；旧 timer 回调拒绝，取消后 deadline 按剩余时间重现。clean-but-pending recovery 仍更新恢复快照；clean 状态不产生 AutoSave；失败的在途 AutoSave 不因草稿取消自动重试。FFI 依据 session 状态同步 pending；Qt success/error interaction 与 edit response 均刷新文件状态和 timer。ShellLoad QtTest 直接验证两种响应错误后 timer 重新启动。未覆盖真实数学草稿驱动 timer 写入，因为 044 仍门控公式 UI；完整 App 关闭、失焦与过期 revision 验证仍待处理。 |
| 2026-11-04 | 恢复快照身份竞态与冲突关闭等待；caret-app 206、caret-ffi 14、native CTest 9/9、Clippy、Rust 格式、include lint、diff check | 恢复副本绑定携带快照 revision；会话前进时为当前 revision 排期。恢复快照和写入完成携带 binding generation，完成回调校验 session、recovery ID 与代际；绑定变化会重置旧策略。恢复写入还在存储锁内校验路径和基线，防止旧绑定快照覆盖新身份。FFI 用例推进单调时钟验证真实 deadline 捕获，覆盖 A→B→A 后旧 A 同 revision 完成不能消费新 A 的在途写入。手动冲突回调刷新状态和 Qt timer；旧 ID/代际或旧 session 的异步完成发出 activity-finished。关闭状态区分保存请求、保存成功和等待中的文件活动；冲突未解决时 stale 活动不会伪报保存成功，关闭恢复显式检查未提交数学草稿。ShellLoad 检查 QML 连接与状态门；真实 Qt 窗口关闭/冲突异步顺序仍未端到端验证。性能未重新测量，043 的其他未完成项不变。 |
| 2026-11-04 | 恢复写入乱序校验收尾；`cargo test --locked --workspace`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff origin/main --check`、native CTest 9/9、`cargo coverage`、`cargo coverage native`；Terra/medium 恢复路径 review、Sol/medium 完整 diff final review | workspace 测试和 Clippy 通过；Rust 全局函数/行覆盖率 87.03%/88.77%，native 函数/行覆盖率 90.35%/85.04%，均超过 85% 门槛。恢复 envelope 持久化 binding generation 与 document revision；恢复自动写入和手动冲突写入在同一存储锁内校验路径、基线、代际并拒绝低 revision，覆盖 A→B→A 旧 token 与同绑定旧 revision 迟到写入。`RecoverySnapshotIdentity` 统一承载写入身份，减少 FFI/存储调用参数耦合。另覆盖缺少保存目标时 `documentSaveFinished(false)`，防止关闭流程把另存为请求当成保存成功。Terra 与 Sol 均未发现阻断项。真实 App 关闭/Qt timer 异步乱序场景仍未端到端验证；性能未重测；043 其余未完成项见剩余风险。 |
| 待完成 | `caret-layout`/`caret-app` 完整用户路径复验、草稿真实关闭事件、跨映射编辑黑盒、Qt Quick/系统 IME、源码逐键/Align/多 Run 混合文本基线 | 尚未完成。Qt 绘制/命中由 [044](044-math-layout-and-rendering.md) 覆盖；草稿关闭决策已本地化，App 窗口关闭事件和失焦状态仍待验证。没有优化前后对照，不宣称性能提升。 |

## 剩余风险与下一步

1. 迁出所有 `serde(skip)` 的数学源码与位置字段。先让解析在生成干净树时同步生成结构同形的 sidecar，再把路径校验、TextFlow 映射、元数据匹配、根层拆分/降级和编辑事务改为消费或变换成对的树与映射，最后删除 DTO 字段和二次采集/刷新桥接。不要单独删除 `TextFlow.source_range/source_map`：文本合并、格式切分和节点插入必须同步更新树形与映射；共享 `flow_edit` 的合并策略也必须显式区分宿主，不能再靠数学 DTO 临时范围阻止合并。Align cell 局部位置与完整源码坐标的转换要由 sidecar API 明确负责，并保留正文同名 `extra` 字段的普通存储语义。
2. 让合法树可建立规范源码投影并保留可映射身份；覆盖相邻 TextFlow 合并、Font 语义和接近源码长度上限的转义文本。
3. 补齐不完整源码草稿的失焦、实际窗口关闭事件及过期 revision 状态迁移；在 044 提供可交互公式路径后，用 Qt 黑盒验证草稿取消会恢复 timer 并实际写入。现有提交/取消、关闭确认和保存错误提示已本地化。
4. 补齐跨 TextFlow 选区、IME 与 Font 环境的宿主事务；不把策略移入 Qt/QML。
5. 044 接通布局和命中后，再启用桌面公式命令，并执行相关 Qt Quick 黑盒验证。

失败时整笔事务保留文档、编辑目标、焦点与可撤销状态。性能优化只有在代表性输入上测量后才记录为收益。
