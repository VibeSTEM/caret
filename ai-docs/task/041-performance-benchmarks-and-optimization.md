# 041 — 性能基准补齐与热点优化

- 状态：in_progress
- 依赖：024、026、027、030
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-10-29

## 目标与背景

034 已完成首轮本机测量与有数据支持的热点优化。本任务承接后续性能验证和基准补齐：当前 Rust Criterion 基准覆盖打开、排版、输入和删除的部分长文档路径，但撤销/重做缺少规模化数据；Qt CPU 合成场景没有跨页滚动，GPU 也只覆盖有限更新场景。nightly 未保留完整 Criterion 原始统计，且部分吞吐量标注可能把一次编辑误报成处理整篇文档。需要补齐可复现测量，再依据结果优化。

## 必读

- [性能分析任务](034-performance-profiling.md)、[增量排版](../architecture/editor/incremental-layout.md)
- [性能测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

- 新增长文档撤销/重做、保存快照以及 Qt 跨页滚动基准；根据现有代表性输入覆盖 10 万与 100 万 Unicode 字符规模。
- 修正 Criterion throughput，仅对确实处理对应元素数的操作报告元素吞吐；单次编辑报告操作耗时。
- nightly 保存可用于比较的 Criterion 统计数据，同时控制 artifact 体积并记录 runner/构建环境。
- 依据基线数据优化确认的热点；不以无测量依据的重写替代分析。
- Qt 无头/离屏场景仍明确标为合成测试，不宣称覆盖真实窗口、屏幕呈现或端到端 IME 延迟。
- 承接 034 尚未完成的验证：在可用 nightly runner 核验 Qt GPU 实际 renderer/GPU timestamp；在真实桌面窗口对 10 万与 100 万字符文档测量打开、输入、删除和换页。记录平台、机器、配置、操作步骤和数据；无可用 GPU 时间戳或无法在某平台手工操作时如实注明限制。
- 基准只在 nightly/手动运行，不进入 PR CI 和默认测试。

## 前置条件与待决策

- 复核现有 Criterion fixture 与 `CARET_PERF_CDOC` 的实际字符数，报告中区分 fixture 固定规模与外部文件规模。
- 明确 artifact 命名、保留周期和跨 runner 数据比较限制。
- 是否优化保存全树 clone、布局快照失效或其他候选瓶颈由基准结果决定。

## 实施步骤

1. 盘点现有 Rust/Qt benchmark 注册、nightly 执行和 artifact 内容。
2. 补充撤销/重做、保存与跨页场景，并校准样本吞吐定义。
3. 本地和 nightly 运行基准，记录输入、机器、优化配置、样本方法与结果。
4. 对显著热点提出并实施小范围优化，前后使用相同基准验证。
5. 更新 034、nightly 文档及本任务，完成独立 final review。

## 预计改动

`tests/performance/rust/cpu/`、`tests/performance/qt/cpu/`、nightly workflow/artifact、性能测试调度工具、034 性能记录和相关优化模块。

## 验收标准

- [x] 10 万与 100 万字符撤销/重做及保存快照序列化有有效 Criterion 基线。
- [x] Qt CPU benchmark 覆盖视口跨页/可见页范围变化，并清楚声明离屏测试边界。
- [x] Criterion throughput 与各测试实际处理工作量一致。
- [x] Nightly artifact 含 Criterion 原始统计结果，并使用唯一 run ID 命名、保留 30 天。
- [ ] Qt GPU nightly 远端执行已核验实际 RHI backend、renderer 与 GPU timestamp 能力；软件 renderer 不表述为物理 GPU 测量。
- [x] macOS 真实窗口已对 10 万与 100 万字符文档执行打开、输入和删除，并确认 100 万字符文档跨页滚动与内容绘制。
- [ ] 真实窗口的可信端到端操作延迟仍待通过帧/性能采集工具测量；辅助功能调用耗时不能代替应用延迟。
- [x] 所有已实施优化均有相同条件下的前后数据；未验证项明确标注。
- [x] 不新增 PR/default CI 性能 job；性能测试仅从 nightly/手动 suite 进入。
- [x] 格式、静态检查、相关测试和独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-29 | 本机 Apple Silicon，macOS 27.0.1，Rust 1.99.0；`cargo bench --locked -p caret-app --bench document_history_save_criterion -- --sample-size 10 --warm-up-time 0.1 --measurement-time 0.3`，优化前后同命令 | 10 万与 100 万字符撤销、重做、保存快照均有可比样本 | 六个场景均完成。使用同一夹具与计时边界分别测完整布局恢复和增量布局恢复；Criterion 自动延长单项采样目标，每项 10 samples。撤销/重做改善约 99.4%–99.5%；保存路径未作优化，统计差异不归因于本次修改。该小样本仅作本机诊断。 |
| 2026-10-29 | `cargo performance qt cpu --samples 10`；Qt 6.11.2，1280×960，Software renderer | 现有场景保持通过，并新增 400 页跨页滚动帧测量 | 5 个场景均通过。计时包括页位置、可见页范围、字形帧、选区、polish/sync/render；跨页合成滚动中位数 2.047 ms、P95 2.215 ms，10 页访问。它仅测离屏软件场景图，不代表真实窗口帧延迟。 |
| 2026-10-29 | `cargo performance qt gpu --samples 10`；本机 macOS 无头进程 | 确认本地是否可创建 RHI 与 GPU 时间戳 | 基准成功编译，但本机执行因 `Cannot create window: no screens available` 失败；不能据此判断 GPU 后端或时间戳能力。Ubuntu nightly 远端仍待实际执行及读取 artifact。 |
| 2026-10-29 | Xcode Instruments `xctrace` Animation Hitches；尝试附加当前 Caret 进程 | 采集真实窗口帧卡顿样本 | 已确认 Xcode 模板可用，但本机运行环境不能把桌面应用映射到可附加进程（`Cannot find process matching name`）；没有生成追踪样本，真实帧延迟仍未测。 |
| 2026-10-29 | `cargo test --locked -p caret-app single_paragraph_undo_and_redo_publish_incremental_layout_deltas`、`cargo test --locked -p caret-engine` | 局部撤销/重做增量结果与领域历史回归通过 | 通过；app 定向测试 1/1，engine 单元测试 91/91。 |
| 2026-10-29 | macOS release app；1,000,001 码点 `/Users/yuki/Documents/caret/test_1.cdoc` 和 100,001 码点临时副本 | 真实窗口打开并显示大文档；验证输入、删除和换页，避免修改原始测试文档 | 两个规模均打开并绘制多语言内容，1M 文档可滚动查看后续页面。100k 临时副本和 1M 原文件均完成人工输入、退格和两次撤销，标题状态恢复为 `Saved`；Autosave 关闭，原始 1M 文件内容未写回。未使用帧采集器；操作链计时包含桌面事件和辅助功能刷新，不作为应用延迟数据。 |

> 外部 1M 文档的实测文本为 1,000,001 个 Unicode 码点，不等于基准夹具固定的 1,000,000 字符。

### 本机撤销、重做与保存快照

每个基准的 Criterion sample size 为 10；每项实际测量时间由 Criterion 依据单次操作耗时自动延长。数字为 Criterion 估计值，不作为跨机器性能门槛。100k 场景为 500 段、100,000 个正文 Unicode 字符；1M 场景为 5,000 段、1,000,000 个正文 Unicode 字符。使用 `iter_batched_ref` 与 `BatchSize::PerIteration`：打开会话、定位光标、准备编辑状态，以及每次迭代后的整份 Session 释放均不计入撤销/重做计时；测量操作包含单段重排与更新 UI 增量 JSON。优化前基线使用相同夹具和计时边界，仅将历史恢复后的布局计算临时切回完整快照。保存快照包括克隆文档信封、序列化和 SHA-256 摘要，不包含磁盘 I/O；保存路径没有随本次历史优化改变。

| 场景 | 优化前 | 优化后 | 改善 | 结果 |
| --- | ---: | ---: | ---: | --- |
| 100k 撤销单字符 | 55.629 ms | 0.331 ms | 99.40%（p<0.05） | 单段布局复用并返回增量快照 |
| 100k 重做单字符 | 60.998 ms | 0.323 ms | 99.47%（p<0.05） | 同上 |
| 100k 保存快照序列化 | 1.693 ms | 1.706 ms | 无显著变化（p=0.45） | 不改写保存路径 |
| 1M 撤销单字符 | 583.07 ms | 3.223 ms | 99.45%（p<0.05） | 单段布局复用并返回增量快照 |
| 1M 重做单字符 | 548.11 ms | 2.826 ms | 99.48%（p<0.05） | 同上 |
| 1M 保存快照序列化 | 17.971 ms | 17.116 ms | 测得约 4.8% 差异；保存实现未改变，不归因于本优化 | 不改写保存路径 |

优化前后使用相同引用式批处理和计时边界；Criterion 均值估计的差值与百分比只代表本机这次 10 样本诊断，不作为跨机器性能承诺。保存快照变化没有对应的代码优化，观测差异不解释为本次性能收益。

## 风险与回退

Criterion 跨机器数据不可直接作为硬门槛；基准应保存原始数据并避免将共享 runner 的噪声包装成确定提升。若新增场景明显延长 nightly，可调整采样次数，不将性能基准移入 PR CI。

## 决策与工作记录

- 2026-10-29：接收 034 未完成项：Qt GPU nightly 远端核验与大文档真实窗口打开、输入、删除、换页端到端测量。034 已完成原定的本机测量及有证据的热点优化；041 独立推进，不等待 034。
- 2026-10-29：为 `Effect` 补充仅在历史补丁恰好影响同一段落时提供的段落 ID；单段撤销/重做复用未修改段落排版并返回布局增量，多段或结构性变化仍生成完整快照。修正计时边界后，单机 100k/1M 撤销与重做的同机对比改善约 99.4%–99.5%；保存序列化没有对应代码改动。
- 2026-10-29：修正打开基准的吞吐量仅标记完整解码/打开操作；单字素删除移出整篇吞吐分组，并记录外部 `.cdoc` 的字节和已知文本字符数。撤销/重做和保存快照基准固定覆盖 100k/1M 字符。
- 2026-10-29：Qt CPU suite 增加 400 页视口跨页滚动合成场景；把页/字形/选区状态安装计入跨页场景计时。Nightly Rust CPU artifact 额外保留 `target/criterion/**/new/*.json`，30 天过期，独立于 PR/default CI。修复性能 job shell 对前序基准失败可能被后续命令掩盖的问题。
- 2026-10-29：独立 final review 指出 Session 析构误计时和 Qt 跨页更新未计时；改用 `iter_batched_ref + PerIteration`、将场景安装纳入计时并重跑同口径对照。复审确认修复，完整 diff 无新增问题。

## 完成摘要

基准入口、统计归档和单段撤销/重做优化已完成并取得同口径本机对比。已在真实窗口验证 100k/1M 打开、输入和删除，并检查 1M 跨页滚动。可信端到端延迟与 Ubuntu nightly 的 Qt RHI renderer/GPU timestamp 核验仍待完成。合成离屏数据不替代这些验证。
