# 008 — M0：基础文本编辑纵向切片

- 状态：implemented
- 依赖：021–024、026、027、030
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-28

## 目标与背景

本任务完成最小编辑纵向切片：用户能打开空文档、输入和删除文本、移动插入点、编辑段落、保存并重开同一份 `.cdoc`。它是[需求](../architecture/requirements.md)的 M0 集成验收。模型、存储、输入、几何、塑形、基础段落排版和画布能力分别由 020–027 提供；Qt 的 IME、导航与选区交互由 030 补齐。本任务负责端到端集成，不把格式命令或 EnvironmentNode 操作塞进基础编辑集成。

## 必读

- [文档模型](../architecture/editor/document-model.md)、[运行时](../architecture/core/runtime.md)、[命令](../architecture/editor/commands.md)
- [分层](../standards/layering.md)、[FFI](../standards/ffi.md)

## 范围与非目标

组合 021–024、026、027、030 的最小能力并接通 FFI 与 ViewModel。`caret-app` 解析路径后交给引擎；画布消费页面快照。打开和保存的原子文件语义遵循[文档模型](../architecture/editor/document-model.md)。沿用当前 QML 外壳，只做承载编辑所必需的连接，不扩展视觉控件。行距、段间距与分页如需对齐 Word/OOXML，参考[Office Open XML 调查](../research/008-office-open-xml-spacing-pagination.md)；底层排版语义仍由 026 维护。

不包含：格式工具栏、环境工具按钮、EnvironmentNode 创建/嵌套、`\` 命令、结构导航、公式、表格、增量排版和命令行进程。布局缓存可以先整篇失效。

## 前置条件与待决策

020 已完成，021–024、026、027 的基础实现已完成。沿用当前 QML 外壳并冻结其视觉设计；只接入编辑画布所需的最小宿主和绑定，不增加新控件。

## 实施步骤

1. 接通 Rust Session、应用服务、FFI、ViewModel 和画布快照。
2. 窗口完成新建、输入、删除、光标移动、选区、段落编辑、保存和重开；采用基础分页显示，不要求环境、格式或命令 UI。
3. 验证路径和原子保存；错误时内存文档仍有效。
4. 独立评审后复验；状态/撤销与脏文档关闭确认交由 018。

## 预计改动

集成 `caret-doc`、`caret-layout`、`caret-engine`、`caret-app`、`caret-ffi`、画布和打开保存的 ViewModel；QML 只接编辑宿主所需的最小绑定。

## 验收标准

- [x] 空文档输入、删除、段落拆分/合并、保存并重开后，正文和页数一致。
- [x] 保存失败不丢内存文档；保存快照、串行顺序和完成回报遵循[运行时保存契约](../architecture/core/runtime.md#保存)，不替换当前会话；更高格式版本被拒绝；另存为路径关联会话，后续保存写回该路径。
- [x] 标题状态只由 `Edited`、`Saving`、`Saved` 三态构成：有保存请求进行时显示 `Saving`；没有请求且当前修订未保存时显示 `Edited`；当前修订已保存或文档刚打开时显示 `Saved`。保存结束后按当前修订重新计算状态，失败或保存期间产生的新编辑都不能误报 `Saved`。
- [x] hover 和拖选预览不往返 Rust；鼠标释放时将最终插入点或选区提交到 Rust。键入会提交命令。
- [x] Qt 输入 DTO 与 `caret-engine::Command` 映射 UTF-16 code unit 和 Rust UTF-8 byte offset；拒绝代理对中间位置，不能用 UTF-16 索引冒充领域位置。
- [x] IME preedit 更新仅显示暂存态；提交只应用一次、取消不改文档。重复平台通知幂等，composition 未提交时 Return 由输入法消费，不触发段落拆分。
- [ ] 同一 Unicode/IME 行为用例在 Windows、macOS、Linux 执行，结果一致；平台差异只留在 Qt 适配层。
- [x] 分层符合 L2 到 L6。fmt、clippy 和本任务测试通过。评审发现已处理。

标题状态由 ViewModel 投影，不复制 Rust 的保存点：Rust 会话提供当前修订是否已保存，Qt 文件命令提供未完成的写入数。

| 当前条件 | 标题状态 |
| --- | --- |
| 存在一个或多个保存请求 | `Saving` |
| 没有保存请求，当前修订尚未保存 | `Edited` |
| 没有保存请求，当前修订等于成功保存点 | `Saved` |

保存完成或失败后重新读取 Rust 的修订状态；保存期间继续编辑时，结束后回到 `Edited`。打开文档时，新会话的修订与保存点一致，显示 `Saved`。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-27 | `cargo test --workspace --locked` | Rust 与 native/QML 测试通过 | 通过；workspace 全部测试通过，native CTest 4/4 通过 |
| 2026-10-27 | `cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` | 无 lint、格式和空白错误 | 通过 |
| 2026-10-27 | `cargo lint` | 9 个 Rust/CMake/native/QML 检查阶段通过 | 通过；包括依赖、分层、qmllint、clang-tidy、include-cleaner 和 cppcheck |
| 2026-10-27 | macOS 手工：输入、首次另存、原路径保存、重开 | 标题状态正确且重开正文一致 | 通过；观察到 `Edited`、`Saved`，重开后正文包含已保存的修订；测试文件已清理 |
| 2026-10-28 | `cargo test --locked -p caret-tests --test native` | 文件流程桌面集成测试与 QML/native 聚合通过 | 通过；新增用例覆盖输入、删除、段落拆分/合并、另存、失败保留内容、三态通知、切换文档后重开正文与页数 |
| 2026-10-28 | `cargo format --check`、`cargo lint clang-tidy --changed --check`、`cargo lint includes --changed --check`、`cargo lint cppcheck --changed --check`、`git diff --check` | 格式、受影响 C++ 静态检查和差异检查通过 | 通过 |
| 2026-10-28 | 独立 Sol medium 评审完整变更 | 文件测试构建条件、会话替换断言、异步资源清理可靠 | 发现的 2 项均修复；复核后无阻塞问题 |
| 待 nightly | Windows/macOS/Linux 输入法与 Unicode 矩阵 | 同一用例得到一致文档结果 | 本地仅验证 macOS；Windows/Linux 待 nightly 平台任务执行 |

## 风险与回退

整篇重排会让长文档变慢。M0 接受这一点，不在本任务加缓存键。回退是窗口回到 007 的占位画布，并撤下三个新 crate 的成员登记。

## 决策与工作记录

- 2026-10-25：创建 M0 文档编辑任务。003 留下的「app 尚不依赖 engine」在本任务补上。
- 2026-10-25：保存路径关联当前会话；状态机、脏文档关闭确认和撤销/重做由 018 负责。
- 2026-10-25：根据 editor-design.md 将实现范围拆到 020–027；本任务收窄为端到端集成验收。
- 2026-10-27：打开候选在 worker 中完成解析、Session 构建和首次排版，安装前校验会话/文档代际；同一路径保存按队列串行，Save As 排队期间的后续保存沿用待保存目标。
- 2026-10-27：标题保存状态统一派生为 Edited、Saving、Saved；打开按钮沿用主题单色图标风格，QML Shortcut 使用 `sequences` 绑定标准快捷键别名。
- 2026-10-28：增加桌面文件流程集成测试；先切换到不同文档再重开保存目标，避免“打开未替换会话”时测试假通过。没有文档 CLI；按约定由 014 提供 CLI 进程测试入口。
- 2026-10-28：独立评审确认测试在 `CARET_ENABLE_APP=OFF` 时不编译 FFI 逻辑，临时目录析构顺序安全；未发现其他阻塞问题。保存期间继续编辑的保存点语义由 Rust 确定性交错用例覆盖，本次未增加依赖耗时的定时竞态测试。

## 完成摘要

桌面编辑与保存重开主链已有端到端覆盖，Rust/native/QML 本地验证通过。任务仍等待 Windows、macOS、Linux nightly 输入法与 Unicode 矩阵结果。
