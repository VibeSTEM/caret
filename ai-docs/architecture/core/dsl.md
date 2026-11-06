# `.pa` DSL 架构

更新日期：2026-11-06。`.pa` 是 Caret 的人类可读声明源，用于界面语言、主题、类型化变量和受限状态机描述。语言目录在构建期编译，内置主题在 Rust 服务初始化时解析求值。语法、kind 和诊断行为见[`.pa` DSL 规范](../../standards/dsl.md)；本文说明它在工程中的职责、使用方和稳定边界。

## 职责与边界

`caret-dsl-core` 提供共享解析、格式化、校验、求值和转换能力。它把源文件解析成类型化文档与诊断，再由调用方按 kind 执行相应处理。`caret-dslc` 是面向维护者的命令行入口，供检查、格式化和输出语言 TS 使用。

DSL 只承载声明数据，不是通用脚本语言。解析和求值不执行任意代码，也不提供文件、网络或 shell 访问。业务语义由拥有该数据的模块解释：例如主题键及其类型由 `caret-app` 的主题 schema 约束。公共 parser 负责统一文法与诊断，不让各业务模块另写解析器。

```text
language .pa ── launcher/build.rs 聚合并生成 TS ── native/i18n CMake 调用 lrelease ── QM ── Qt 加载

theme / variables .pa ── caret-app Rust 服务初始化时解析、求值 ── ViewModel 快照 ── QML

fsm .pa ── 当前无产品来源或构建消费者；有真实消费者后再接入构建期元数据生成

caret-dsl-core ── 共用 parser / validator / formatter / evaluator / converter
caret-dslc ── 维护者 CLI：check / format / emit-ts
```

Qt/QML 界面不解析 `.pa`。语言由 Qt 加载构建生成的 QM；主题和变量由 Rust `ThemeService` 在初始化时校验、求值并以快照交给 ViewModel。QML 只消费投影，不维护另一份主题默认值。`caret-dsl-core` 已提供 FSM 解析和 Rust 元数据生成能力，但当前没有产品 `.pa` FSM 来源或生产构建调用方；只有出现真实流程消费者后才接入构建。

## 使用方

| 使用方 | 使用内容 | 责任 |
| --- | --- | --- |
| launcher 构建脚本、native/i18n CMake | `language` | launcher 聚合来源并生成临时 TS；CMake 调用 `lrelease` 生成 QM，Qt 加载打包后的翻译资源 |
| `caret-app` 主题服务 | `theme`、`variables` | 启动时解析并求值内嵌来源，以主题 schema 校验键和值，生成供界面读取的快照 |
| `caret-dsl-core` FSM API | `fsm` | 当前只有 parser、formatter 和 Rust 元数据生成能力，没有生产来源或构建调用方；未来接入时只生成构建期转移元数据，业务状态迁移仍由 Rust 实现 |
| `caret-dslc` | 所有支持的 kind | 提供检查、规范化格式化和语言 TS 输出，便于维护者本地验证 |

语言构建或主题服务初始化失败时，不发布部分目录或快照；生成文件、临时 TS 与 QM 留在构建目录，不作为源文件维护。FSM 声明本身不含动作或 I/O，未来生成的元数据也不改变领域服务的权威状态归属。

## 稳定性承诺

- `.pa` 的 UTF-8 文本格式、已支持 kind、关键字、版本解释、错误码及源码位置属于维护者和构建流程共同依赖的输入契约。变更前需要检查仓库中的所有来源及工具消费者，并同步更新规范、夹具和诊断测试。
- 已知的来源行为（文法、kind 与诊断契约）随 `caret-dsl-core` 移植而保留。更改扩展名、既有关键字或必需版本规则属于单独的不兼容变更，不能作为普通格式调整悄然发布。
- `caret-dsl-core` 位于工作区内且 `publish = false`。它当前没有对外发布的 Rust semver API 承诺；Rust 模块/API 可随内部调用方一起演进，但必须维持上述 `.pa` 输入契约及构建产物语义。
- 生成物是派生数据，不形成独立的源格式承诺。改变 TS、QM、主题快照或 FSM 生成方式时，应验证实际构建消费者和失败时不发布部分结果的行为。

扩展 DSL 时，先确定数据属于哪一类、由哪个 Rust 服务解释，以及构建失败如何阻止不完整产物进入应用。只有出现真实消费者后才增加新的 kind；不为抽象复用添加脚本、I/O 或运行时解释能力。
