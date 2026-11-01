# Task 索引

更新日期：2026-11-01。非平凡改动先从[模板](task/_template.md)建 task，在本索引登记，然后再动代码。行为规格在[架构](architecture/README.md)和[规范](standards/README.md)。2026-10-25 删去未执行的旧队列，现行任务从 001 重新编号。

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
| planned | 已编排，但依赖或其他明确的开始条件尚未满足 |
| ready | 可开始，尚无实现 |
| in-progress | 正在实施 |
| implemented | 仓库内验证已完成，外部验收待补 |
| blocked | 有具体阻塞和解除条件 |
| done | 验收完成且有证据 |
| deferred | 暂不排入 |
| cancelled | 已取消，保留编号与原因 |

## 规划约定

依赖列决定任务顺序；优先级和规模用于排期估算（S 不超过 1 天，M 为 2–5 天，L 超过 1 周，均为粗略估计）。产品行为以[架构文档](architecture/README.md)为准，工程与验证要求以[规范文档](standards/README.md)为准。本索引只维护任务状态、依赖和分类，不复制设计细节或进度摘要。

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
| 011 | 数学排版 | [M3：公式总体验收](task/011-impl-math.md) | P2 | 010、042、043、044、045、046 | M | planned |
| 012 | 表格排版 | [M3：表格总体验收](task/012-impl-table.md) | P2 | 045、046 | S | planned |
| 013 | 性能 | [M4：脏段落排版](task/013-impl-layout.md) | P2 | 011、012、027 | M | planned |
| 014 | CLI | [应用命令行](task/014-impl-cli.md) | P2 | 008 | M | ready |
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
| 025 | 格式命令 | [字符与段落格式命令](task/025-text-formatting.md) | P2 | 009、021、022、024 | M | in-progress |
| 026 | 段落排版 | [行距、段间距与分页](task/026-paragraph-layout.md) | P2 | 023、024 | L | done |
| 027 | 画布命中 | [页面画布、命中与布局快照](task/027-canvas-and-hit-testing.md) | P2 | 022、024、026 | L | done |
| 028 | 文档治理 | [文档一致性与职责整理](task/028-documentation-consolidation.md) | P1 | — | M | done |
| 029 | 应用偏好 | [Preferences 服务与跨层常量归属](task/029-preferences-and-constants.md) | P2 | 003、027 | M | done |
| 030 | 输入交互 | [IME、键盘导航与选区交互](task/030-ime-navigation-selection.md) | P2 | 022、027 | L | done |
| 031 | 画布交互 | [当前环境背景高亮](task/031-environment-highlight.md) | P2 | 010、027、033 | M | done |
| 032 | 文档兼容 | [未知节点安全预览](task/032-unknown-node-preview.md) | P2 | 021、027 | M | ready |
| 033 | 排版 | [多语言共享行距网格与选区几何](task/033-open-type-baselines.md) | P2 | 024、026、027 | M | done |
| 034 | 性能 | [Qt / Rust 性能测量与热点优化](task/034-performance-profiling.md) | P2 | 024、026、027、030 | L | done |
| 035 | 文档模式 | [Editing、Review、View 模式语义与行为](task/035-document-work-modes.md) | P2 | 018 | L | planned |
| 036 | 图标与致谢 | [Icons8 图标替换与 Settings 关于页面署名](task/036-icons8-icons-and-attribution.md) | P1 | 018 | M | planned |
| 037 | AutoSave | [AutoSave 实现](task/037-autosave-implementation.md) | P2 | 018、019、029 | L | in-progress |
| 038 | 窗口架构 | [多窗口文档会话与全局操作协调](task/038-multiwindow-document-sessions.md) | P2 | 037 | L | planned |
| 039 | Settings UI | [Settings 中的主题与界面语言](task/039-settings-theme-language-ui.md) | P2 | 003、004、029 | M | done |
| 040 | 架构治理 | [架构与结构审计整改](task/040-architecture-structure-cleanup.md) | P2 | — | M | done |
| 041 | 性能 | [性能基准补齐与热点优化](task/041-performance-benchmarks-and-optimization.md) | P2 | 024、026、027、030 | M | in-progress |
| 042 | 数学排版 | [M3：数学后端选型与验证](task/042-math-backend.md) | P2 | — | M | done |
| 043 | 数学排版 | [M3：公式模型与编辑](task/043-math-model-and-editing.md) | P2 | 010、042 | L | in-progress |
| 044 | 数学排版 | [M3：公式布局与画布渲染](task/044-math-layout-and-rendering.md) | P2 | 027、043 | L | in-progress |
| 045 | 表格 | [M3：表格模型与交互](task/045-table-model-and-editing.md) | P2 | 010、021、043 | M | planned |
| 046 | 表格 | [M3：表格布局与画布](task/046-table-layout-and-canvas.md) | P2 | 027、043、044、045 | M | planned |
| 047 | 文档交换 | [Markdown 导入导出与转换框架](task/047-markdown-interchange.md) | P2 | 008、010–012、018、021、023–027、029、050–053 | L | planned |
| 048 | 文档交换 | [DOCX 导入导出适配器](task/048-docx-interchange.md) | P2 | 047 | L | planned |
| 049 | 文档交换 | [TeX 文档导入导出适配器](task/049-tex-interchange.md) | P2 | 047 | L | planned |
| 050 | 文档模型 | [代码、引用与链接语义节点及编辑](task/050-document-code-links-quotes.md) | P2 | 010、021、025 | L | planned |
| 051 | 画布 | [代码、引用、链接与列表的布局和画布呈现](task/051-code-links-quotes-layout.md) | P2 | 024、027、050 | L | planned |
| 052 | 文档模型 | [图片节点、媒体资源与编辑](task/052-image-model-and-editing.md) | P2 | 021 | M | planned |
| 053 | 画布 | [图片布局与画布呈现](task/053-image-layout-and-canvas.md) | P2 | 024、027、052 | M | planned |
| 054 | 结构命令 | [APA 标题层级命令与默认样式](task/054-apa-heading-commands.md) | P2 | 010、024、025、026、027 | M | planned |
| 055 | 排版基线 | [OpenType BASE 脚本基线与字形溢出诊断](task/055-opentype-base-script-alignment.md) | P2 | 024、026、027、033 | L | ready |
| 056 | 字体设置 UI | [文档字体设置专用布局](task/056-font-settings-and-family-picker.md) | P2 | 024、025、039 | M | deferred |
| 057 | 结构命令 | [块环境命令作用于文本选区](task/057-block-environment-selection-commands.md) | P2 | 010、022 | M | planned |
| 058 | QML 架构 | [QML 主题令牌与固定常量分层](task/058-qml-theme-tokens-and-constants.md) | P2 | 029、039 | M | ready |
| 059 | 字体命令 | [命令字体选择器交互与布局优化](task/059-command-font-picker.md) | P2 | 024、025 | M | planned |
| 060 | 字符格式 | [字号命令与局部字号格式](task/060-font-size-formatting.md) | P2 | 022、024、026、027 | M | ready |
| 061 | 字符格式 | [字色、高亮与字符底纹](task/061-text-paint-formatting.md) | P2 | 022、024、027 | M | ready |
| 062 | 段落格式 | [段落格式命令](task/062-paragraph-formatting.md) | P2 | 022、026、027 | M | ready |
| 063 | 文档样式 | [文档排版主题与命名样式](task/063-document-typography-styles.md) | P2 | 020、021 | L | ready |
| 064 | 字体 | [内置字体角色与统一默认标记](task/064-builtin-font-roles.md) | P2 | 024、025、042 | M | in-progress |

分类描述任务主题；里程碑和实施边界写在对应任务中。任务拆分、状态变更及执行记录以各任务文档为准。
