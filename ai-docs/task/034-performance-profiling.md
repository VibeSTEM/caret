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
- Qt CPU：采样 Caret 画布的 CPU 场景图准备、页面/字形几何生成与选区更新；覆盖单页与 400 页文档视口；不把 GPU 等待混算成 CPU 工作。
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
- [x] Qt 场景图只保留可视页和一页预读范围的页面背景节点；同机 400 页场景完成优化前后对比。
- [x] 独立 review 完成；验证计划、工作记录和 task index 与实现一致。

## 性能优化结果总结

- **普通输入**：选区完全落在同一段时，Rust 只重新塑形该段，保留其他段落的 Parley 结果；跨段选区仍走完整排版。每次编辑后的分页和页面坐标仍由 Rust 计算。
- **Rust → Qt 快照**：普通单段输入传递首个实际发生变化的页面起的后缀，以及当前段落；Rust 对比编辑前后的分页快照，避免孤行控制或后续重排改变更早页面时保留旧片段。Qt 使用缓存的完整快照合并增量，并校验基准会话与文档代际；失配时向 Rust 重取完整快照。结构修改、跨段输入和初始化仍使用完整快照。
- **Qt 页面场景**：页面背景节点限制在可见范围及一页预读内；滚动期间仅可视页范围变化时重新栅格化，滚动结束再对齐最终像素相位。相同字形纹理在布局更新之间复用。
- **同一台 Apple M4 的 10 万字符测试**：Rust 输入响应中位数从整篇排版的 34.854 ms 降至段落复用后的 3.079 ms，再降至增量快照后的 1.406 ms；UI 响应从 656,962 字节降至 2,144 字节（99.67%）。这只测 Rust 命令与响应构造，不含 Qt 主线程解析、场景图绘制和显示器呈现。
- **未保留的尝试**：复用每个 `QSGImageNode` 的实验在无头软件场景基准里没有显示稳定收益，因此撤回；避免保留没有测量依据的场景图复杂度。
- **当前限制**：普通输入仍在 UI 调用链同步执行，并在 Rust 内重新计算分页；异步分页或增量分页还未实现。需要用真实窗口和 10 万字符文档复测用户感知滚动及输入延迟。

### 10 万字交互采样

- 使用 macOS `/usr/bin/sample` 对实际 `caret` 进程采样 25 秒，1 ms 采样间隔；期间完成 100,499 字符文档粘贴并滚动画布。报告保存在本机临时目录，不纳入仓库。
- 主线程共 17,872 个样本，其中约 15,062 个在 AppKit 等待下一个事件（约 84%）；`QSGRenderThread` 的 17,273 / 17,872 个样本在 `QWaitCondition::wait`（约 97%），`renderSceneGraph` 约 240 个样本。这个窗口内未观察到持续的 GPU 或场景图线程饱和。
- 主线程的可见应用工作集中在 `PageSurface.qml` 的可视页字形栅格化调用：采样栈经过 `LayoutViewModel::rasterizePageGlyphs`（约 770 个样本）、Rust `DocumentViewRuntime::rasterize_page_glyphs`（约 707 个样本）和 `PageGlyphRasterizer::rasterize`（约 677 个样本）；其中 `colorize_alpha` 约 169 个样本。该路径同步从 QML 调用 Rust，并为字形覆盖率生成着色 RGBA 像素。
- **判断**：继续优化方向应先聚焦可视页字形栅格化的触发频率、缓存命中与像素着色成本，以及它在主线程同步执行造成的短时阻塞；现有数据不支持优先重写 Rust 整篇布局或 GPU 绘制。
- **采样限制**：该报告是含大量空闲时间的进程级抽样，不是逐帧追踪，也没有可用的 Xcode `xctrace` 或 `cargo flamegraph` 生成 SVG 火焰图。样本计数只能指出热点方向，不能换算为单帧耗时或证明用户感受到的卡顿都来自此路径；后续应在连续滚动/连续输入场景下采集 Qt 主线程事件耗时和帧时间，再评估异步栅格化或跨页面缓存策略。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-27 | Rust CPU Criterion：共享百万字符混合语言文本，10 samples | 输出整篇排版、单段重排及小型排版/命中/字形缓存统计 | 同机测得整篇排版约 515 ms、单段编辑重排约 15.1 ms；现有复用路径约快 34 倍。跳过 Session 已保证有效的重复校验后 Criterion 未检测出显著差异，已撤回该生产代码改动 |
| 2026-10-27 | Rust CPU Criterion：100,499 Unicode 字符、500 段；每个样本重建相同文档，末段输入一个字符并生成 UI 响应；Apple M4，release profile，10 samples | 覆盖普通文本输入、排版、分页投影和 UI 数据传输 | 整篇重排并发送完整快照中位数 34.854 ms；复用未变段落、仍发送完整快照 3.079 ms；再发送受影响段落和首个受影响页起的分页后缀 1.537 ms。最后方案响应从 656,962 B 降至 2,115 B（99.68%）。只代表 Rust FFI 响应，不含 Qt 解析、绘制与设备呈现 |
| 2026-10-27 | Rust CPU Criterion 复测：100,499 Unicode 字符、500 段；每个样本重新准备同一文档，单字符输入和增量 UI 响应；Apple M4，release profile，10 samples | 排除每样本运行时析构，重测最终分页差异算法 | 中位数 1.406 ms（区间 1.349–1.471 ms）；增量响应 2,144 B，对比完整快照 656,962 B，减少 99.67%。只代表 Rust 命令与响应构造 |
| 2026-10-27 | Caret 实际窗口：粘贴 100,499 个 Unicode 字符、500 段的混合中英法阿拉伯俄语希腊日文文本 | 确认真实 UI 粘贴、绘制及滚动后的内容保持情况 | 粘贴调用约 2.06 s；画布显示多语言段落，滚动后仍可见后续页面；滚动仍略卡。配合 macOS `sample` 找到可视页字形栅格化是主线程内最明确的应用热点，详见“10 万字交互采样”；此手工结果不代替帧时间基准 |
| 2026-10-27 | Qt CPU：1280×960，1,000 glyph placements，96 unique images，12 selection rectangles，31 samples | 无头软件场景图输出中位数和 P95 | 多次本机采样中位数 787–914 μs/帧，P95 860–997 μs/帧；稳定静态字形/选区场景 |
| 2026-10-27 | Qt CPU：同一 1280×960 视口、1,000 placements、96 images、12 个位于当前视口的选区；每帧移动一个 glyph；400 页，31 samples；macOS 27.0.1、Apple M4、Qt 6.11.2、Software renderer | 对比整篇页面背景节点/字形纹理重建与可视页节点/可复用纹理 | 页面背景节点从 800 个降为 6 个（可视页加一页预读）；中位数 2,092.667 → 1,443.833 μs/帧（改善 31.0%），P95 2,373.917 → 1,606.625 μs/帧（改善 32.3%）。单页场景中位数 2,212.750 → 2,288.000 μs/帧，未显示收益。这是合成场景的软件渲染测量，不代表百万字符端到端渲染时间 |
| 2026-10-27 | Qt GPU：Qt Quick / OpenGL RHI | 离屏渲染并记录 backend、renderer、device type 和 GPU 时间或明确不支持 | GPU benchmark 可编译；当前 macOS 无屏幕环境不能本机运行。nightly 改由 Xvfb 提供虚拟 X11 context，仍通过 QQuickRenderControl 离屏渲染，待 Linux nightly 验证 |
| 2026-10-27 | 测试范围、Rust CLI 与性能 target | Rust benchmark 按 Cargo target 路由，Qt benchmark 不触发 PR native suite | `cargo test -p caret-tests --bin caret-tests` 通过；Rust 和 Qt 手动 benchmark 目标均可构建 |
| 2026-10-27 | PR / push 与 nightly workflow 检查 | PR / push workflow 不运行 benchmark；nightly 分别运行三个适用 suite | 配置检查与 selector 单测通过；尚未远程运行新增的 Linux GPU job |

## 风险与回退

软件渲染、驱动差异、字形缓存热身及调度器负载会让绝对数字不可直接比较。报告记录这些元数据，不在异构环境设置阈值。若 Qt RHI 暴露的 GPU 计时不可用，保留离屏渲染 smoke 和明确的缺项说明；不回退为误导性 wall-clock 指标。所有优化保持正确性测试先行，可以逐项 revert。

## 决策与工作记录

- 2026-10-27：创建任务。Rust GPU 当前不适用；Qt GPU 通过无头 RHI 离屏渲染采样。
- 2026-10-27：实测发现 `updatePaintNode` 按文档总页数创建并逐帧更新页面背景节点；段落布局结果更新时还会重建页面全部字形纹理。增加 QML 可视页区间与预读页，让场景图节点数量随视口增长；字形列表变化时复用同索引且像素内容未变的纹理。滚动只在可视页范围变化时刷新字形，停止后按当前位置刷新相位。
- 2026-10-27：长文档输入审查发现普通 `insertText` 每个字符都会重新塑形整篇文档；此前单段重排基准只覆盖 IME 预览路径。对选区完全位于同一段的普通文本输入复用其余段落的排版，跨段选区仍走整篇布局以保持正确性；增加固定 10 万字符文档的 UI 输入端点基准，实测 Rust 命令与完整快照生成从 34.854 ms 降至 3.079 ms。Qt 仍需解析完整快照，后续需再测真实 GUI 线程路径。
- 2026-10-27：继续缩减普通输入 FFI 数据：单段输入发送完整分页中受影响段及起始页后的分页后缀；Qt 以已有解析快照为基准原子合并，校验文档/字体代际与页面边距。100k 文档响应从 656,962 B 降至 2,115 B，Rust 端中位数降至 1.537 ms。结构命令和跨段编辑仍返回完整快照；分页仍在 Rust 内重新计算。Qt 集成测试覆盖 100 段多页文档，确认前置段落与页面保留、当前段更新。
- 2026-10-27：独立复核补充分页移页、增量基准失配和纹理创建失败场景。Rust 增量起点改为编辑前后页面的首个差异；Qt 校验 delta 的基准 session/generation，失配时重取完整 Rust 快照；场景图在后续自然更新发现有效字形缺少纹理时重试创建，不增加轮询定时器。使用 Caret 窗口手动粘贴 100,499 字符，内容保持并可滚动，但仍观察到明显延迟，需继续用 flamegraph 定位端到端热点。

## 完成摘要

仓库实现与本地验证完成。性能 suite 已按 Rust CPU、Qt CPU、Qt GPU 分离；夜间报告包含工作负载、CPU、构建配置、字体清单摘要、Qt RHI renderer 和提交信息。Qt 页面背景场景节点现在按可视范围创建，字形更新复用未变化纹理；400 页软件渲染基准中位数改善 31.0%、P95 改善 32.3%。滚动期间仅当可视页区间变化时刷新字形，停止后按最终像素相位更新。普通单段输入复用未变段落布局，并通过小型页面/段落增量更新 Qt 快照；10 万字符混合语言文档的 Rust 输入响应中位数为 1.537 ms，响应从约 657 KB 降至约 2.1 KB。百万字符混合语言文本作为 Rust/Word 的共同 Benchmark 数据，Word 对照独立进行，不接入仓库测试。

限制：上述输入数据不包括 Qt 主线程 JSON 解析、字形帧更新及显示设备呈现的耗时；当前仍在 Rust 主线程同步计算分页。Qt GPU 基准已本地编译，但 macOS 无屏幕环境不能运行；Linux nightly 通过 Xvfb 提供虚拟 X11 context 后是否能取到 GPU timestamp，仍需首次远程运行确认。
