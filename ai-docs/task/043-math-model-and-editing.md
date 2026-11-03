# 043 — M3：公式文档模型与编辑

- 状态：in-progress
- 依赖：010、042
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-03

## 目标与边界

建立 Caret 自有的可编辑数学树，并实现公式创建、内部编辑、位置和键盘行为。数学文档模型、行内/块环境、宿主适配与持久化设计以[数学公式架构](../architecture/editor/math.md)和[文档模型](../architecture/editor/document-model.md)为权威；[011](011-impl-math.md) 管理总体验收。解析器和排版后端不得成为 `.cdoc` 的第二份状态。

043 覆盖 `caret-doc`、`caret-math`、`caret-engine` 的模型、TeX 适配和编辑事务。页面绘制、布局、命中及光标几何归 [044](044-math-layout-and-rendering.md)；表格单元格模型/编辑与画布集成归 [045](045-table-model-and-editing.md)/[046](046-table-layout-and-canvas.md)。首版不支持自动编号、`\label`、`\ref` 或 IME 数学源码输入；字体命令资源与 fallback 归 [065](065-math-text-font-environments.md)。

## 完成状态

- [x] 以 Caret 节点表示 InlineMath、Equation、Align 与 MathFlow；schema v3 和所需能力声明已接入。`.cdoc` 只写结构树、行列和格式，不写 TeX/sourceRange/sourceMap；旧双表示 schema 与已退役字段在存储边界拒绝，不迁移。
- [x] 公式 TextFlow 复用正文 Run、Run 属性和字素边界；单 TextFlow 输入/删除、Bold/Italic、Run underline、InlineMath 根层拆分及 `Mod+Backspace` 降级走宿主事务。engine 的 Align 导航与元数据匹配消费 `caret-math` 同次解析提供的范围。
- [x] MathEdit 投影绑定 session 与 document generation；历史快照共享 `Arc<MathSourceMap>`，恢复校验源码/种类/路径，过期编辑拒绝并可显式放弃。插入、删除、源码解析失败、未知字段和不可无损映射等边界已有行为回归。
- [x] InlineMath 根层 TextFlow 提升为 ParagraphContent，保留顺序、空文本、Run 属性及单层 InlineEnvironment 身份；固定 MathNode 操作数槽位不提升。
- [ ] 将 DTO 中 serde 跳过的临时源码和节点/TextFlow 范围字段迁出模型：`MathTree.source`、`Align.source`、`MathSourceRange`、`TextFlow.sourceMap` 等改由 MathEdit 投影 sidecar 持有。当前 `.cdoc` 不持久化这些字段，但编辑器仍在 DTO 上桥接它们。
- [ ] 将源码输入改为可暂存的不完整 TeX 草稿：解析失败时保留草稿、已提交树和焦点；失焦/保存/关闭先提交或显式取消；草稿 undo/redo 可修复输入；本地历史耗尽后，仅在有效草稿成功提交时访问文档历史，否则保留草稿并拒绝迁移；过期草稿拒绝提交并保留到显式放弃。当前逐次编辑解析失败会保留旧树和活动焦点，但不保留失败的源码输入。
- [ ] 完成跨相邻 TextFlow 的选区/组合字素、数学文本 IME 与 Font 环境树编辑；桌面公式命令的可执行状态等待 044 的布局/命中链路。
- [ ] 保证 TeX 可无损表达且未超过源码预算的 MathTree 都能建立规范编辑投影；结构归一化须能处理相邻 TextFlow 合并并保留可映射身份，对不支持 Font 语义或超过源码预算的树明确拒绝且不改动文档/焦点。

## 验收

- [x] 三种公式载体使用稳定 ID、未知字段和版本化 Caret schema；paragraph/math content-flow 能力校验完整。
- [x] 数学节点的编码 schema 只含结构化树、行列和格式，不含 TeX/sourceRange/sourceMap；旧双表示和退役字段在入口拒绝。
- [ ] InlineMath、Equation、Align 的完整 `.cdoc` 保存/重开与 ID/未知字段往返；目前公式文档仍会进入只读预览，待保存能力链路启用后验收。
- [x] 编号环境、自动编号、`\label`、`\ref` 由解析器拒绝且不改动文档。
- [x] 普通 TextFlow 编辑、数学单 TextFlow 输入/删除/Bold/Italic/underline、InlineMath 根层拆分/降级、Align 范围与 undo/redo 有命令级行为用例；失败不提交部分树、位置或历史状态。
- [ ] 清理 DTO 临时源码/范围桥接，并让映射仅由与目标树状态绑定的 MathEdit sidecar 持有。
- [ ] 实现并测试不完整源码草稿的编辑历史与失焦、保存、关闭、过期 revision 状态迁移。
- [ ] 完成跨 TextFlow 选区/IME、Font 数学环境及 044 依赖后的公式命令调用链。
- [ ] 按验证规范完成 Rust/native C++ coverage gate、最终全分支审查和尚未覆盖的黑盒检查。性能结论需另有可复现基线；当前不宣称性能提升。

## 当前验证与评审

| 日期 | 范围与检查 | 结果及限制 |
| --- | --- | --- |
| 2026-11-03 | `cargo test --locked -p caret-engine -p caret-math -p caret-doc --no-fail-fast -- --quiet` | caret-engine 373、caret-math 51、caret-doc 58 项通过。覆盖公式文本树事务、格式/拆分、Align trivia 与非规范包装、metadata ID 保留、投影一致性及实际 Session undo/redo 焦点恢复。 |
| 2026-11-03 | `cargo clippy --locked -p caret-math -p caret-engine --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff HEAD --check` | 全部通过。 |
| 2026-11-03 | Terra/medium Align 编辑与投影评审；Sol/medium `cc439ee..ac0faf5` 独立完整 diff final review | 坐标归一化、投影克隆、源码/投影一致性及 Session 历史边界问题均已修复；该切片无 P0–P3。Reviewer 未运行命令，验证结论以本表实际命令为准。 |
| 2026-11-03 | `cargo test --locked -p caret-math -p caret-engine -p caret-app`、三 crate Clippy、`cargo fmt --all -- --check`、`git diff --check` | math 50、engine 365、app 196 项通过；覆盖此前 Math TextFlow 树事务与 app 直接消费者。当前切片后 app/layout 直接消费者仍待复验。 |
| 2026-11-02 | PR #87 CI 的 `cargo check and build` / Clippy 暴露 `formula_text_environment_semantics_match_source_and_keep_extensions` 测试 E0502 | 通过限制测试内可变借用修复；定向测试、`cargo check --offline --locked --workspace --all-targets`、engine Clippy、格式和差异检查通过。 |
| 2026-11-01 | `cargo test --offline --locked --workspace` | Rust 单元/集成测试通过；native 7/8 通过，`ShellLoad::documentPathAdapterReturnsLocalPathsAndDefaultSaveLocation` 因 `default_url.isLocalFile()` 断言失败，原因待查，详见[064](064-builtin-font-roles.md)。 |
| 待完成 | Rust/native C++ coverage gate、`caret-layout`/`caret-app` 直接消费者复验、Qt Quick/IME 黑盒、公式性能基线 | 尚未完成。Qt 绘制/命中由 [044](044-math-layout-and-rendering.md) 覆盖；基准需记录代表公式、构建配置、分配/扫描/内存及序列化测量。 |

## 剩余风险与下一步

1. 迁出所有 `serde(skip)` 的数学源码与位置字段。保留现有树编辑行为，令同次解析产生的 `MathSourceMap`/树位置 sidecar 成为唯一范围权威；修改 DTO 后检查模型构造、校验、序列化、历史、根层提升和所有直接消费者，避免为删除字段而重解析整树。
2. 让合法树可建立规范源码投影并保留可映射身份；覆盖相邻 TextFlow 合并、Font 语义和接近源码长度上限的转义文本。
3. 实现不完整源码草稿及其本地撤销、提交/取消与失效状态迁移。
4. 补齐跨 TextFlow 选区、IME 与 Font 环境的宿主事务；不把策略移入 Qt/QML。
5. 044 接通布局和命中后，再启用桌面公式命令，并执行相关 Qt Quick 黑盒验证。

失败时整笔事务保留文档、编辑目标、焦点与可撤销状态。性能优化只有在代表性输入上测量后才记录为收益。
