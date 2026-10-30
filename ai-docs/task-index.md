# Task 索引

更新日期：2026-10-29。非平凡改动先从[模板](task/_template.md)建 task，在本索引登记，然后再动代码。行为规格在[架构](architecture/README.md)和[规范](standards/README.md)。2026-10-25 删去未执行的旧队列，现行任务从 001 重新编号。

## 使用方式

1. 复制[模板](task/_template.md)为 `task/NNN-kebab-case.md`。编号取当前最大编号加一，不复用。
2. 在下方队列登记，并核对依赖没有形成环。
3. 依赖满足后标为 ready。开始实现时标为 in-progress。范围有变化，先改 task。
4. 每次 commit 更新该 task 的工作记录和验证表。验收都有证据后，task 和索引一起标为 done。
5. 已有任务范围内的小修复补进原 task。新功能另建 task。

## 状态约定

| 状态 | 含义 |
| --- | --- |
| draft | 缺少范围或验收条件，不能开始 |
| planned | 已编排，依赖未满足 |
| ready | 可开始，尚无实现 |
| in-progress | 正在实施 |
| implemented | 仓库内验证已完成，外部验收待补 |
| blocked | 有具体阻塞和解除条件 |
| done | 验收完成且有证据 |
| deferred | 暂不排入 |
| cancelled | 已取消，保留编号与原因 |

## 执行顺序

基础设施已接入，QML 窗口外壳按 007 当前形态冻结。编辑先从 020 文档模型、021 存储和 022 基础文本输入开始，再补齐 023/024/026/027 的最小编辑布局和画布能力；030 补齐 Qt IME、导航与选区交互，最后由 008 集成输入、保存、重开并完成 M0 验收。025 格式命令不阻塞 M0，先实现默认字体设置与局部 Font 环境命令，不新增字体选择窗口或环境工具按钮。009 建立 `\` 命令入口，010 通过命令和快捷键创建、嵌套与操作环境。只有编辑纵向切片确实需要时才改动 QML 外观，并先写静态原型再落成 QML。

来源是 [panta](https://github.com/Yuki-Nagori/panta) `ee84d95`。移植提交写明实际复制的 commit。不移植几何、网格、VTK、求解协议，以及只服务于这些领域的 crate 和对话框。

优先级：P0 基础设施，P1 当前里程碑的界面，P2 该里程碑的编辑器实现。规模是草估：S 不超过 1 天，M 为 2–5 天，L 超过 1 周。

## 任务队列

| 编号 | 分类 | 任务 | 优先级 | 依赖 | 规模 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| 001 | 工具链 | [工具链、workspace 与基础 crate](task/001-infra-toolchain.md) | P0 | — | L | done |
| 002 | DSL | [DSL 核心与 dslc](task/002-infra-dsl.md) | P0 | 001 | M | done |
| 003 | 应用服务 | [路径、设置、语言与主题](task/003-infra-app.md) | P0 | 001、002 | M | done |
| 004 | Qt/QML | [QML 组件库与通用对话框](task/004-infra-qml-kit.md) | P0 | 001、002 | L | done |
| 005 | 桌面入口 | [launcher、CMake 与 FFI 骨架](task/005-infra-launcher.md) | P0 | 003、004 | M | done |
| 006 | CI/发布 | [CI 与打包管线](task/006-infra-ci.md) | P0 | 005 | M | implemented |
| 007 | 窗口 UI | [M0 窗口](task/007-ui-m0.md) | P1 | 006 | S | implemented |
| 008 | M0 集成 | [M0：基础文本编辑纵向切片](task/008-impl-document.md) | P2 | 021、022、023、024、026、027、030 | M | done |
| 009 | 命令输入 | [M1：`\` 命令](task/009-impl-commands.md) | P2 | 008 | M | done |
| 010 | 结构导航 | [M2：结构导航与结构命令](task/010-impl-structure.md) | P2 | 009、020 | M | done |
| 011 | 数学排版 | [M3：公式总体验收](task/011-impl-math.md) | P2 | 010、042、043、044 | M | planned |
| 012 | 表格排版 | [M3：表格总体验收](task/012-impl-table.md) | P2 | 045、046 | S | planned |
| 013 | 性能 | [M4：脏段落排版](task/013-impl-layout.md) | P2 | 011、012、027 | M | planned |
| 014 | CLI | [应用命令行](task/014-impl-cli.md) | P2 | 008 | M | planned |
| 015 | 文档维护 | [文档审查修正](task/015-doc-review-fixes.md) | P0 | — | S | done |
| 016 | SDK 发布 | [独立 GoogleTest SDK 生产与发布](task/016-googletest-package.md) | P0 | 001 | S | done |
| 017 | 代码质量 | [当前代码整体审查与优化](task/017-code-review.md) | P0 | — | M | done |
| 018 | 文档生命周期 | [文档状态、关闭确认与编辑历史](task/018-document-lifecycle-history.md) | P2 | 008 | L | done |
| 019 | AutoSave | [AutoSave 策略设计](task/019-autosave-design.md) | P2 | 018 | M | done |
| 020 | 模型设计 | [Paragraph/Run 语义与文档模型细化](task/020-document-model-decision.md) | P2 | — | M | done |
| 021 | 文档存储 | [文档树与 .cdoc 持久化](task/021-document-tree-and-storage.md) | P2 | 020 | L | done |
| 022 | 输入编辑 | [文本编辑、选择与 IME](task/022-text-editing-and-ime.md) | P2 | 021 | L | done |
| 023 | 页面几何 | [页面尺寸、节与几何](task/023-page-geometry.md) | P2 | 021 | M | done |
| 024 | 字体塑形 | [字体回退与文字塑形](task/024-text-shaping.md) | P2 | 022、023 | L | done |
| 025 | 格式命令 | [字符与段落格式命令](task/025-text-formatting.md) | P2 | 009、021、022、024 | M | planned |
| 026 | 段落排版 | [行距、段间距与分页](task/026-paragraph-layout.md) | P2 | 023、024 | L | done |
| 027 | 画布命中 | [页面画布、命中与布局快照](task/027-canvas-and-hit-testing.md) | P2 | 022、024、026 | L | done |
| 028 | 文档治理 | [文档一致性与职责整理](task/028-documentation-consolidation.md) | P1 | — | M | done |
| 029 | 应用偏好 | [Preferences 服务与跨层常量归属](task/029-preferences-and-constants.md) | P2 | 003、027 | M | done |
| 030 | 输入交互 | [IME、键盘导航与选区交互](task/030-ime-navigation-selection.md) | P2 | 022、027 | L | done |
| 031 | 画布交互 | [当前环境背景高亮](task/031-environment-highlight.md) | P2 | 010、027 | M | in-progress |
| 032 | 文档兼容 | [未知节点安全预览](task/032-unknown-node-preview.md) | P2 | 021、027 | M | ready |
| 033 | 排版基线 | [OpenType BASE 脚本基线与行框溢出](task/033-open-type-baselines.md) | P2 | 024、026、027 | M | ready |
| 034 | 性能 | [Qt / Rust 性能测量与热点优化](task/034-performance-profiling.md) | P2 | 024、026、027、030 | L | done |
| 035 | 文档模式 | [Editing、Review、View 模式语义与行为](task/035-document-work-modes.md) | P2 | 018 | L | planned |
| 036 | 图标与致谢 | [Icons8 图标替换与 Settings 关于页面署名](task/036-icons8-icons-and-attribution.md) | P1 | 018 | M | planned |
| 037 | AutoSave | [AutoSave 实现](task/037-autosave-implementation.md) | P2 | 018、019、029 | L | in-progress |
| 038 | 窗口架构 | [多窗口文档会话与全局操作协调](task/038-multiwindow-document-sessions.md) | P2 | 037 | L | planned |
| 039 | Settings UI | [Settings 中的主题与界面语言](task/039-settings-theme-language-ui.md) | P2 | 003、004、029 | M | done |
| 040 | 架构治理 | [架构与结构审计整改](task/040-architecture-structure-cleanup.md) | P2 | — | M | done |
| 041 | 性能 | [性能基准补齐与热点优化](task/041-performance-benchmarks-and-optimization.md) | P2 | 024、026、027、030 | M | in_progress |
| 042 | 数学排版 | [M3：latex-rust 后端验证](task/042-latex-rust-math-backend.md) | P2 | — | M | ready |
| 043 | 数学排版 | [M3：公式模型与编辑](task/043-math-model-and-editing.md) | P2 | 010、042 | L | planned |
| 044 | 数学排版 | [M3：公式布局、画布渲染与表格集成](task/044-math-layout-and-rendering.md) | P2 | 012、027、043 | L | planned |
| 045 | 表格 | [M3：表格模型与交互](task/045-table-model-and-editing.md) | P2 | 010、021 | M | planned |
| 046 | 表格 | [M3：表格布局与画布](task/046-table-layout-and-canvas.md) | P2 | 027、045 | M | planned |
| 047 | 文档交换 | [Markdown 导入导出与转换框架](task/047-markdown-interchange.md) | P2 | 008、010–012、018、021、023–027、029、050–053 | L | planned |
| 048 | 文档交换 | [DOCX 导入导出适配器](task/048-docx-interchange.md) | P2 | 047 | L | planned |
| 049 | 文档交换 | [TeX 文档导入导出适配器](task/049-tex-interchange.md) | P2 | 047 | L | planned |
| 050 | 文档模型 | [代码、引用与链接语义节点及编辑](task/050-document-code-links-quotes.md) | P2 | 010、021、025 | L | planned |
| 051 | 画布 | [代码、引用、链接与列表的布局和画布呈现](task/051-code-links-quotes-layout.md) | P2 | 024、027、050 | L | planned |
| 052 | 文档模型 | [图片节点、媒体资源与编辑](task/052-image-model-and-editing.md) | P2 | 021 | M | planned |
| 053 | 画布 | [图片布局与画布呈现](task/053-image-layout-and-canvas.md) | P2 | 024、027、052 | M | planned |
| 054 | 结构命令 | [APA 标题层级命令与默认样式](task/054-apa-heading-commands.md) | P2 | 010、024、025、026、027 | M | planned |

分类列描述实现主题，优先级列使用 P0–P2；产品里程碑用 M0–M4 标记在任务标题和需求文档中。020 的模型决策已定稿；M3 公式由 011 总体验收及 042–044 子任务推进；表格由 012 总体验收及 045/046 拆分推进；按依赖推进 021/022 与编辑所需的页面布局链，之后由 008 集成基础文本编辑；025 的格式命令不阻塞 M0。018 依赖 008；019 在 018 后单独设计 AutoSave。009 建立通用命令入口，010 增加环境命令和结构状态。公式和表格画在画布里。014 不挡后续里程碑。

004 负责 Qt 供给与最小 QML 构建和加载测试，005 在该工程上接桌面入口和 FFI。011 是 M3 公式总体验收；042 验证并锁定数学后端，043 实现公式模型与编辑，044 负责公式布局渲染及[单元格公式集成验收](architecture/editor/math-and-table.md#m3-集成验收)。012 是表格总体验收，由 045 实现模型与交互、046 实现布局与画布。011、012、044 全部验收通过后 013 才可开始。015 是本次文档维护，不作为基础设施或产品里程碑的实现证据。

031 的颜色语义区分行内环境、块级环境和命令输入提示。009 负责命令提示 token，010 提供块级 EnvironmentNode 的布局范围；块级淡绿色高亮已接入，031 继续完成行内高亮和整体验收。三类颜色均属于 GUI Theme，不写入文档排版主题或 `.cdoc`。

008 的 Windows、macOS、Linux Nightly 合成 IME/Unicode 矩阵已通过并完成验收；009 可直接基于已完成的 008 开始。

021 的 done 只表示文档树、`.cdoc` 校验和 `caret-app` 存储边界完成，不表示 M0 编辑器已完成；M0 仍需 022–024、026、027 和 008 集成验收。025 格式命令不阻塞 M0。`caret-doc` 已落地；`caret-layout`、`caret-structure`、`caret-engine` 和 `caret-cli` 按各自任务的依赖顺序创建，不提前建立空 crate。019 定义 AutoSave 策略并通过独立评审后，037 实现单窗口偏好、触发、保存冲突和恢复；未命名文档开启 AutoSave 必须先让用户选择保存路径。038 在 037 完成后迁移为窗口级文档运行时，并实现跨窗口 AutoSave 开启协调。

外部格式使用 [Caret 规范文档树](architecture/editor/document-interchange.md) 作为唯一中间模型，禁止引入通用最低公分母 AST；格式专属 IR 只存在于适配器边界。050–053 补齐代码、引用、链接、图片及画布能力；011/012 补齐公式和表格。047 必须等编辑会话、关闭保护、全部节点/媒体和布局画布能力完成后，才实现完整 Markdown 导入导出；048/049 在 047 之后分别实现完整 DOCX 与 TeX 映射。

034 的本机性能测量与有数据支持的热点优化已完成；Qt GPU nightly 核验、真实窗口大文档端到端测量及额外基准补齐移交 041，由 041 独立推进。039 接通现有 Rust Settings、Theme、Language 服务与设置 UI，并确保 locale 选项对应真实构建资源。040 承接架构审计中需要独立迁移或回归验证的整改；可直接安全清理的孤儿组件在发现后及时删除。

## 待决策

020 已通过最终独立评审。后续实现任务须遵守其定稿的产品语义；产品行为以架构为准，版本和移植边界以规范为准。
