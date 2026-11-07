# Rust 性能范围

Rust suite 只测 CPU：文档模型、Unicode 文本、字体回退、Parley 排版、分页和编辑后的重排都由 Rust 执行。GPU 绘制由 Qt Quick / RHI 负责，对应的无头基准位于 `tests/performance/qt/gpu/`。

长文档基准读取 `tests/performance/fixtures/novel-1m-multilingual.txt`，包含 5,000 段、共 1,000,000 个 Unicode 字符，语言覆盖中文、英文、法文、阿拉伯文、俄文、希腊文和日文。该 UTF-8 纯文本也可在 Word 中单独打开作本机对照。基准覆盖整篇首次排版、单段编辑后重排、打开与删除、撤销/重做以及保存快照序列化。

`document_history_save_criterion` 使用相同语料的前 500 段（100,000 个正文 Unicode 字符）和全部 5,000 段（1,000,000 个正文 Unicode 字符），分别测撤销、重做和 `.cdoc` 保存快照序列化。会话打开、光标定位和编辑状态准备在 Criterion 计时区间外；撤销/重做计时包含 Rust 布局更新及 UI 快照生成，不包含 Qt 渲染或磁盘 I/O。该基准用于比较 history 操作规模，不设置吞吐量，因为每次只撤销或重做一项编辑。

`document_open_delete_criterion` 仅为完整文档解码和打开准备报告按实际 Unicode 字符数计算的 Criterion throughput；单字素删除独立分组，不标注整篇吞吐量。若设置 `CARET_PERF_CDOC`，基准会记录实际文件字节数和可见文本 Unicode 字符数，结果不能按默认一百万字符语料解读。

`document_view_input_criterion` 还测量在 100,499 个 Unicode 字符、500 段的多语言文档中，对中间段选区执行 `\underline` 命令并生成更新快照的耗时。文档构建和段落选择在计时区间外；命令行 begin/type/accept、Rust 格式事务、布局更新和快照生成在计时区间内。它不包含字体栅格化、Qt 场景图更新或 GPU 呈现。2026-10-30 macOS arm64 release 本机 10 样本的 Criterion 回归斜率估计为 1.5899 ms，95% 置信区间为 1.5742–1.6245 ms；这是当前实现的单次测量，没有历史对照组，因此不据此声称性能提升。

`math_edit_criterion` 在 256 与 4,096 个 ASCII 文本字节的 `\frac{\text{...}}{\beta}` 公式上测量 MathSourceMap 投影解析、中间位置插入一个字符、Equation 首次布局和编辑后的完整数学文档重排。样本均为 Equation 固定分子内的单个 ASCII Run。投影和 TextFlow 单字符编辑使用 `iter_batched_ref`，输入 `String`/Session 构建、克隆和光标定位在计时外，解析/编辑结果作为计时闭包返回。Equation 布局与编辑后重排使用按值 `iter_batched`：Session、DocumentLayoutService、布局前快照等输入准备在计时外，但这些被消费输入的析构在计时闭包内；首次布局每样本新建 service，重排样本在计时外先做一次基线布局。系统字体目录只在基准组开始时扫描一次并在样本间复用。投影解析按源码字节数报告吞吐量，单字符编辑和布局不标注吞吐量。该基准记录当前路径成本，为后续性能比较提供可复现基线，不代表性能优化已完成。基准不包含源码逐键、Align、多 Run、转义/混合文字、Qt 绘制或 IME。

2026-11-04 基线：macOS Darwin 27.2.0 arm64，`rustc 1.99.0`，Criterion release profile，10 样本、1 秒 warm-up 和 1 秒 measurement。投影解析在 256/4,096 文本字节输入上的 Criterion 区间分别为 8.39–9.17 μs 与 86.40–92.15 μs；Math TextFlow 中间位置插入一个 ASCII 字符为 5.84–6.75 μs 与 23.03–26.37 μs。CPU 型号未能从当前沙箱读取；这些是单机局部基线，没有优化前后对照，也不代表布局或端到端输入性能。原始 Criterion `new/*.json` 位于本地 `target/criterion/rust_math_projection/` 与 `target/criterion/rust_math_textflow_edit/`，后续运行会覆盖且不入版本控制；Nightly suite 上传的 Criterion JSON artifact 才是可留存比较数据。


`math_edit_criterion` 另含 `rust/math_mixed_document_100k` 长文档组：从百万字多语言语料取前 500 段，每段 200 Unicode 字符（100,000 正文字符），段落语言按中文、英文、法文、阿拉伯文、俄文、希腊文和日文轮换；每十段之一嵌入 InlineMath，并插入 Equation 和两行 Align 块。输入构建、解析、Session 创建和拖选端点准备均在计时外；字体目录在组开始前加载并在样本间复用。该组的 `DocumentLayoutService` 也在样本间复用，因此 `initial_layout_and_snapshot_publish` 指新 Session 在已预热服务上的首次布局/快照发布，不代表冷启动。数学编辑组每次在 setup 中向同一个长文档 Session 插入并进入 InlineMath TextFlow、发布同 Session 的基线快照；计时区间只含一个字符插入及布局。缓存路径使用 `layout_session_reusing` 并指定承载 InlineMath 的段落 ID，重排公式所在段落并复用其余段落；强制完整布局对照在计时外为同一 Session 发布基线，计时内再调用 `layout_session` 完整布局。`math_text_drag_preview_hit_and_geometry` 测已发布快照上预选端点的数学拖选 hit 重校验与选区几何；`math_text_drag_selection_commit` 测数学命中提交到 Session 选区状态。样本用 `iter_batched_ref` 保留输入 Session/快照，避免输入对象析构落入计时；布局快照作为计时结果返回，结束样本后释放。该边界是 Rust Engine 最近的公开数学快照/交互 API，不含 `DocumentViewRuntime` JSON 序列化、Qt 信号/场景图、字形栅格化、GPU 呈现或鼠标事件派发；拖选端点已确定，不测逐像素鼠标轨迹。

2026-11-07 长文档数学基线：macOS Darwin 27.2.0 arm64，`rustc 1.99.0`，Criterion release profile；命令 `cargo bench --locked -p caret-engine --bench math_edit_criterion -- rust/math_mixed_document_100k --sample-size 10 --warm-up-time 1 --measurement-time 1`。CPU 型号无法从沙箱读取。Criterion 区间：首次布局/快照发布 55.368–56.717 ms；编辑数学 TextFlow、重排公式段并复用其余段落 1.4804–1.6043 ms；编辑同一数学 TextFlow 并强制完整布局 53.977–56.400 ms；数学拖选 hit 加几何预览 1.7288–1.7641 μs；数学拖选 Session 提交 230.84–282.55 μs（1 个 low severe、1 个 high severe outlier）。每组采 10 个样本；强制完整布局未能在 1 秒目标内完成采样，Criterion 估计并延长到约 1.20 秒。前序试跑已排除，不作比较。各组为当前单机观测，不构成跨组配对显著性检验或性能收益证明。该基准不包括生产 Qt 窗口、Qt 快照 JSON、字形栅格化或 GPU，也不代表三平台性能。

2026-11-07 同命令复测：区间为首次布局/快照发布 57.592–57.884 ms；数学 TextFlow 编辑、重排公式段并复用其余段落 1.3589–1.4045 ms；强制完整布局 58.072–58.481 ms；拖选 hit 与预览 1.6971–1.7008 μs；Session 提交 194.20–205.79 μs。Criterion 对照前一条本机记录显示首排版约 +3.01%、缓存复用重排约 -10.47%、完整布局约 +6.27%、拖选预览约 -1.98%、提交约 -20.13%（各比较 p < 0.05）。该对照不是隔离变量的因果测量；不据此将变化归因于单项改动，也不声称整体性能优化完成。
