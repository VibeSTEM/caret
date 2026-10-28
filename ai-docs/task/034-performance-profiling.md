# 034 — Qt / Rust 性能测量与热点优化

- 状态：in-progress
- 依赖：024、026、027、030
- 优先级：P2
- 创建 / 更新：2026-10-27 / 2026-10-28

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
- [ ] 10 万字及百万字文档打开、删除字符保持可交互；长文档实测卡顿需继续定位 Qt 安装与渲染阶段。
- [ ] 独立 review 完成；验证计划、工作记录和 task index 与实现一致。

## 性能优化结果总结

- **普通输入**：选区完全落在同一段时，Rust 只重新塑形该段，保留其他段落的 Parley 结果；跨段选区仍走完整排版。每次编辑后的分页和页面坐标仍由 Rust 计算。
- **输入法预编辑**：同段预编辑只发送变更段和首个受影响页面之后的增量；增量起点同时覆盖文档基准与上一份预览，处理段落跨页移动后又移回的情况。Qt 允许文档代际不变的 composition preview 增量，并在旧快照上原子替换该段。候选提交继续使用递增文档代际的普通编辑增量，起点同时覆盖提交前实际显示的预览。跨段 composition 回退为完整快照。
- **结构编辑与粘贴**：回车拆段、软换行以及单段范围内的纯文本粘贴复用未变化段落的排版结果；输出仍为完整快照，以便 Qt 正确接收新增/删除的段落和段落顺序变化。
- **Rust → Qt 快照**：普通单段输入传递首个实际发生变化的页面起的后缀，以及当前段落；Rust 对比编辑前后的分页快照，避免孤行控制或后续重排改变更早页面时保留旧片段。Qt 使用缓存的完整快照合并增量，并校验基准会话与文档代际；失配时向 Rust 重取完整快照。结构修改、跨段输入和初始化仍使用完整快照。
- **Qt 页面场景**：页面背景节点限制在可见范围及一页预读内；滚动期间仅可视页范围变化时重新栅格化，滚动结束再对齐最终像素相位。相同字形纹理在布局更新之间复用。
- **同一台 Apple M4 的 10 万字符测试**：普通输入的 Rust 响应中位数从整篇排版的 34.854 ms 降至段落复用与增量快照后的约 1.4 ms；UI 响应从约 657 KB 降至约 2.1 KB（99.67%）。这只测 Rust 命令与响应构造，不含 Qt 主线程解析、场景图绘制和显示器呈现。
- **未保留的尝试**：复用每个 `QSGImageNode` 的实验在无头软件场景基准里没有显示稳定收益，因此撤回；避免保留没有测量依据的场景图复杂度。
- **当前限制**：普通输入仍在 UI 调用链同步执行，并在 Rust 内重新计算分页；异步分页或增量分页还未实现。需要用真实窗口和 10 万字符文档复测用户感知滚动及输入延迟。

### 2026-10-28 长文档打开与删除跟进

- 输入文件：`~/Documents/caret/test_1.cdoc`，3,673,391 B，5,001 个正文条目，约 1,000,000 个字符、5,000 个段落；字符分布与仓库多语言长篇夹具一致。
- Rust release Criterion 的 `open_read_validate_layout_and_snapshot` 包含读文件、解析/校验、创建 Session、首次整篇 Parley 排版与完整 UI 快照序列化；单独 `decode_validate_cdoc` 约 8.34 ms。release 完整准备约 358.83–371.60 ms（时间点估计 364.83 ms）。这个数字不能直接与 `cargo run` 的 dev GUI 比较。
- 同一 debug/dev 配置的临时阶段测量：解析约 87 ms、Session 构造约 8.8 ms、字体目录约 107 ms、整篇布局约 10.86 s；布局输出 5,001 段、536 页。debug Instruments Time Profiler 采样显示热点集中于 Parley builder、字体塑形和 HarfBuzz。符合用户所见约 10 s 延迟，瓶颈在未优化 debug 构建的整篇塑形，而非文件解码。
- 为开发期仍保留调试符号的同时避免这种延迟，Cargo dev profile 对 `caret-app`、`caret-engine`、`caret-layout`、Parley、HarfBuzz、字体/ICU 热路径包设 `opt-level = 2`。相同 dev 配置复测整篇布局 1.081 s，完整 `prepare_open` 1.304 s（从 11.595 s 降低约 88.8%）。首次切换 profile 会重编这些包；结果不含 Qt 快照解析、QML 更新和首帧绘制。
- 单字符退格原先为每次编辑重新塑形整篇文档，Criterion 基线 335.63 ms；同段删除改为复用其他段落布局并生成受影响页之后的增量快照，最新复测中位数 13.75 ms，减少约 321.88 ms（约快 95.9%）。跨段、段落边界和结构性删除仍使用完整布局路径。
- 删除基准针对段落中间的单字素退格，测量 Rust 编辑及增量快照构造；不代表跨段选区删除、Qt 收到增量后的全部成本或窗口呈现时间。
- 打开安装阶段原来在 GUI 线程解析完整布局 JSON，并把每段的行布局、每页的分页片段转换成未被产品界面读取的嵌套 `QVariant`。现在后台 worker 校验/解析快照；页面与段落布局作为共享 `QJsonArray` 保存，只在调用对应兼容属性时才转成 `QVariantList`。页面尺寸、段落行数和分页片段仍校验，命中测试与文档布局权威继续属于 Rust。
- 当前尚无 GUI 端到端复测证明 10 s 已消失。Qt/QML 首帧布局和可视页 glyph 栅格化仍需在真实窗口单独计时；Rust 侧火焰图显示首次塑形与分页是主要 CPU 成本。

### 10 万字交互采样

- 使用 macOS `/usr/bin/sample` 对实际 `caret` 进程采样 25 秒，1 ms 采样间隔；期间完成 100,499 字符文档粘贴并滚动画布。报告保存在本机临时目录，不纳入仓库。
- 主线程共 17,872 个样本，其中约 15,062 个在 AppKit 等待下一个事件（约 84%）；`QSGRenderThread` 的 17,273 / 17,872 个样本在 `QWaitCondition::wait`（约 97%），`renderSceneGraph` 约 240 个样本。这个窗口内未观察到持续的 GPU 或场景图线程饱和。
- 主线程的可见应用工作集中在 `PageSurface.qml` 的可视页字形栅格化调用：采样栈经过 `LayoutViewModel::rasterizePageGlyphs`（约 770 个样本）、Rust `DocumentViewRuntime::rasterize_page_glyphs`（约 707 个样本）和 `PageGlyphRasterizer::rasterize`（约 677 个样本）；其中 `colorize_alpha` 约 169 个样本。该路径同步从 QML 调用 Rust，并为字形覆盖率生成着色 RGBA 像素。
- **判断**：继续优化方向应先聚焦可视页字形栅格化的触发频率、缓存命中与像素着色成本，以及它在主线程同步执行造成的短时阻塞；现有数据不支持优先重写 Rust 整篇布局或 GPU 绘制。
- **采样限制**：当时的 `/usr/bin/sample` 报告包含大量空闲时间，是进程级抽样，不是逐帧追踪；当时没有得到可用的 SVG 火焰图。后续已通过 `xcrun xctrace` 成功导出 IME Time Profiler 表，但 `cargo flamegraph` 的折叠步骤因 Xcode 27 的 `frame` XML 格式失败。样本计数只能指出热点方向，不能换算为单帧耗时或证明用户感受到的卡顿都来自此路径；仍需在连续滚动/连续输入场景下采集 Qt 主线程事件耗时和帧时间，再评估异步栅格化或跨页面缓存策略。

### 2026-10-28 换页滚动与输入法延迟跟进

- **代码路径确认**：页面帧缓存原先把文档代际和全局布局 revision 放进每页缓存键；任何单段 IME 更新都会使当前可视范围内所有页面缓存失效。`PageSurface` 还只保留最近一次可视请求的帧，滚动换页后旧页帧被丢弃。缓存未命中时，QML 同步调用 Rust `rasterize_page_glyphs`，并在 UI 线程转换整页图片和 glyph placements。此路径与上面的主线程采样热点一致，可以解释换页时的短暂停顿，也会放大快速输入期间的显示成本。
- **本轮改动**：布局投影为每页分配 glyph revision；部分快照保留受影响页之前的 revision，只刷新受影响页及其后续页。QML 缓存键使用页级 revision，并在可视范围附近保留有界帧缓存。页面帧继续校验字体代际、缩放、设备像素相位和主题文字颜色，因此几何或显示条件改变时仍会重新栅格化。IME/Edit 响应现在复用已解析的 `QJsonDocument`，避免每次输入先解析判断类型、再把同一布局快照重新解析一次。
- **验证**：Qt 集成测试确认单段尾部编辑保留未受影响前页的 glyph revision，并更新受影响末页；`cargo test --locked -p caret-tests --test native` 的 CTest 4/4 和格式检查通过。独立 review 未发现缓存错误复用路径。
- **限制与后续**：后缀页 revision 采用保守失效；当前编辑页仍需同步栅格化整页，页面缓存未命中或设备像素相位变化时也会同步栅格化。本轮没有真实窗口帧时间数据，不能宣称 IME 字形延迟已完全消失。需在 10 万字符文档中复测换页滚动、快速 IME 预编辑/提交和连续退格；若活动页仍有明显延迟，再测量并考虑段级帧合成或后台栅格化。

## 验证计划与结果

### 已量化的优化

| 范围 | 优化前 | 优化后 | 绝对变化 | 相对变化 |
| --- | ---: | ---: | ---: | ---: |
| Rust 单字符输入与 UI 响应（100,499 字符） | 34.854 ms | 1.291 ms | −33.563 ms | 快 96.30%（27.00×） |
| Rust 输入阶段：仅复用未变段落布局 | 34.854 ms | 3.079 ms | −31.775 ms | 快 91.17% |
| Rust 输入阶段：在段落复用上增加分页/段落增量 | 3.079 ms | 1.537 ms | −1.542 ms | 快 50.08% |
| Rust → Qt 快照体积 | 654,930 B | 2,128 B | −652,802 B | 减少 99.68% |
| Rust IME 预编辑复用快照（100,499 字符，dev） | 4.623 s / 3,000 次 | 1.229 s、1.278 s / 3,000 次 | 后两轮均值减少 3.370 s | 约快 72.9%（3.69×） |
| Qt CPU 400 页静态场景中位帧时间 | 2,092.667 μs | 1,443.833 μs | −648.834 μs | 快 31.01% |
| Qt CPU 400 页静态场景 P95 帧时间 | 2,373.917 μs | 1,606.625 μs | −767.292 μs | 快 32.32% |
| Qt CPU 单页单字形变更帧中位数（1,000 placements） | 2,339.833 μs | 1,312.584 μs | −1,027.249 μs | 快 43.9% |
| Qt CPU 单页 IME 后缀位移帧中位数（45 glyphs） | 2,369.291 μs | 1,389.125 μs | −980.166 μs | 快 41.4% |
| Qt CPU 400 页单字形变更帧中位数 | 1,520.833 μs | 1,206.543 μs | −314.290 μs | 快 20.7% |
| Qt CPU 400 页 IME 后缀位移帧中位数 | 1,604.584 μs | 1,281.417 μs | −323.167 μs | 快 20.1% |
| Qt 页面背景节点数（400 页文档） | 800 | 6 | −794 个 | 减少 99.25% |

本次 Rust 普通输入复测中位数为 1.291 ms（区间 1.254–1.319 ms）；增量响应 2,128 B。表中的阶段性值来自同机同负载的多轮 Criterion 对比，并非单次运行内的连续采样；中间阶段曾测得 1.537 ms / 2,115 B，上一轮最终复测为 1.406 ms / 2,144 B。Qt 单页合成场景从 2,212.750 μs 变为 2,288.000 μs（慢 3.40%），没有观察到收益。Qt CPU 数值来自合成软件渲染场景，不能代表真实窗口的端到端帧时间。

本次 100,499 字符 Criterion 复测测得 IME 预编辑 1.322 ms、候选提交 2.300 ms、回车拆段 2.669 ms；10,010 字符粘贴 5.468 ms。它们目前没有可比的同条件优化前基线，因此只列测得值，不计算提升百分比。这些 Rust 命令时间不含 Qt JSON 合并、字形栅格化、窗口帧呈现或真实输入间隔。

针对 IME 快速输入另用临时 harness，在准备好 100,499 字符、500 段文档并开始预编辑后，连续调用 3,000 次预编辑更新。相同 Apple M4、Cargo dev 配置下，Arc 优化前单次为 4.623 s；优化后两次为 1.229 s 和 1.278 s，平均 1.254 s，较前次测量减少约 72.9%。旧布局中每次复用都深拷贝未变段落的文本、完整 Parley 结果及 UTF-16 检查点；现在这些不可变数据由 `Arc` 共享，只有段落分页位置仍随新快照重建，并在每次新布局时重新分配空位置表。这个小样本仅用于验证热点优化方向，不是 Criterion 统计，也不代表真实窗口端到端延迟。

为检查残留延迟，Criterion 新增了 IME 更新/候选提交后同步栅格化活动页的 release 路径。100,499 字符语料、活动页字形缓存预热后，预编辑更新加页面字形生成的中位数为 667.937 μs（区间 663.917–678.667 μs）；候选提交加页面字形生成的中位数为 618.542 μs（区间 612.708–624.625 μs）。基准在每次更新后读取当前 caret 的页面索引，所以排版导致跨页时仍栅格化实际活动页。该基准包括 Rust 输入、caret 几何、布局投影和 Rust 活动页字形栅格化，不包括 Qt JSON/QVariant 校验、QSG 节点更新、纹理上传和 vsync。Qt 场景图基准已补充单字形变更与同一行 45 个后缀 glyph 位移的两种更新负载，计时覆盖 `setGlyphFrames` 校验/安装、`setSelectionRects`、Qt Quick polish/sync/render，并拆分记录安装与场景图时间。安装阶段约 133–163 μs，节点更新/软件绘制约 1.1–2.2 ms；节点复用使后者中位数降低约 22–46%，安装成本基本不变。该结果不包括 Rust、JSON 解析或实际屏幕呈现，不能证明真实窗口端到端延迟已消失。

### 验证结果

| 验证项 | 工作负载 / 命令 | 结果 |
| --- | --- | --- |
| `.cdoc` 打开与解析 | `CARET_PERF_CDOC=~/Documents/caret/test_1.cdoc cargo bench --locked -p caret-app --bench document_open_delete_criterion -- --sample-size 10 --warm-up-time 0.5 --measurement-time 3`；Apple M4，release，3,673,391 B / 1,000,000 字符 | 完整读入、校验、首次排版、快照序列化时间点估计 364.83 ms（区间 358.83–371.60 ms）；只解析与校验 8.34 ms（区间 8.03–8.71 ms）。跨轮波动存在，Criterion 未检测到显著变化 |
| `.cdoc` dev 打开阶段 | 临时 `perf_open` 示例测阶段后删除；Apple M4，Cargo dev，3,673,391 B / 1,000,000 字符 | 未优化时解析 87.10 ms、Session 8.82 ms、字体目录 107.25 ms、布局 10.863 s；应用 dev 热路径 `opt-level=2` 后布局 1.081 s、完整 prepare 1.304 s（快约 90.0% / 88.8%） |
| IME 连续预编辑采样 | 临时 `perf_ime_profile` harness；Apple M4，Cargo dev；100,499 字符、500 段；先准备布局，再重复预编辑 3,000 次 | `cargo flamegraph` 成功运行并采样，但 Xcode 27 输出的 trace XML 无法被 flamegraph 折叠（`Unpaired tag: frame`）。直接用 `xcrun xctrace` 导出 Time Profiler 表并汇总：1,767 个主线程样本；`layout_with_reuse` 约 61.1%，`LaidOutParagraph::clone` 约 35.6%，Parley layout 约 15.3%。应用 Arc 共享后同 harness 为 1.229 s / 1.278 s（优化前单次 4.623 s）；临时 harness 已删除。 |
| 百万字符单字素删除 | 同一 `.cdoc`；段落 2,500 内退格并构造 Rust UI 增量快照 | 335.63 ms → 13.75 ms，约快 95.9%；结构性删除不在此次快速路径内 |
| 打开快照 Qt 安装 | 打开 `.cdoc` 时在后台解析/验证 JSON，保留共享 JSON 数组并延迟构造嵌套 QVariant | native 构建通过；仍需真实窗口端到端计时确认解析、QML 更新和首帧绘制耗时 |
| Rust 长篇排版基线 | `cargo bench --locked -p caret-engine --bench novel_document_criterion`；Apple M4，release，百万字符混合语言语料 | 整篇排版约 515 ms、单段编辑重排约 15.1 ms。过去曾因无显著收益撤回 Session 路径的重复校验跳过；本轮重新引入以避免编辑后重复遍历，未单独量化，不作为本轮性能提升结论 |
| Rust 编辑与输入基准 | `cargo bench --locked -p caret-app --bench document_view_input_criterion -- --sample-size 10 --warm-up-time 1 --measurement-time 2`；Apple M4，release，100,499 字符交互语料 | 普通单字符输入 1.291 ms；IME 预编辑 1.322 ms、候选提交 2.300 ms、回车拆段 2.669 ms、10,010 字符粘贴 5.468 ms。Criterion 提示部分样本需更长测量窗口；结果用于本机对比，不代表 UI 端到端延迟 |
| IME 与活动页字形 Rust 路径 | `cargo bench --locked -p caret-app --bench document_view_input_criterion -- visible_page_raster --sample-size 10 --warm-up-time 0.5 --measurement-time 1`；Apple M4，release，100,499 字符 | 预编辑更新加当前页栅格化中位数 667.937 μs（663.917–678.667 μs）；候选提交加当前页栅格化中位数 618.542 μs（612.708–624.625 μs）。预热活动页缓存，并在计时内获取更新后的 caret 页；只测 Rust，Qt snapshot codec、QSG 节点/纹理和呈现未包含 |
| Rust 正确性与静态检查 | `cargo test --locked -p caret-engine`；`cargo test --locked -p caret-app`；`cargo clippy --locked -p caret-app --all-targets -- -D warnings` | caret-engine 83 项、caret-app 104 项测试通过；Clippy 通过。测试覆盖普通输入、IME 增量预览、跨页回退、跨段预览/取消、回车与粘贴，以及段落布局克隆共享只读数据 |
| Qt / QML 集成 | `cargo test --locked -p caret-tests --test native`；QML 格式检查 | CTest 4/4 通过；QML 格式检查通过。覆盖 Qt 合并增量预览、跨段预览后段落删除与取消恢复 |
| Qt CPU 基线 | 1280×960、1,000 glyph placements、96 unique images、12 selection rectangles、31 samples | 稳定静态场景多次采样中位数 787–914 μs/帧，P95 860–997 μs/帧 |
| Qt CPU 页面帧更新 | 1280×960、1,000 placements、96 images、12 个可视选区；Apple M4、Qt 6.11.2、Software renderer、1,000 samples；包含单 glyph 变更和同一行 45 glyph 位移两种负载 | 计时覆盖 `setGlyphFrames`、`setSelectionRects` 和 `polish/sync/render`，并分段采样。旧/新单页单 glyph总中位数 2,339.833/1,312.584 μs（快 43.9%）；400 页为 1,520.833/1,206.543 μs（快 20.7%）。IME 后缀位移场景单页快 41.4%、400 页快 20.1%。安装阶段中位数约 133–163 μs，复用前后接近；主要改善来自场景图阶段，单页约快 44–46%，400 页约快 22%。仍不含 Rust、JSON 解析和真实屏幕呈现；P95 受本机调度离群值影响，暂不作为结论 |
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
| 2026-10-28 | 使用 Xcode Instruments Time Profiler 对 release Criterion 与 Cargo dev 打开路径采样；`cargo flamegraph` 在 Xcode 27 的 trace XML 上无法折叠 `frame` 标签，改用 `xcrun xctrace record/export` 汇总 | release Rust 准备约 0.36 s；未优化 dev 整篇布局 10.86 s，主热点为 Parley builder、字体塑形与 HarfBuzz。不能将 release 结果用于解释 dev GUI 延迟 |
| 2026-10-28 | 同段退格改为段落布局复用并返回增量快照；Cargo dev 对测得的排版热路径启用 `opt-level=2`；打开时将 Qt 快照解析移到 worker，延迟嵌套 QVariant 转换并用 ticket guard 管理候选文档生命周期 | 删除从 335.63 ms 降至 13.75 ms（约快 95.9%）；dev 整篇布局从 10.86 s 降至 1.081 s，完整 Rust prepare 为 1.304 s。native/QML 构建检查通过；需真实窗口确认 1.304 s 之外的 Qt 与首帧耗时 |
| 2026-10-28 | 安装并尝试用 `cargo flamegraph` 采样长文档 IME 更新；Xcode 27 的 flamegraph 折叠器不能读取其生成的 trace XML，改由 `xcrun xctrace` 导出 Time Profiler 数据检查调用栈 | 1,767 个主线程样本中 `layout_with_reuse` 约占 61.1%，深克隆未变段落约占 35.6%，Parley 排版约占 15.3%。结果证明该输入路径主要受快照复制影响，不是重新塑形 |
| 2026-10-28 | 将未变段落的正文、Parley 布局和 UTF-16 检查点改为 `Arc` 共享；分页行位置仍归新快照所有，复用时重新分配空位置表 | 100,499 字符、3,000 次 IME 更新从单次 4.623 s 降至两次 1.229 s / 1.278 s，平均快约 72.9%。引擎 83 项和应用 104 项测试通过；真实窗口输入延迟仍需复测 |
| 2026-10-28 | 为 100,499 字符输入基准补入活动页字形缓存预热后的 IME 预编辑/候选提交及 Rust 页面栅格化 | 更新后按当前 caret 页栅格化；预编辑路径中位数 667.937 μs、候选提交 618.542 μs。Rust 路径已很短；Qt JSON 校验、QSG 节点变更和帧呈现仍需单独采样，暂不据此重构场景图 |
| 2026-10-28 | Qt 页面 glyph frame 变化时复用 `QSGImageNode`，只更新发生变化的节点属性；增加单 glyph 和 IME 行内后缀移动软件渲染负载，并拆分帧安装和场景图计时 | 1,000 次采样中，单页总中位数改善 43.9% / 41.4%，400 页视口改善 20.7% / 20.1%；场景图阶段改善约 22–46%，`setGlyphFrames` 安装约 0.13–0.16 ms 且优化前后相近。独立 review 发现页面缩短时无效 page rect 会早退并遗留旧 glyph，现已在不可绘制帧清理节点；native 集成通过。结果不代表真实显示延迟，仍需实际窗口采样 |

## 完成摘要

已有性能 suite 与此前优化完成；本次长文档体验跟进仍在进行。性能 suite 已按 Rust CPU、Qt CPU、Qt GPU 分离；夜间报告包含工作负载、CPU、构建配置、字体清单摘要、Qt RHI renderer 和提交信息。Qt 页面背景场景节点现在按可视范围创建，字形更新复用未变化纹理；400 页软件渲染基准中位数改善 31.0%、P95 改善 32.3%。普通输入和同段 IME 预编辑复用未变段落布局，并通过页面/段落增量更新 Qt 快照；回车拆段和单段粘贴复用未变化段落的塑形结果。100,499 字符 Rust 基准最新复测测得 IME 预编辑 1.322 ms、候选提交 2.300 ms、回车拆段 2.669 ms；10,010 字符粘贴 5.468 ms。百万字符混合语言文本作为 Rust/Word 的共同 Benchmark 数据，Word 对照独立进行，不接入仓库测试。

限制：优化后的同一 dev 配置 Rust prepare 为 1.304 s，但尚未完成真实窗口端到端回归；Qt 快照解析已移入 worker，未使用的布局投影延迟转换。首屏布局、QML 更新、可视页 glyph 栅格化及显示设备呈现仍需端到端分段计时。release 与 dev 指标分别报告，不混作因果对比。Qt GPU 基准已本地编译，但 macOS 无屏幕环境不能运行；Linux nightly 通过 Xvfb 提供虚拟 X11 context 后是否能取到 GPU timestamp，仍需首次远程运行确认。
