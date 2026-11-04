# Rust 性能范围

Rust suite 只测 CPU：文档模型、Unicode 文本、字体回退、Parley 排版、分页和编辑后的重排都由 Rust 执行。GPU 绘制由 Qt Quick / RHI 负责，对应的无头基准位于 `tests/performance/qt/gpu/`。

长文档基准读取 `tests/performance/fixtures/novel-1m-multilingual.txt`，包含 5,000 段、共 1,000,000 个 Unicode 字符，语言覆盖中文、英文、法文、阿拉伯文、俄文、希腊文和日文。该 UTF-8 纯文本也可在 Word 中单独打开作本机对照。基准覆盖整篇首次排版、单段编辑后重排、打开与删除、撤销/重做以及保存快照序列化。

`document_history_save_criterion` 使用相同语料的前 500 段（100,000 个正文 Unicode 字符）和全部 5,000 段（1,000,000 个正文 Unicode 字符），分别测撤销、重做和 `.cdoc` 保存快照序列化。会话打开、光标定位和编辑状态准备在 Criterion 计时区间外；撤销/重做计时包含 Rust 布局更新及 UI 快照生成，不包含 Qt 渲染或磁盘 I/O。该基准用于比较 history 操作规模，不设置吞吐量，因为每次只撤销或重做一项编辑。

`document_open_delete_criterion` 仅为完整文档解码和打开准备报告按实际 Unicode 字符数计算的 Criterion throughput；单字素删除独立分组，不标注整篇吞吐量。若设置 `CARET_PERF_CDOC`，基准会记录实际文件字节数和可见文本 Unicode 字符数，结果不能按默认一百万字符语料解读。

`document_view_input_criterion` 还测量在 100,499 个 Unicode 字符、500 段的多语言文档中，对中间段选区执行 `\underline` 命令并生成更新快照的耗时。文档构建和段落选择在计时区间外；命令行 begin/type/accept、Rust 格式事务、布局更新和快照生成在计时区间内。它不包含字体栅格化、Qt 场景图更新或 GPU 呈现。2026-10-30 macOS arm64 release 本机 10 样本的 Criterion 回归斜率估计为 1.5899 ms，95% 置信区间为 1.5742–1.6245 ms；这是当前实现的单次测量，没有历史对照组，因此不据此声称性能提升。

`math_edit_criterion` 在 256 与 4,096 个 ASCII 文本字节的 `\frac{\text{...}}{\beta}` 公式上测量 MathSourceMap 投影解析和中间位置插入一个字符。样本均为 Equation 固定分子内的单个 ASCII Run。会话、公式、Criterion 输入夹具 `String` 的克隆和光标定位在计时区间外；解析器自身拥有源码、构造投影及编辑事务的工作仍计入测量。基准在计时外析构解析输出和编辑后的 Session。投影解析按源码字节数报告吞吐量，单字符编辑不标注吞吐量。该基准记录当前路径成本，为后续把 Math TextFlow 事务从 TeX 映射桥接迁出提供可复现比较，不代表性能优化已完成。基准不包含源码逐键、Align、多 Run、转义/混合文字、公式布局、Qt 绘制或 IME。

2026-11-04 基线：macOS Darwin 27.2.0 arm64，`rustc 1.99.0`，Criterion release profile，10 样本、1 秒 warm-up 和 1 秒 measurement。投影解析在 256/4,096 文本字节输入上的 Criterion 区间分别为 8.39–9.17 μs 与 86.40–92.15 μs；Math TextFlow 中间位置插入一个 ASCII 字符为 5.84–6.75 μs 与 23.03–26.37 μs。CPU 型号未能从当前沙箱读取；这些是单机局部基线，没有优化前后对照，也不代表布局或端到端输入性能。原始 Criterion `new/*.json` 位于本地 `target/criterion/rust_math_projection/` 与 `target/criterion/rust_math_textflow_edit/`，后续运行会覆盖且不入版本控制；Nightly suite 上传的 Criterion JSON artifact 才是可留存比较数据。
