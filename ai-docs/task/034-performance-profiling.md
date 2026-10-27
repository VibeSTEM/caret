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
- **输入法预编辑**：同段预编辑只发送变更段和首个受影响页面之后的增量；增量起点同时覆盖文档基准与上一份预览，处理段落跨页移动后又移回的情况。Qt 允许文档代际不变的 composition preview 增量，并在旧快照上原子替换该段。候选提交继续使用递增文档代际的普通编辑增量，起点同时覆盖提交前实际显示的预览。跨段 composition 回退为完整快照。
- **结构编辑与粘贴**：回车拆段、软换行以及单段范围内的纯文本粘贴复用未变化段落的排版结果；输出仍为完整快照，以便 Qt 正确接收新增/删除的段落和段落顺序变化。
- **Rust → Qt 快照**：普通单段输入传递首个实际发生变化的页面起的后缀，以及当前段落；Rust 对比编辑前后的分页快照，避免孤行控制或后续重排改变更早页面时保留旧片段。Qt 使用缓存的完整快照合并增量，并校验基准会话与文档代际；失配时向 Rust 重取完整快照。结构修改、跨段输入和初始化仍使用完整快照。
- **Qt 页面场景**：页面背景节点限制在可见范围及一页预读内；滚动期间仅可视页范围变化时重新栅格化，滚动结束再对齐最终像素相位。相同字形纹理在布局更新之间复用。
- **同一台 Apple M4 的 10 万字符测试**：普通输入的 Rust 响应中位数从整篇排版的 34.854 ms 降至段落复用与增量快照后的约 1.4 ms；UI 响应从约 657 KB 降至约 2.1 KB（99.67%）。这只测 Rust 命令与响应构造，不含 Qt 主线程解析、场景图绘制和显示器呈现。
- **未保留的尝试**：复用每个 `QSGImageNode` 的实验在无头软件场景基准里没有显示稳定收益，因此撤回；避免保留没有测量依据的场景图复杂度。
- **当前限制**：普通输入仍在 UI 调用链同步执行，并在 Rust 内重新计算分页；异步分页或增量分页还未实现。需要用真实窗口和 10 万字符文档复测用户感知滚动及输入延迟。

### 10 万字交互采样

- 使用 macOS `/usr/bin/sample` 对实际 `caret` 进程采样 25 秒，1 ms 采样间隔；期间完成 100,499 字符文档粘贴并滚动画布。报告保存在本机临时目录，不纳入仓库。
- 主线程共 17,872 个样本，其中约 15,062 个在 AppKit 等待下一个事件（约 84%）；`QSGRenderThread` 的 17,273 / 17,872 个样本在 `QWaitCondition::wait`（约 97%），`renderSceneGraph` 约 240 个样本。这个窗口内未观察到持续的 GPU 或场景图线程饱和。
- 主线程的可见应用工作集中在 `PageSurface.qml` 的可视页字形栅格化调用：采样栈经过 `LayoutViewModel::rasterizePageGlyphs`（约 770 个样本）、Rust `DocumentViewRuntime::rasterize_page_glyphs`（约 707 个样本）和 `PageGlyphRasterizer::rasterize`（约 677 个样本）；其中 `colorize_alpha` 约 169 个样本。该路径同步从 QML 调用 Rust，并为字形覆盖率生成着色 RGBA 像素。
- **判断**：继续优化方向应先聚焦可视页字形栅格化的触发频率、缓存命中与像素着色成本，以及它在主线程同步执行造成的短时阻塞；现有数据不支持优先重写 Rust 整篇布局或 GPU 绘制。
- **采样限制**：该报告是含大量空闲时间的进程级抽样，不是逐帧追踪，也没有可用的 Xcode `xctrace` 或 `cargo flamegraph` 生成 SVG 火焰图。样本计数只能指出热点方向，不能换算为单帧耗时或证明用户感受到的卡顿都来自此路径；后续应在连续滚动/连续输入场景下采集 Qt 主线程事件耗时和帧时间，再评估异步栅格化或跨页面缓存策略。

## 验证计划与结果

### 已量化的优化

| 范围 | 优化前 | 优化后 | 绝对变化 | 相对变化 |
| --- | ---: | ---: | ---: | ---: |
| Rust 单字符输入与 UI 响应（100,499 字符） | 34.854 ms | 1.291 ms | −33.563 ms | 快 96.30%（27.00×） |
| Rust 输入阶段：仅复用未变段落布局 | 34.854 ms | 3.079 ms | −31.775 ms | 快 91.17% |
| Rust 输入阶段：在段落复用上增加分页/段落增量 | 3.079 ms | 1.537 ms | −1.542 ms | 快 50.08% |
| Rust → Qt 快照体积 | 654,930 B | 2,128 B | −652,802 B | 减少 99.68% |
| Qt CPU 400 页场景中位帧时间 | 2,092.667 μs | 1,443.833 μs | −648.834 μs | 快 31.01% |
| Qt CPU 400 页场景 P95 帧时间 | 2,373.917 μs | 1,606.625 μs | −767.292 μs | 快 32.32% |
| Qt 页面背景节点数（400 页文档） | 800 | 6 | −794 个 | 减少 99.25% |

本次 Rust 普通输入复测中位数为 1.291 ms（区间 1.254–1.319 ms）；增量响应 2,128 B。表中的阶段性值来自同机同负载的多轮 Criterion 对比，并非单次运行内的连续采样；中间阶段曾测得 1.537 ms / 2,115 B，上一轮最终复测为 1.406 ms / 2,144 B。Qt 单页合成场景从 2,212.750 μs 变为 2,288.000 μs（慢 3.40%），没有观察到收益。Qt CPU 数值来自合成软件渲染场景，不能代表真实窗口的端到端帧时间。

本次 100,499 字符 Criterion 复测测得 IME 预编辑 1.322 ms、候选提交 2.300 ms、回车拆段 2.669 ms；10,010 字符粘贴 5.468 ms。它们目前没有可比的同条件优化前基线，因此只列测得值，不计算提升百分比。这些 Rust 命令时间不含 Qt JSON 合并、字形栅格化、窗口帧呈现或真实输入间隔。

### 验证结果

| 验证项 | 工作负载 / 命令 | 结果 |
| --- | --- | --- |
| Rust 长篇排版基线 | `cargo bench --locked -p caret-engine --bench novel_document_criterion`；Apple M4，release，百万字符混合语言语料 | 整篇排版约 515 ms、单段编辑重排约 15.1 ms。跳过 Session 已保证有效的重复校验未显示显著收益，已撤回该改动 |
| Rust 编辑与输入基准 | `cargo bench --locked -p caret-app --bench document_view_input_criterion -- --sample-size 10 --warm-up-time 1 --measurement-time 2`；Apple M4，release，100,499 字符交互语料 | 普通单字符输入 1.291 ms；IME 预编辑 1.322 ms、候选提交 2.300 ms、回车拆段 2.669 ms、10,010 字符粘贴 5.468 ms。Criterion 提示部分样本需更长测量窗口；结果用于本机对比，不代表 UI 端到端延迟 |
| Rust 正确性与静态检查 | `cargo test --locked -p caret-app`；`cargo clippy --locked -p caret-app --all-targets -- -D warnings` | 94 项测试通过；Clippy 通过。测试覆盖普通输入、IME 增量预览、跨页回退、跨段预览/取消、回车与粘贴 |
| Qt / QML 集成 | `cargo test --locked -p caret-tests --test native`；QML 格式检查 | CTest 4/4 通过；QML 格式检查通过。覆盖 Qt 合并增量预览、跨段预览后段落删除与取消恢复 |
| Qt CPU 基线 | 1280×960、1,000 glyph placements、96 unique images、12 selection rectangles、31 samples | 稳定静态场景多次采样中位数 787–914 μs/帧，P95 860–997 μs/帧 |
| Qt CPU 400 页对比 | 1280×960、1,000 placements、96 images、12 个可视选区；每帧移动一个 glyph；Apple M4、Qt 6.11.2、Software renderer、31 samples | 页面节点从 800 降至 6；中位数和 P95 分别改善 31.01% 与 32.32%。单页场景慢 3.40%，未显示收益 |
| 真实窗口粘贴与滚动 | Caret 实际窗口粘贴 100,499 字符、500 段的混合中英法阿拉伯俄语希腊日文文本 | 粘贴调用约 2.06 s，画布显示多语言段落且可滚动到后续页面；滚动仍略卡。该手工结果不代替帧时间基准 |
| macOS 主线程采样 | `/usr/bin/sample` 采样实际 `caret` 进程 25 秒，1 ms 间隔；期间粘贴 100,499 字符并滚动 | 约 84% 主线程样本在 AppKit 等待；可见应用工作最集中于同步可视页 glyph 栅格化。没有可用的 Xcode `xctrace` / `cargo flamegraph` SVG 火焰图；采样细节与限制见“10 万字交互采样” |
| Qt GPU / nightly | Qt Quick / OpenGL RHI 离屏基准；PR / push 与 nightly workflow 检查 | GPU 基准可编译，但当前 macOS 无屏幕环境不能运行；Linux nightly 使用 Xvfb 和 `QQuickRenderControl`，仍待远程验证。selector 检查通过；PR / push 不运行性能基准 |

## 风险与回退

软件渲染、驱动差异、字形缓存热身及调度器负载会让绝对数字不可直接比较。报告记录这些元数据，不在异构环境设置阈值。若 Qt RHI 暴露的 GPU 计时不可用，保留离屏渲染 smoke 和明确的缺项说明；不回退为误导性 wall-clock 指标。所有优化保持正确性测试先行，可以逐项 revert。

## 决策与工作记录

| 日期 | 决策 / 实施 | 验证结果与后续 |
| --- | --- | --- |
| 2026-10-27 | 建立 Rust CPU、Qt CPU 与 Qt GPU 独立性能套件；Rust 侧不设 GPU 空基准，性能任务放在 nightly | 基准与 PR / push CI 分离；GPU 离屏运行路径已实现，Linux nightly 首次远程验证待完成 |
| 2026-10-27 | 将 Qt 页面背景节点限制在可视范围和一页预读；缓存未变化的页面字形纹理 | 400 页合成场景中位帧时间改善 31.01%、P95 改善 32.32%；单页场景没有收益，详见量化表 |
| 2026-10-27 | 为普通单段输入复用未变化段落的 Parley 结果，再将 UI 响应改为只传递变更段和首个受影响页后的分页 | 100,499 字符输入从 34.854 ms 降至最终复测 1.406 ms；快照从约 657 KB 降至约 2.1 KB。跨段编辑和结构操作保留完整快照，Qt 校验代际并在增量失配时回退完整快照 |
| 2026-10-27 | 根据实际输入反馈增加 IME 预编辑、候选提交、回车和粘贴基准；同段 IME 预览改为增量，回车/软换行/单段粘贴复用未变化段落；滚动结束后才预读下一页 glyph | 最新 Rust 复测：预编辑 1.322 ms、提交 2.300 ms、回车 2.669 ms、10,010 字符粘贴 5.468 ms。跨段 composition 使用完整快照；分页 delta 同时考虑基础布局与上一份预览，覆盖跨页移回场景 |
| 2026-10-27 | 使用 Caret 实际窗口和 macOS `sample` 检查长文档交互热点 | 100,499 字符粘贴约 2.06 s；主线程最明显的应用热点是同步可视页字形栅格化。快速输入法闪动、Qt 粘贴/回车延迟尚无端到端帧时间数据，不宣称已解决 |

## 完成摘要

仓库实现与本地验证完成。性能 suite 已按 Rust CPU、Qt CPU、Qt GPU 分离；夜间报告包含工作负载、CPU、构建配置、字体清单摘要、Qt RHI renderer 和提交信息。Qt 页面背景场景节点现在按可视范围创建，字形更新复用未变化纹理；400 页软件渲染基准中位数改善 31.0%、P95 改善 32.3%。滚动期间仅当可视页区间变化时刷新字形；下一页 glyph 预读只在滚动结束后发生。普通输入和同段 IME 预编辑复用未变段落布局，并通过页面/段落增量更新 Qt 快照；回车拆段和单段粘贴复用未变化段落的塑形结果。100,499 字符 Rust 基准最新复测测得 IME 预编辑 1.322 ms、候选提交 2.300 ms、回车拆段 2.669 ms；10,010 字符粘贴 5.468 ms。百万字符混合语言文本作为 Rust/Word 的共同 Benchmark 数据，Word 对照独立进行，不接入仓库测试。

限制：上述输入数据不包括 Qt 主线程 JSON 解析、字形帧更新及显示设备呈现的耗时；当前仍在 Rust 主线程同步计算分页，IME 预览变化也会重新栅格化当前可视页的字形。快速输入时的字符闪动、真实 Qt 回车/粘贴延迟尚未通过端到端帧时间测量关闭。Qt GPU 基准已本地编译，但 macOS 无屏幕环境不能运行；Linux nightly 通过 Xvfb 提供虚拟 X11 context 后是否能取到 GPU timestamp，仍需首次远程运行确认。
