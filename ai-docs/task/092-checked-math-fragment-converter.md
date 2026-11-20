# 092 — 共享数学片段转换接口

- 状态：implemented
- 依赖：043
- 优先级：P1
- 创建 / 更新：2026-11-16 / 2026-11-21

## 目标与背景

`caret-math` 已有受限 TeX 数学解析器和 MathTree/Align serializer。Markdown 047、TeX 文档 049、公式源码输入及富剪贴板都需要复用同一数学子语言转换边界，但当前序列化接口返回裸字符串，不能报告未知语义或扩展字段无法表达造成的损失。本任务为这一共享领域能力定义可检查的转换 API，不接管任一外层格式，也不创建第二套 AST。

权威边界见[文档格式导入与导出](../architecture/editor/document-interchange.md)和[数学公式](../architecture/editor/math.md)。

## 必读

- [数学公式](../architecture/editor/math.md)
- [文档格式导入与导出](../architecture/editor/document-interchange.md)
- [047 Markdown 适配器](047-markdown-interchange.md)
- [049 TeX 文档适配器](049-tex-interchange.md)
- [验证与评审规范](../standards/validation-and-review.md)

## 范围与非目标

- 以 `MathInputKind` 为显式宿主种类，把受支持 TeX 数学片段解析为 `MathTree` / `Align`，并从规范模型产生规范 TeX 数学片段。
- 解析和序列化共用一套 Rust 领域 API，结构化错误能区分语法错误、超限、类型不匹配及语义/extra 不可表达，并尽可能带 MathTree 路径。
- checked serializer 验证树形和所有已知节点、Run/环境属性及 extra 数据；任何不能无损表达的已知语义或不透明扩展都返回明确损失，不得静默跳过。
- Markdown、完整 TeX 文档、公式输入和剪贴板负责各自的外层语法、分隔符、源位置和诊断；它们只调用共享数学转换 API，不相互依赖。
- 不实现 TeX 文档解析、文档级 IR、剪贴板 MIME/选区、Markdown 扩展、TeX 宏执行或其他外层分隔符扫描。

## 完成情况

`caret-math` 新增 `fragment` 模块并导出 `MathFragment`、`MathFragmentError`、`MathFragmentLoss`、`MathFragmentPath`、`MathFragmentPathStep`、`MathLossReason`：

- `MathFragment::parse(source, kind)` 复用现有 `parse`，保留解析器的结构化分类（语法+字节偏移、超长/过深/节点超限、编号命令、宿主种类不匹配），并强制模型变体与 Inline/Equation/Align 宿主一致。
- `MathFragment::to_tex()` 先按当前 schema 校验树形/属性/限制，再逐层拒绝 TeX 无法无损表达的元数据与超出解析器精度预算的尺寸，最后复用现有 `serialize_tree`/`serialize_align`；不引入第二 AST、第二序列化器或外层格式依赖。输出为不含外层定界符的规范 TeX 正文；解析器产生/编辑器维护且尺寸在预算内的标准模型是序列化不动点，尺寸超预算返回 `UnrepresentableDimension`，`from_math` 接受任意手工构造模型时不保证可再读取（契约见 `from_math` 文档与遗留边界）。
- 损失以模型路径 + 原因返回：任意层级的 `extra`（含 `SoftBreak.extra`）、`Opaque` 内容、TextFlow Run 的 `props`/`language`/`extra`、零 Run TextFlow、行内环境的主题模板引用/未知实例属性/缺少 `fontFamily`/非 `serif`、`sans-serif`、`monospace` 字体族，以及超出 TeX 精度预算或序列化算术溢出的空白/支柱尺寸（`dimension::format_dimension_checked` 全 checked 判定，避免检查阶段自身 panic/静默错误输出）。透明 Row/Style 包装、空根 flow、空 Group、空 Align/空 cell 按规范序列化归一化（有行为测试）；空操作数槽位沿用既有归一化、不在片段层单独断言；ID/源位置不进入片段身份；一次检查返回首个损失。
- 转换失败不修改输入模型（`to_tex` 取 `&self`，测试断言失败后模型不变）。047/049/公式源码输入/091 的实际接入按验收标准留给各自任务；本任务提供并通过 API、行为测试与文档。片段校验不替代文档级 InlineMath/Equation/Align 宿主规则。

架构文档已同步：[数学公式](../architecture/editor/math.md#共享片段转换)新增“共享片段转换”边界，[文档格式导入与导出](../architecture/editor/document-interchange.md)指向该 checked API 的损失规则。

## 前置条件与待决策

- 以 `caret-math` 现有 parser、MathTree/Align serializer 及验证逻辑为基线，确认公有/内部边界和当前静默丢失字段清单。
- 对每类 MathNode、MathText Run、InlineEnvironment、Align 行/cell、extra 和未知节点记录可序列化、拒绝或可忽略的理由；不得按“当前 serializer 输出了字符串”推断无损。
- 确定结构化诊断中源字节位置与 MathTree 路径的可用边界；位置无法提供时明确报告缺失，不伪造。

## 实施步骤

1. 审计 parser、serializer、模型校验与 extra 字段，对照数学架构支持表列出转换缺口。
2. 定义单一 parse/checked-serialize API 和结构化错误类型；保持现有 `parse`/`serialize_tree`/`serialize_align` 调用方行为不变，实际迁移由 047/049/公式源码输入和剪贴板在各自任务中接入该共享边界。
3. 为 Inline、Equation、Align 添加有效输入、边界、超限和不可表达字段行为测试；覆盖 parse → serialize → parse 的语义往返。
4. 更新数学与文档交换架构、任务依赖，并完成分域审查、修复及独立最终评审。

## 预计改动

`crates/caret-math` parser/serializer API 和测试，必要时数学模型文档及引用该 API 的适配器调用点。不新建 crate，不把外层格式、剪贴板策略或文档 AST 引入数学 crate。

## 验收标准

- [x] Inline、Equation、Align 均通过同一受类型约束的片段转换 API；错误明确区分语法、限制、宿主类型和无法表达的内容。
- [x] serializer 对不能安全还原的语义或 extra 返回结构化损失；不会静默删除已知属性、未知字段或数学节点（检查项见完成情况；透明包装归一化与错误类别有测试锁定）。
- [x] 支持子集的 parse → serialize → parse 保留规定的 MathTree/Align 语义；错误源位置与模型路径由解析错误和 `MathFragmentPath` 提供。
- [x] 统一 API 与当前 caret-math parser/serializer 一致，未引入第二实现、第二 AST 或外层格式依赖；047、049、公式源码输入和 091 的实际接入由各自任务验收，不作为本任务的前置验收条件。
- [x] 失败转换不修改输入模型或编辑会话；格式、lint、行为测试通过。
- [x] 分域评审（数学序列化域 + 集成/文档域）与未参与实现的只读最终评审通过；发现项（SoftBreak.extra、尺寸检查溢出/回退、记录提前宣称、文档口径等）已修复并复验。覆盖率门槛由 Nightly 覆盖流程验证（本机未跑覆盖率）。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-21 | macOS 27.2.1 arm64 / rustc 1.100.0；`cargo test --locked -p caret-math` | Inline/Equation/Align 往返、损失路径、解析错误分类、嵌套/精度限制、归一化、i128 极值不 panic | 通过：167 项（新增 18 项 fragment 与 3 项 checked formatter 测试） |
| 2026-11-21 | `cargo clippy --locked -p caret-math --all-targets -- -D warnings`；`cargo fmt --all -- --check` | 无 lint/格式问题 | 通过 |
| 2026-11-21 | 全量门禁：`cargo fmt --all -- --check`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo test --workspace --locked`、`RUSTDOCFLAGS=-D warnings cargo doc --no-deps -p caret-math` | 无回归 | 通过：workspace 1935 项 + native CTest 20/20；fmt/clippy/doc 通过 |
| 2026-11-21 | `cargo test --locked -p caret-doc -p caret-engine` | 现有解析/序列化消费方无回归 | 通过：90 / 890 项 |
| 2026-11-21 | 行为覆盖 | 有效输入、边界、超限、不可表达字段 | 覆盖：三类宿主与环境属性 roundtrip 不动点；语法/超长/过深/编号命令/宿主不匹配；extra（含 SoftBreak/嵌套环境 child）、Opaque、Run 元数据、零 Run TextFlow、环境模板/属性/缺字体/字体族、尺寸精度与 i128 极值、未约分超预算 mu 尺寸、空 cell；损失路径精确到 content/operand/environmentChild/textRun/row/cell；透明包装/空 Group/空根 flow/空 Align 归一化；常规 em/mu 尺寸正例；失败不修改模型 |
| 2026-11-21 | 两轮分域评审（数学序列化域、集成/文档域）+ 两轮未参与实现的最终评审 | 发现项闭环 | 完成：SoftBreak.extra、检查阶段尺寸 panic/静默溢出、mu 回退分叉、SPDX 署名、记录提前宣称、文档绝对化等均已修复复验 |
| — | PR CI 与覆盖率 | 通过 | PR 建立后补记；覆盖率由 Nightly 覆盖流程验证 |

## 风险与回退

旧 serializer 可能接受了无法完整往返的树。遇到没有安全输出的内容时宁可返回可见损失错误；需要完整外部保真的 Caret 内部复制由 091 的结构 MIME 保留 MathTree，不以 TeX 文本回退冒充无损。

已知接口边界（应在 047/049/091 接入时按需处理）：一次转换只返回首个损失，不支持汇总全部损失；`from_math` 假设输入是解析器/编辑器维护的规范模型，任意手工构造的合法 `MathNode.text` 组合不保证可再解析（检查器覆盖 schema、元数据与尺寸预算，不做通用语义 roundtrip 指纹）；空 Align/空 cell、透明 Row/Style 包装、空根 flow 按规范序列化归一化；片段合法不替代文档级 InlineMath/Equation/Align 宿主规则；非 `array` Matrix 的 `columns` 在序列化时按既有规范归一化；直接调用旧 `serialize_tree`/`serialize_align` 的路径仍使用未受 checked 保护的尺寸格式化，接入片段 API 后可控，旧调用方风险由各自任务确认；`MathFragment` 不携带源码投影，源码编辑仍需组合 `parse_projection`/sidecar。

## 决策与工作记录

- 2026-11-16：确认 `caret-math` 是 047/049 共用的数学片段转换器；通用文档适配器和剪贴板只负责各自的外层结构。
- 2026-11-16：从富剪贴板任务拆出 checked serializer/API，避免 047 因剪贴板实现而产生无关依赖。
- 2026-11-21：实现 checked 片段 API：`MathFragment` 复用现有 parser/serializer；损失检查覆盖 extra、Opaque、Run 元数据、零 Run TextFlow、环境模板/未知属性/不可表达字体族与尺寸精度，路径统一为 `MathFragmentPath`。选择“解析器+校验+显式元数据检查”而非通用 roundtrip 指纹比较，避免透明 Row/Style 归一化造成误报；行为测试覆盖三类宿主/环境属性往返、全部损失类别、归一化与失败不变更模型。架构文档同步“共享片段转换”边界。
- 2026-11-21：分域评审发现 `SoftBreak.extra` 与超精度尺寸会被静默写出/panic，已修复并补测试；评审同时指出记录中提前引用 PR CI 与最终评审，已改为待建/待完成，补齐环境信息、实现步骤口径与残留接口边界。
- 2026-11-21：最终评审发现检查阶段自身仍可因 i128 极值在未受保护的 `format_dimension` 中 panic/溢出；新增 `dimension::format_dimension_checked`（全 checked、18 位精度预算）供 checked 片段判定，Strut 深度取负也改为 checked，并补 i128 极值、正常 em/mu 正例、缺 `fontFamily` Font 环境、空 Group/空 Align/空 cell 等回归；文档的“解析器产生的标准模型可再读取”限定为“尺寸在精度预算内”。
- 2026-11-21：最终评审第二轮发现 checked formatter 的 mu 分支在候选不可回读时会 fallthrough 到 em 候选，导致检查通过但实际 serializer 写出超预算 mu；已改为早返回并补“checked Some ⇒ 与 format_dimension 一致且可回读”的不变式测试与未约分 hspace 拒绝用例。最终评审确认闭环：尺寸检查三项极值 + 未约分 mu 均返回 Loss 且不 panic，空 cell/空 Align 归一化测试成立，任务记录数字与最终工作树一致。

## 完成摘要

`caret-math` 提供统一的 `MathFragment` checked 转换边界：三类宿主共用同一 parse/checked-serialize API，解析错误保留结构化分类，序列化前按 schema 校验并对无法无损表达的 extra、Opaque 内容、Run 元数据和行内环境元数据返回带路径的结构化损失，不静默删除；可表达子集（含尺寸精度预算）parse → serialize → parse 保持语义且序列化是不动点。新增 18 项片段行为测试与 3 项 checked formatter 回归并同步数学与文档交换架构；格式、clippy、行为测试通过。047/049/公式源码输入/091 的实际接入按各自任务验收。
