# 034 — Qt / Rust 性能测量与热点优化

- 状态：in-progress
- 依赖：024、026、027、030
- 优先级：P2
- 创建 / 更新：2026-10-27 / 2026-10-28

## 目标与背景

为文本编辑主链建立可重复的 Rust / Qt CPU 与 Qt GPU 性能基线，依据采样结果优化确认过的热点，并记录输入规模、机器、构建模式、测量范围和限制。性能 suite 仅由 nightly 执行，不进入 PR / push CI，也不设毫秒门禁。

共同长篇语料为[多语言文本夹具](../../tests/performance/fixtures/novel-1m-multilingual.txt)：1,000,000 个 Unicode 字符、5,000 段，每段 200 字符，包含中文、英语、法语、阿拉伯语、俄语、希腊语和日语。它用于 Caret 与本机 Word 的独立 Benchmark 对照；Word 自动化不进仓库或 CI。Rust 当前无 GPU 工作负载，GPU 测量只覆盖 Qt Quick / RHI。

## 必读

- [测试规范](../standards/testing.md)、[持续集成](../standards/ci.md)、[验证与评审](../standards/validation-and-review.md)
- [文字塑形](../architecture/editor/text-shaping.md)、[页面画布](027-canvas-and-hit-testing.md)、[运行时](../architecture/core/runtime.md)

## 范围与非目标

- **Rust CPU：** Parley 排版、命中测试、光标几何、字形缓存，以及百万字符首次布局与单段编辑。
- **Qt CPU：** Caret 画布页面、字形和选区的场景图更新；覆盖单页和 400 页文档视口。
- **Qt GPU：** 使用 `QQuickRenderControl` 无头离屏绘制，记录 RHI 后端、renderer、GPU 时间与吞吐；设备不支持 GPU 时间戳时如实报告缺项。
- **CI：** 各性能 suite 独立运行并上传带环境与 revision 信息的结果，仅作趋势和热点诊断，不作为跨机器阈值门禁。

不包含 PR / push 性能任务、Rust GPU 空基准、把软件渲染结果解释成物理 GPU 性能，或与测得热点无关的大规模重构。

## 前置条件与待决策

- Linux nightly 使用 Mesa / EGL 和虚拟显示运行 Qt GPU 离屏 suite；仍需确认远端 renderer 与 GPU timestamp 能力，并区分软件设备与物理 GPU。
- macOS 本机没有可用于当前无头会话的屏幕环境，Qt GPU 基准可编译但不能在此运行。
- runner、优化级别、Qt/RHI backend、renderer、字体信息和输入规模必须随结果记录。高噪声结果只作诊断，必要时在稳定硬件复测。

## 实施步骤

1. 建立 Rust CPU、Qt CPU、Qt GPU 的独立基准入口，确认它们不进入默认测试与 PR / push CI。
2. 使用固定语料和可复现的合成画布负载采样；对真实交互中的热点补充 Instruments / 系统采样。
3. 只优化有数据支持的路径，复跑同一负载并验证正确性、线程和资源生命周期。
4. 同步本任务的优化数据、验证结果、限制与剩余工作；完成时按验证规范进行独立 final review。

## 预计改动

性能基准位于 `tests/performance/rust/cpu/` 与 `tests/performance/qt/{cpu,gpu}/`；相关 Cargo / CMake 注册、nightly workflow、测试规范和本任务文档。基准不注册进默认 CTest。

## 验收标准

- [x] nightly workflow 已配置 Rust CPU、Qt CPU、Qt GPU 独立 jobs 与结果 artifact；Rust GPU 不创建空 suite。
- [x] 结果包含可解释环境和 workload 信息；不以 CPU 时间冒充 GPU 时间。
- [x] 基准不进入默认 workspace 测试、默认 CTest 或 PR / push CI。
- [x] Rust 长篇打开、编辑和 IME 路径已测量，并优化有证据的布局复用热点。
- [x] Qt 长文档滚动与字形更新路径已测量；页面背景节点限于可视范围附近，页面 glyph 更新复用场景图节点。
- [ ] 10 万与百万字符文档的真实窗口打开、输入、删除和换页仍需完成端到端复测；用户感知延迟尚不能以合成基准判定已消失。
- [ ] Qt GPU nightly 需完成远端运行核验；随后更新性能报告并完成整项 final review。

## 验证计划与结果

### 性能结果

Rust 数字为 Apple M4 本机值；Qt 页面帧对比为 Apple M4、Qt 6.11.2、release 软件 renderer。数值不能横向代表其他机器或真实显示器时延。

| 路径 / workload | 优化前 | 优化后 | 变化 | 备注 |
| --- | ---: | ---: | ---: | --- |
| Rust 单字符输入及 UI 增量（100,499 字符） | 34.854 ms | 1.291 ms | 快 96.30% | Apple M4、release；命令与测量范围见下文 |
| Rust IME 预编辑，3,000 次（dev 临时 harness） | 4.623 s | 1.229 s、1.278 s；均值 1.254 s | 约快 72.9% | 未变段落正文、Parley 布局与 UTF-16 检查点改为 `Arc` 共享；非 Criterion |
| Rust 单字素退格（1,000,000 字符文档） | 335.63 ms | 13.75 ms | 快约 95.9% | 段落中间退格及 Rust 增量快照，不代表跨段删除 |
| Rust IME 更新 + 活动页栅格化 | — | 667.937 μs | — | Apple M4、release；100,499 字符；预热活动页缓存 |
| Rust 候选提交 + 活动页栅格化 | — | 618.542 μs | — | 同上；每次按当前 caret 页栅格化 |
| Qt 单页单 glyph 帧更新 | 2,339.833 μs | 1,312.584 μs | 快 43.9% | 1,000 placements，1,000 次采样 |
| Qt 单页 IME 后缀移动帧更新 | 2,369.291 μs | 1,389.125 μs | 快 41.4% | 同一行 45 个 glyph 位移 12 pt |
| Qt 400 页单 glyph 帧更新 | 1,520.833 μs | 1,206.543 μs | 快 20.7% | 仅视口附近页面创建节点 |
| Qt 400 页 IME 后缀移动帧更新 | 1,604.584 μs | 1,281.417 μs | 快 20.1% | 同上 |

普通输入数字来自 `cargo bench --locked -p caret-app --bench document_view_input_criterion -- --sample-size 10 --warm-up-time 1 --measurement-time 2`，使用 100,499 字符交互语料，测 Rust 命令与增量响应构造。活动页字形数据来自 `cargo bench --locked -p caret-app --bench document_view_input_criterion -- visible_page_raster --sample-size 10 --warm-up-time 0.5 --measurement-time 1`；同一语料先预热活动页缓存，计时内按更新后的 caret 页面栅格化，仅覆盖 Rust。IME 3,000 次对比由临时 dev harness 完成：先准备 100,499 字符、500 段文档再重复预编辑，优化前一轮、优化后两轮；不是 Criterion 统计，临时 harness 已删除。

Qt 帧基准计时覆盖 `setGlyphFrames` / `setSelectionRects` 安装及 Qt Quick `polish/sync/render`，并分别采样安装与场景图阶段。`setGlyphFrames` 安装中位数约 133–163 μs，复用前后相近；主要收益来自 QSG glyph 节点更新与软件绘制阶段，单页约快 44–46%，400 页约快 22%。它使用合成 RGBA glyph、无头 Software renderer，不含 Rust、JSON 解码、真实 RHI/GPU 呈现和 vsync。P95 受本机调度离群值影响，不作为本轮结论。

已有的 400 页静态场景优化把背景节点从 800 减至 6，中位数改善 31.01%，P95 改善 32.32%；单页静态场景未显示收益。Rust 增量快照体积从 654,930 B 降至约 2,128 B（减少 99.68%）。

### 验证记录

| 日期 | 环境 / 命令或操作 | 实际结果与限制 |
| --- | --- | --- |
| 2026-10-27 | Rust / Qt 性能 suite、nightly selectors 与 PR / push CI 范围检查 | nightly workflow 配有 Rust CPU、Qt CPU、Qt GPU 独立 jobs 和 artifact；性能任务不进入 PR / push CI 和默认测试。Rust 无 GPU suite；Qt GPU 远端结果仍待验证。 |
| 2026-10-27 | `cargo bench --locked -p caret-engine --bench novel_document_criterion`；Apple M4、release、百万字符混合语料 | 整篇排版约 515 ms，单段编辑重排约 15.1 ms。 |
| 2026-10-28 | `CARET_PERF_CDOC=~/Documents/caret/test_1.cdoc cargo bench --locked -p caret-app --bench document_open_delete_criterion -- --sample-size 10 --warm-up-time 0.5 --measurement-time 3`；3,673,391 B / 1,000,000 字符 | release 完整读入、校验、Session、首次排版和快照序列化估计 364.83 ms（358.83–371.60 ms）；仅解析校验约 8.34 ms。不能用 release 数据解释 dev GUI。 |
| 2026-10-28 | 临时 dev 阶段计时与 Instruments；同一 `.cdoc` | 优化前解析约 87 ms、Session 8.8 ms、字体目录 107 ms、布局 10.86 s；对测得热路径启用 `opt-level=2` 后布局 1.081 s、完整 Rust `prepare_open` 1.304 s。均不含 Qt/QML 首帧。临时 harness 已删除。 |
| 2026-10-28 | Instruments Time Profiler；100,499 字符、500 段，dev IME 连续更新 3,000 次 | 优化前 1,767 个主线程样本中 `layout_with_reuse` 约 61.1%、`LaidOutParagraph::clone` 约 35.6%、Parley layout 约 15.3%。Xcode 27 的 `cargo flamegraph` 折叠步骤不能解析 trace XML 的 `frame` 标签，改用 `xcrun xctrace` 导出。 |
| 2026-10-28 | 1,000,000 字符 `.cdoc` 单字素退格 | Rust 编辑与增量快照从 335.63 ms 降至 13.75 ms；跨段、段落边界与结构性删除不属于此快速路径。 |
| 2026-10-27 | 真实窗口粘贴 100,499 字符混合语言文本并滚动画布；使用 `/usr/bin/sample` 观察进程 | 粘贴调用约 2.06 s，可滚动到后续页面但仍略卡。25 秒采样中主线程约 84% 在 AppKit 等待；该采样窗口内没有持续的 GPU 或场景图线程饱和。此操作不是 `.cdoc` 打开计时。 |
| 2026-10-28 | Qt release 离屏基准；Apple M4、Qt 6.11.2、Software renderer、1,000 samples | 更新前后单页和 400 页视口数据见上表。计时分解显示 `setGlyphFrames` 安装约 0.13–0.16 ms，QSG/软件绘制阶段约 1.1–2.2 ms；独立 review 发现无效页面早退会遗留旧 glyph，修复后 native 集成通过。 |
| 2026-10-28 | `cargo test --locked -p caret-tests --test native` | 通过；CTest 4/4。 |
| 2026-10-28 | `cargo format --check`、Qt CPU benchmark 构建及无头冒烟（10 samples） | 格式检查、构建和 JSON 输出验证通过；完整前后对比使用 1,000 samples。 |
| 待执行 | Qt GPU nightly、真实窗口端到端采样 | macOS 当前会话无可用屏幕；需远端运行 GPU suite，并在真实窗口测量打开、IME、退格与换页。 |

## 风险与回退

- 合成软件渲染基准只用于定位与同机对比，不能证明真实窗口延迟已消失。GPU runner 可能实际使用 llvmpipe，报告必须区分 renderer 与物理 GPU。
- 节点复用必须维持页面 glyph frame 的绘制顺序、纹理所有权及场景图线程约束。若正确性验证失败，可独立回退 QSG 节点复用，不影响 Rust 布局共享与页面背景范围优化。
- dev 与 release 的绝对耗时不可混比；后续每项性能结论应继续附 workload、配置与测量范围。

## 决策与工作记录

| 日期 | 决策 / 实施 | 结果与后续 |
| --- | --- | --- |
| 2026-10-27 | Rust 只测 CPU；Qt 分 CPU 与 GPU；性能基准独立于 PR / push CI | 建立 nightly suite 和固定多语言语料；GPU runner 结果仍需远端确认。 |
| 2026-10-27 | Qt 页面背景节点限制在可视页及一页预读范围，复用未变页面字形纹理 | 400 页静态场景背景节点减少 99.25%；中位数和 P95 改善约 31% / 32%。 |
| 2026-10-27 | 普通编辑和同段 IME 预览复用未变段落排版，并发送页面 / 段落增量 | 跨段编辑和结构操作保留完整快照；Qt 按文档代际校验增量，失配时请求完整快照。 |
| 2026-10-28 | 根据火焰图结果共享不可变段落布局数据；优化 dev 热路径，并将打开时 Qt 快照解析放入 worker | IME 重复预览快约 72.9%；dev Rust 整篇布局从约 10.86 s 降到 1.081 s。真实窗口端到端时间仍待验证。 |
| 2026-10-28 | 增加 Rust 活动页栅格基准与 Qt QSG 帧更新负载；复用页面 glyph 节点 | Rust IME 更新及当前页栅格约 0.62–0.67 ms；Qt 合成负载 QSG 阶段改善约 22–46%。页面尺寸无效时清除旧节点。 |

## 完成摘要

Rust 编辑、打开与 Qt 页面帧路径已有多轮优化和本机基准。当前最新 Qt 结果证明合成 Software renderer 下的 QSG 节点复用降低页面更新成本；它还不能解释全部用户可感知延迟。任务保持 `in-progress`，直到 Qt GPU nightly 结果、10 万 / 百万字符真实窗口交互复测，以及独立 final review 记录完成。
