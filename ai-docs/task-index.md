# Task 索引

更新日期：2026-10-28。非平凡改动先从[模板](task/_template.md)建 task，在本索引登记，然后再动代码。行为规格在[架构](architecture/README.md)和[规范](standards/README.md)。2026-10-25 删去未执行的旧队列，现行任务从 001 重新编号。

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
| 008 | M0 集成 | [M0：基础文本编辑纵向切片](task/008-impl-document.md) | P2 | 021、022、023、024、026、027、030 | M | implemented |
| 009 | 命令输入 | [M1：`\` 命令](task/009-impl-commands.md) | P2 | 008 | M | planned |
| 010 | 结构导航 | [M2：结构导航与环境命令](task/010-impl-structure.md) | P2 | 009、020 | M | planned |
| 011 | 数学排版 | [M3：公式](task/011-impl-math.md) | P2 | 010 | L | planned |
| 012 | 表格排版 | [M3：表格](task/012-impl-table.md) | P2 | 010 | M | planned |
| 013 | 性能 | [M4：脏段落排版](task/013-impl-layout.md) | P2 | 011、012、027 | M | planned |
| 014 | CLI | [应用命令行](task/014-impl-cli.md) | P2 | 008 | M | planned |
| 015 | 文档维护 | [文档审查修正](task/015-doc-review-fixes.md) | P0 | — | S | done |
| 016 | SDK 发布 | [独立 GoogleTest SDK 生产与发布](task/016-googletest-package.md) | P0 | 001 | S | done |
| 017 | 代码质量 | [当前代码整体审查与优化](task/017-code-review.md) | P0 | — | M | done |
| 018 | 文档生命周期 | [文档状态、关闭确认与编辑历史](task/018-document-lifecycle-history.md) | P2 | 008 | L | in-progress |
| 019 | AutoSave | [AutoSave 策略设计](task/019-autosave-design.md) | P2 | 018 | S | planned |
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
| 031 | 画布交互 | [当前环境背景高亮](task/031-environment-highlight.md) | P2 | 027 | M | ready |
| 032 | 文档兼容 | [未知节点安全预览](task/032-unknown-node-preview.md) | P2 | 021、027 | M | ready |
| 033 | 排版基线 | [OpenType BASE 脚本基线与行框溢出](task/033-open-type-baselines.md) | P2 | 024、026、027 | M | ready |
| 034 | 性能 | [Qt / Rust 性能测量与热点优化](task/034-performance-profiling.md) | P2 | 024、026、027、030 | L | in-progress |

分类列描述实现主题，优先级列使用 P0–P2；产品里程碑用 M0–M4 标记在任务标题和需求文档中。020 的模型决策已定稿；按依赖推进 021/022 与编辑所需的页面布局链，之后由 008 集成基础文本编辑；025 的格式命令不阻塞 M0。018 依赖 008；019 在 018 后单独设计 AutoSave。009 建立通用命令入口，010 增加环境命令和结构状态。公式和表格画在画布里。014 不挡后续里程碑。

004 负责 Qt 供给与最小 QML 构建和加载测试，005 在该工程上接桌面入口和 FFI。011、012 的后完成者负责[单元格公式集成验收](architecture/editor/math-and-table.md#m3-集成验收)，通过后 013 才可开始。015 是本次文档维护，不作为基础设施或产品里程碑的实现证据。

021 的 done 只表示文档树、`.cdoc` 校验和 `caret-app` 存储边界完成，不表示 M0 编辑器已完成；M0 仍需 022–024、026、027 和 008 集成验收。025 格式命令不阻塞 M0。`caret-doc` 已落地；`caret-layout`、`caret-structure`、`caret-engine` 和 `caret-cli` 按各自任务的依赖顺序创建，不提前建立空 crate。

## 待决策

020 已通过最终独立评审。后续实现任务须遵守其定稿的产品语义；产品行为以架构为准，版本和移植边界以规范为准。
