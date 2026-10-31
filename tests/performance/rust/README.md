# Rust 性能范围

Rust suite 只测 CPU：文档模型、Unicode 文本、字体回退、Parley 排版、分页和编辑后的重排都由 Rust 执行。GPU 绘制由 Qt Quick / RHI 负责，对应的无头基准位于 `tests/performance/qt/gpu/`。

长文档基准读取 `tests/performance/fixtures/novel-1m-multilingual.txt`，包含 5,000 段、共 1,000,000 个 Unicode 字符，语言覆盖中文、英文、法文、阿拉伯文、俄文、希腊文和日文。该 UTF-8 纯文本也可在 Word 中单独打开作本机对照。基准覆盖整篇首次排版、单段编辑后重排、打开与删除、撤销/重做以及保存快照序列化。

`document_history_save_criterion` 使用相同语料的前 500 段（100,000 个正文 Unicode 字符）和全部 5,000 段（1,000,000 个正文 Unicode 字符），分别测撤销、重做和 `.cdoc` 保存快照序列化。会话打开、光标定位和编辑状态准备在 Criterion 计时区间外；撤销/重做计时包含 Rust 布局更新及 UI 快照生成，不包含 Qt 渲染或磁盘 I/O。该基准用于比较 history 操作规模，不设置吞吐量，因为每次只撤销或重做一项编辑。

`document_open_delete_criterion` 为完整文档解码和打开准备按实际 `.cdoc` 文件字节数报告 Criterion throughput；这一口径同时适用于正文、行内公式和块级公式。单字素删除独立分组，不标注整篇吞吐量。若设置 `CARET_PERF_CDOC`，基准使用该文件本身的字节数，结果不能按默认一百万字符语料解读。

`document_view_input_criterion` 还测量在 100,499 个 Unicode 字符、500 段的多语言文档中，对中间段选区执行 `\underline` 命令并生成更新快照的耗时。文档构建和段落选择在计时区间外；命令行 begin/type/accept、Rust 格式事务、布局更新和快照生成在计时区间内。它不包含字体栅格化、Qt 场景图更新或 GPU 呈现。2026-10-30 macOS arm64 release 本机 10 样本的 Criterion 回归斜率估计为 1.5899 ms，95% 置信区间为 1.5742–1.6245 ms；这是当前实现的单次测量，没有历史对照组，因此不据此声称性能提升。

## 数学树内存探针

`math_tree_memory` 是一次性布局探针，不参与默认测试或 Criterion suite。它先解析一次，再复用解析结果导入 1 个典型公式、1 个十层嵌套分数或 1,000 个典型公式；报告 Caret 节点数/深度、MathNode/NodeId 静态尺寸、导入阶段分配调用和累计字节、保留堆字节及进程峰值 RSS。导入前的 ParsedMath 不计入保留堆差值；公式外层 Vec 容量和其中内联保存的根节点计入。分配字节是 allocator 请求大小，不含 allocator 元数据。进程峰值 RSS 是进程启动以来的历史高水位，包含解析、预热及探针基线，不代表测量时刻的精确 RSS，且运行间可能波动。Release 构建与运行：

```sh
cargo build --locked --offline --release -p caret-engine --example math_tree_memory
target/release/examples/math_tree_memory typical
target/release/examples/math_tree_memory deep
target/release/examples/math_tree_memory many
```

运行环境、命令和观测值写入 task；macOS/Linux 上 RSS 来自 `getrusage(RUSAGE_SELF)`，其他平台显示 `None`。各样本需独立进程运行。
