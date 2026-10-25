# Task 索引

更新日期：2026-10-25。非平凡改动先从[模板](task/_template.md)建 task，在本索引登记，然后再动代码。行为规格在[架构](architecture/README.md)和[规范](standards/README.md)。2026-10-25 删去未执行的旧队列，现行任务从 001 重新编号。

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

先做完基础设施，再按[需求](architecture/requirements.md#里程碑)做界面和实现。界面只做当前里程碑要用的部分，并且先写静态 HTML，再落成 QML。M0 只要能看见一页的窗口。

来源是 [panta](https://github.com/Yuki-Nagori/panta) `ee84d95`。移植提交写明实际复制的 commit。不移植几何、网格、VTK、求解协议，以及只服务于这些领域的 crate 和对话框。

优先级：P0 基础设施，P1 当前里程碑的界面，P2 该里程碑的编辑器实现。规模是草估：S 不超过 1 天，M 为 2–5 天，L 超过 1 周。

## 任务队列

| 编号 | 任务 | 阶段 | 依赖 | 规模 | 状态 |
| --- | --- | --- | --- | --- | --- |
| 001 | [工具链、workspace 与基础 crate](task/001-infra-toolchain.md) | P0 | — | L | done |
| 002 | [DSL 核心与 dslc](task/002-infra-dsl.md) | P0 | 001 | M | done |
| 003 | [路径、设置、语言与主题](task/003-infra-app.md) | P0 | 001、002 | M | done |
| 004 | [QML 组件库](task/004-infra-qml-kit.md) | P0 | 001、002 | L | done |
| 005 | [launcher、CMake 与 FFI 骨架](task/005-infra-launcher.md) | P0 | 003、004 | M | done |
| 006 | [CI 与打包管线](task/006-infra-ci.md) | P0 | 005 | M | implemented |
| 007 | [M0 窗口](task/007-ui-m0.md) | P1 | 006 | S | in-progress |
| 008 | [M0：打开、打字、保存](task/008-impl-document.md) | P2 | 007 | L | planned |
| 009 | [M1：`\` 命令与补全层](task/009-impl-commands.md) | P2 | 008 | M | planned |
| 010 | [M2：结构导航](task/010-impl-structure.md) | P2 | 009 | M | planned |
| 011 | [M3：公式](task/011-impl-math.md) | P2 | 010 | L | planned |
| 012 | [M3：表格](task/012-impl-table.md) | P2 | 010 | M | planned |
| 013 | [M4：脏段落排版](task/013-impl-layout.md) | P2 | 011、012 | M | planned |
| 014 | [应用命令行](task/014-impl-cli.md) | P2 | 008 | M | planned |
| 015 | [文档审查修正](task/015-doc-review-fixes.md) | P0 | — | S | done |
| 016 | [独立 GoogleTest SDK 生产与发布](task/016-googletest-package.md) | P0 | 001 | S | done |
| 017 | [当前代码整体审查与优化](task/017-code-review.md) | P0 | — | M | done |
| 018 | [文档状态、关闭确认与编辑历史](task/018-document-lifecycle-history.md) | P2 | 008 | L | planned |
| 019 | [AutoSave 策略设计](task/019-autosave-design.md) | P2 | 018 | S | planned |

007 是简化后的文档窗口：自定义标题栏、Home 与右侧 Comments / Editing / Share、居中的页面、底栏左侧页码字数语言、右侧缩放尺。009 才加补全层。010 在同一扇窗口上加焦点框和结构状态。公式和表格画在画布里。014 不挡后续里程碑。

004 负责 Qt 供给与最小 QML 构建和加载测试，005 在该工程上接桌面入口和 FFI。011、012 的后完成者负责[单元格公式集成验收](architecture/editor/math-and-table.md#m3-集成验收)，通过后 013 才可开始。015 是本次文档维护，不作为基础设施或产品里程碑的实现证据。

006 完成前不创建 `caret-doc`、`caret-layout`、`caret-structure`、`caret-engine`、`caret-cli`。

## 待决策

没有阻塞本队列的未决问题。产品行为以架构为准，版本和移植边界以规范为准。
