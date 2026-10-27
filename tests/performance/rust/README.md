# Rust 性能范围

Rust suite 只测 CPU：文档模型、Unicode 文本、字体回退、Parley 排版、分页和编辑后的重排都由 Rust 执行。GPU 绘制由 Qt Quick / RHI 负责，对应的无头基准位于 `tests/performance/qt/gpu/`。

长文档基准读取 `tests/performance/fixtures/novel-1m-multilingual.txt`，包含 5,000 段、共 1,000,000 个 Unicode 字符，语言覆盖中文、英文、法文、阿拉伯文、俄文、希腊文和日文。该 UTF-8 纯文本也可在 Word 中单独打开作本机对照。基准测量整篇首次排版，以及单段编辑后复用未修改段落的重排。
