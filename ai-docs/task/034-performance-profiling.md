# 034 — Qt / Rust 性能测量与热点优化

- 状态：implemented
- 依赖：024、026、027、030
- 优先级：P2
- 创建 / 更新：2026-10-27 / 2026-10-27

## 目标与背景

为当前文本编辑主链建立可复现的性能基线，分别测 Rust 与 Qt 的 CPU 工作，并在无可见窗口的环境测量 Qt Quick GPU 渲染。根据采样结果优化实际热点，记录同一输入、同一 runner 和同一配置下的优化前后数字。性能采样只由 nightly 运行，不进入 PR / push CI，也不设置毫秒门禁。

当前 Rust 侧拥有 Unicode 分段、字体回退、Parley 排版、命中映射和 Swash 字形栅格；Qt 侧拥有输入事件、布局快照转换、场景图节点、纹理上传和页面绘制。长篇输入固定在[共享文本夹具](../../tests/performance/fixtures/novel-1m-multilingual.txt)：5,000 个段落、每段 200 个 Unicode 字符，共 1,000,000 个字符，覆盖中文、英语、法语、阿拉伯语、俄语、希腊语和日语。该纯文本仅作为共同 Benchmark 数据；Word 对照测试独立于仓库和 CI。Rust 目前没有 GPU 执行后端，不建立空基准；Qt GPU suite 覆盖 Qt Quick/RHI 渲染链路。

## 必读

- [测试规范](../standards/testing.md)、[持续集成](../standards/ci.md)、[验证与评审](../standards/validation-and-review.md)
- [文字塑形](../architecture/editor/text-shaping.md)、[页面画布](../task/027-canvas-and-hit-testing.md)、[运行时](../architecture/core/runtime.md)

## 范围与非目标

- Rust CPU：以多语言文档采样 Parley 排版、视觉命中与字形栅格冷/热缓存路径，并以百万字符长篇采样首次整篇排版和单段编辑后的重排。
- Qt CPU：采样 Caret 画布的 CPU 场景图准备、页面/字形几何生成与选区更新；不把 GPU 等待混算成 CPU 工作。
- Qt GPU：通过 `QQuickRenderControl` 离屏绘制 Caret `DocumentCanvas`，记录实际 RHI 后端、renderer、GPU 帧时间和吞吐；headless 运行，不启动桌面窗口。
- Rust GPU：当前没有 Rust GPU 工作负载，不建立空 suite；Rust GPU 绘制工作属于 Qt Quick/RHI。
- 文件按 `tests/performance/rust/cpu/`、`tests/performance/qt/{cpu,gpu}/` 组织；nightly 各 suite 独立运行并上传带输入规模、优化级别、runner、Qt/RHI backend、renderer、字体与提交 SHA 的结果。性能结果用于人工比较，不失败门禁。
- 依据基线修复或优化真实热点，重跑同一基准对比；保持 Rust 领域语义、文字绘制精度和 Qt/Rust 职责边界。

不包含：PR CI 性能作业、跨不同 runner/驱动的阈值比较、宣称软件渲染器结果代表物理 GPU、Rust GPU 空实现、与已测热点无关的大规模重构。

## 前置条件与待决策

- nightly Linux runner 已安装 Mesa/EGL 前置；需确认 `QQuickRenderControl` 在 headless OpenGL/RHI 下能启动并报告 renderer 与 GPU 时间。
- Qt GPU 时间通过 Qt 6.11 `QRhiCommandBuffer::lastCompletedGpuTime()` 读取；设备不支持 GPU timestamp 时明确跳过 GPU 时间采样，但仍记录无头渲染可用性，不以 CPU frame time 伪装。
- GitHub hosted runner 的 renderer 可能是 Mesa llvmpipe 等软件设备；报告必须区分 deviceType 和 renderer，因此该结果用于稳定回归趋势与路径诊断，不等同独立硬件 GPU 的绝对性能。
- nightly 结果不自动改门禁。若 runner 噪声过高，只做 artifact 报告并在独立稳定硬件上复测。

## 实施步骤

1. 建立本任务分支并检查已有 Criterion、Qt Quick offscreen 渲染与 nightly 任务，固定测试数据和元数据格式。
2. 建立 Rust CPU 与 Qt CPU / Qt GPU 独立基准入口，确保默认测试和 PR CI 不执行基准。
3. 在同一 runner / workload 上收集初始结果和 profiler 采样，识别热点及其影响范围。
4. 仅优化有测量证据的热点；同时运行对应正确性测试，并记录优化前后数据与限制。
5. 将独立 nightly suites 和 artifact 接入 `nightly.yml`，确认日常 `ci.yml` 没有性能任务。
6. 由 subagent 独立评审完整 diff，修复发现并复验。

## 预计改动

`tests/performance/`、相关 crate 的 bench 注册、可选的 native 手动基准目标、`.github/workflows/nightly.yml`、`ai-docs/standards/testing.md`、本任务与索引。基准不应注册到默认 CTest；native 源文件仍通过项目既有格式和 lint 检查。

## 验收标准

- [x] nightly 将 Rust CPU、Qt CPU、Qt headless GPU 分别报告并独立上传；Rust GPU 不建立空测试。
- [x] 测量报告带有可解释性能所需的输入规模、配置、硬件/renderer 和 commit 信息；GPU 时间缺失时不会以 CPU 时间替代。
- [x] 性能基准不进入 PR / push CI、默认 `cargo test` 或默认 CTest，也没有毫秒门槛。
- [x] 对实测候选热点做了同输入前后试验；Criterion 未确认显著收益，因此撤回试验性生产改动并记录数据，未保留无证据优化。
- [x] 独立 review 完成；验证计划、工作记录和 task index 与实现一致。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-27 | Rust CPU Criterion：共享百万字符混合语言文本，10 samples | 输出整篇排版、单段重排及小型排版/命中/字形缓存统计 | 同机测得整篇排版约 515 ms、单段编辑重排约 15.1 ms；现有复用路径约快 34 倍。跳过 Session 已保证有效的重复校验后 Criterion 未检测出显著差异，已撤回该生产代码改动 |
| 2026-10-27 | Qt CPU：1280×960，1,000 glyph placements，96 unique images，12 selection rectangles，31 samples | 无头软件场景图输出中位数和 P95 | 多次本机采样中位数 787–914 μs/帧，P95 860–997 μs/帧；波动明显，节点复用试验无稳定收益，未保留生产代码改动 |
| 2026-10-27 | Qt GPU：Qt Quick / OpenGL RHI | 离屏渲染并记录 backend、renderer、device type 和 GPU 时间或明确不支持 | GPU benchmark 可编译；当前 macOS 无屏幕环境不能本机运行。nightly 改由 Xvfb 提供虚拟 X11 context，仍通过 QQuickRenderControl 离屏渲染，待 Linux nightly 验证 |
| 2026-10-27 | 测试范围、Rust CLI 与性能 target | Rust benchmark 按 Cargo target 路由，Qt benchmark 不触发 PR native suite | `cargo test -p caret-tests --bin caret-tests` 通过；Rust 和 Qt 手动 benchmark 目标均可构建 |
| 2026-10-27 | PR / push 与 nightly workflow 检查 | PR / push workflow 不运行 benchmark；nightly 分别运行三个适用 suite | 配置检查与 selector 单测通过；尚未远程运行新增的 Linux GPU job |

## 风险与回退

软件渲染、驱动差异、字形缓存热身及调度器负载会让绝对数字不可直接比较。报告记录这些元数据，不在异构环境设置阈值。若 Qt RHI 暴露的 GPU 计时不可用，保留离屏渲染 smoke 和明确的缺项说明；不回退为误导性 wall-clock 指标。所有优化保持正确性测试先行，可以逐项 revert。

## 决策与工作记录

- 2026-10-27：创建任务。Rust GPU 当前不适用；Qt GPU 通过无头 RHI 离屏渲染采样。

## 完成摘要

仓库实现与本地验证完成。性能 suite 已按 Rust CPU、Qt CPU、Qt GPU 分离；夜间报告包含工作负载、CPU、构建配置、字体清单摘要、Qt RHI renderer 和提交信息。百万字符混合语言文本作为 Rust/Word 的共同 Benchmark 数据，Word 对照独立进行，不接入仓库测试。没有保留缺少稳定收益证据的产品代码改动。

限制：Qt GPU 基准已本地编译，但 macOS 无屏幕环境不能运行；Linux nightly 通过 Xvfb 提供虚拟 X11 context 后是否能取到 GPU timestamp，仍需首次远程运行确认。
