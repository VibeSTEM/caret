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
- [ ] 补齐源码草稿状态迁移：`MathSourceDraft` 已独立保存未完成源码与有界局部撤销历史；解析不完整或候选树元数据无法无损映射时，已提交树、焦点和文档 generation 保持原样，输入作为交互状态返回。局部撤销/重做优先于文档历史；草稿回到基线后才可进入文档历史。保存和异步打开拒绝丢弃未提交草稿，dirty 状态包含草稿；Escape 显式取消并恢复基线光标。仍需补齐失焦/关闭 UI 流程、过期 revision 生命周期及 Qt 黑盒行为验证。
- [ ] 完成跨相邻 TextFlow 的选区/组合字素、数学文本 IME 与 Font 环境树编辑；桌面公式命令的可执行状态等待 044 的布局/命中链路。
- [ ] 保证 TeX 可无损表达且未超过源码预算的 MathTree 都能建立规范编辑投影；结构归一化须能处理相邻 TextFlow 合并并保留可映射身份，对不支持 Font 语义或超过源码预算的树明确拒绝且不改动文档/焦点。

## 验收

- [x] 三种公式载体使用稳定 ID、未知字段和版本化 Caret schema；paragraph/math content-flow 能力校验完整。
- [x] 数学节点的编码 schema 只含结构化树、行列和格式，不含 TeX/sourceRange/sourceMap；旧双表示和退役字段在入口拒绝。
- [ ] InlineMath、Equation、Align 的完整 `.cdoc` 保存/重开与 ID/未知字段往返；目前公式文档仍会进入只读预览，待保存能力链路启用后验收。
- [x] 编号环境、自动编号、`\label`、`\ref` 由解析器拒绝且不改动文档。
- [x] 普通 TextFlow 编辑、数学单 TextFlow 输入/删除/Bold/Italic/underline、InlineMath 根层 Math/Text 焦点拆分/降级、Align 范围与 undo/redo 有命令级行为用例；失败不提交部分树、位置或历史状态。
- [x] 清理持久化公式源码副本；MathEdit 历史投影以目标身份和历史源码恢复，规范源码不依赖 DTO 字段恢复。
- [ ] 清理 DTO 临时范围桥接，并让映射仅由与目标树状态绑定的 MathEdit sidecar 持有。
- [ ] 完成并测试源码草稿的失焦/保存/关闭决策与过期 revision 状态迁移；当前保存和打开会保留文档并拒绝，Escape 可取消，公式 UI 仍等待 044 接线。
- [ ] 完成跨 TextFlow 选区/IME、Font 数学环境及 044 依赖后的公式命令调用链。
- [ ] 按验证规范完成 Rust/native C++ coverage gate、最终全分支审查和尚未覆盖的黑盒检查。性能结论需另有可复现基线；当前不宣称性能提升。

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
| 待完成 | Rust/native C++ coverage gate、`caret-layout`/`caret-app` 直接消费者复验、Session 级跨映射编辑黑盒、Qt Quick/IME、公式 UI 错误本地化、公式性能基线 | 尚未完成。Qt 绘制/命中由 [044](044-math-layout-and-rendering.md) 覆盖；当前阻止保存的草稿错误尚未接入本地化提示。没有测量证据，不宣称性能提升。 |

## 剩余风险与下一步

1. 迁出所有 `serde(skip)` 的数学源码与位置字段。保留现有树编辑行为，令同次解析产生的 `MathSourceMap`/树位置 sidecar 成为唯一范围权威；修改 DTO 后检查模型构造、校验、序列化、历史、根层提升和所有直接消费者，避免为删除字段而重解析整树。
2. 让合法树可建立规范源码投影并保留可映射身份；覆盖相邻 TextFlow 合并、Font 语义和接近源码长度上限的转义文本。
3. 补齐不完整源码草稿的失焦/关闭决策、提交/取消与失效状态迁移，并在 UI 显示可操作的本地化提示。
4. 补齐跨 TextFlow 选区、IME 与 Font 环境的宿主事务；不把策略移入 Qt/QML。
5. 044 接通布局和命中后，再启用桌面公式命令，并执行相关 Qt Quick 黑盒验证。

失败时整笔事务保留文档、编辑目标、焦点与可撤销状态。性能优化只有在代表性输入上测量后才记录为收益。
