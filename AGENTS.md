# 仓库协作指南

Caret 是轻量的所见即所得结构化编辑器：边写边看结构。领域数据、业务规则和 DSL 快照在 Rust；窗口、输入和界面在 Qt 6 / QML。工程基础设施从维护者自己的 [panta](https://github.com/Yuki-Nagori/panta) 移植，范围见[技术规范索引](ai-docs/standards/README.md)。

仓库已有 `README.md`、`LICENSE` 和 `ai-docs/standards`。还没有 Cargo、CMake、QML 和 CI。规范里的命令、路径和 crate 是规划，落地前不能当成已经可以运行。

## 工作入口

| 工作内容 | 入口 |
| --- | --- |
| 开始任务、查依赖与状态 | [任务索引](ai-docs/task-index.md) / [任务模板](ai-docs/task/_template.md) |
| 查产品行为与编辑器契约 | [架构总览](ai-docs/architecture/README.md) |
| 查版本、分层、编码和移植范围 | [技术规范索引](ai-docs/standards/README.md) |
| 查未排期构想 | [ideas](ai-docs/ideas/) |
| 查外部调查 | [research](ai-docs/research/) |

只阅读当前任务相关的架构与规范。工程栈以规范为准。架构文档里尚未改完的实现描述，不覆盖规范。外部调查只放在 `ai-docs/research/`。任务范围变化时先更新所属任务；长期决定同步到架构或规范。

## 任务与代码约束

- 实施前创建或更新 `ai-docs/task/NNN-name.md`，并登记 [任务索引](ai-docs/task-index.md)。每个任务一个文件，元数据用无序列表。小范围修复更新所属任务，不另建任务。验收后写下实际验证结果，并同步状态和索引。
- 注释遵循[注释规范](ai-docs/standards/comments.md)，新文件遵循[仓库文件规范](ai-docs/standards/repository-hygiene.md)，验证遵循[验证与评审](ai-docs/standards/validation-and-review.md)。
- 不保留死代码。替换时同步删除旧实现。确需兼容时使用 `COMPAT(...)` 并登记清理任务，见[代码生命周期](ai-docs/standards/code-lifecycle.md)。
- 每次提交保持代码、测试、配置、文档和任务一致，遵循[提交规范](ai-docs/standards/commits.md)。不把已知损坏状态留给下一次提交。
- 从 panta 移植时，提交正文写来源仓库和 commit，并把 `panta` 前缀改成 `caret`。几何内核、网格、求解协议，以及只服务于这些领域的 crate、面板和工艺对话框不搬入。

## 架构边界

- Rust 承担领域数据、业务规则、路径、设置、语言、主题和 DSL。CXX 只声明映射和签名。C++ 适配层持有 Qt 对象、窗口、输入和画布，并封装异常与所有权。
- QML 使用移植来的原子组件、组合组件和通用对话框，经 C++ ViewModel 提交意图。QML 不解析 `.pa`，不直接读写文档存储。
- 自有 C++ 使用 C++20。依赖方向见[分层规范](ai-docs/standards/layering.md)。
- Cargo 是统一入口，CMake 拥有 native 构建图。版本见[技术基线](ai-docs/standards/baseline.md)。下载的工具和 SDK 放在仓库根 `.toolchain/`，编译产物留在 `target/`。
- 用户可见的行为由与 Qt 无关的 Rust 服务提供。界面提交意图；以后的命令行适配同一服务。
- 快捷键借鉴 Mogan 的键位语义，不复制它的代码、键表或注释。Mogan 是 GPL。键位说明见[快捷键](ai-docs/architecture/interface/keyboard-shortcuts.md)。

## 构建与验证

基础设施落地前，只运行仓库里实际存在的检查。文档改动检查相对链接和暂存范围。

落地后的日常入口见[测试规范](ai-docs/standards/testing.md)：先看 `cargo test-plan`，再跑受影响的 `cargo test-scope` 或 `cargo verify-changed`。完整聚合 `cargo test --locked --workspace` 用于范围无法缩小的验收和夜间任务。真实窗口验收见 [Qt](ai-docs/standards/qt.md)，无头测试不能代替它。

## Subagent 使用规则

### 何时调用

- 每个非平凡任务在交付前安排一次独立评审。评审对照需求、风险和完整 diff，不复述实现者的自检。拼写、格式或单行修复不因此强制评审，用户明确要求时除外。
- 动到排版结果或缓存键时，评审确认基准没有退化，键也没有因此扩散。
- 覆盖率缺口按[测试规范](ai-docs/standards/testing.md)拆给 subagent 调查或修复。主代理整合并运行验证。不为了数字机械补断言或豁免行。
- 复杂问题能拆成互不重叠的产出时，可以并行委派。先划清边界，避免两个代理同时改同一处。
- 主线工作可以直接完成、又没有独立审查要求时，不为形式启动 subagent。

### 默认模型

- 代码和架构的独立评审默认使用 `gpt-6.1-sol`，推理强度为高。用户对某次工作指定了其他模型或强度时，以该指示为准。
- 调查和实现沿用当前代理的默认模型。工具支持时，委派前写明模型和推理强度。

### 委派与评审边界

- 委派说明写明目标、范围、相关路径、验收条件、要运行的检查，以及只读还是允许修改。独立评审默认只读。
- 评审报告给出可操作的问题、文件和行号、触发条件和影响。没有发现时也写检查范围和限制。主代理判断发现是否成立，并负责修复、验证和更新文档。评审结论记在所属任务里，不直接当成通过证明。
- 等待评审时，主代理继续做不依赖评审结论的工作。修复后重跑受影响的检查。只有范围无法缩小或明确要求完整验收时才跑全量测试。
- Subagent 不自行提交、推送、创建或合并 PR，也不对外沟通，除非用户明确授权。

## 分支、GitHub 与提交

- 查询和操作 GitHub 的 issue、PR、Actions 和仓库状态，使用已认证的 `gh` CLI。写操作前核对仓库、目标编号、当前分支和本次授权。
- 代码、测试、依赖、构建、CI，以及同时包含实现和配套文档的工作，在专用分支上完成，经 PR 合并到 `main`。
- 只有纯文档改动，且用户明确要求直接提交到 `main` 时，才在 `main` 上提交。提交前确认没有混入其他改动。
- 提交、推送、合并或改写共享历史，都要有当前会话的明确授权。完成评审本身不是授权。获准推送实现前，先完成独立评审。
- 许可证：Caret 是 Apache-2.0。移植文件保留原版权声明。LLVM、Qt 和其他第三方工具链沿用各自许可证，不套用仓库许可证。
