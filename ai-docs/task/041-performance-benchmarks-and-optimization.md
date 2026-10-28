# 041 — 性能基准补齐与热点优化

- 状态：planned
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

- [ ] 10 万与 100 万字符撤销/重做及保存路径有有效 Criterion 基线。
- [ ] Qt CPU benchmark 覆盖视口跨页/可见页范围变化，并清楚声明离屏测试边界。
- [ ] Criterion throughput 与各测试实际处理工作量一致。
- [ ] Nightly artifact 含足以追踪基准变化的统计结果，并有合理体积/保留策略。
- [ ] Qt GPU nightly 远端执行已核验实际 RHI backend、renderer 与 GPU timestamp 能力；软件 renderer 不表述为物理 GPU 测量。
- [ ] 真实窗口中已对 10 万与 100 万字符文档执行打开、输入、删除和换页测试，记录用户可感知卡顿与可采集耗时；无法取得的量明确标为未测。
- [ ] 所有优化均有相同条件下的前后数据，未验证项明确标注。
- [ ] 不新增 PR/default CI 性能 job；性能测试仅从 nightly/手动 suite 进入。
- [ ] 格式、静态检查、相关测试和独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 待实施后填写 | 基准可重复，nightly 结果可追踪，优化有实测依据，真实窗口与 Qt GPU 限制清楚记录 | 未执行 |

## 风险与回退

Criterion 跨机器数据不可直接作为硬门槛；基准应保存原始数据并避免将共享 runner 的噪声包装成确定提升。若新增场景明显延长 nightly，可调整采样次数，不将性能基准移入 PR CI。

## 决策与工作记录

- 2026-10-29：接收 034 未完成项：Qt GPU nightly 远端核验与大文档真实窗口打开、输入、删除、换页端到端测量。034 已完成原定的本机测量及有证据的热点优化；041 独立推进，不等待 034。

## 完成摘要

未完成。
