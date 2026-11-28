# 084 — 行内与数学内容形状及容器边界

- 状态：implemented
- 依赖：025、032、081、082
- 优先级：P1
- 创建 / 更新：2026-11-10 / 2026-11-28

## 目标与背景

文档树可能以多种可编辑形状表达语义相同的内容，例如相邻 Text、空 Run、兼容格式包装的相邻或嵌套形态，以及空 MathFlow 与 Placeholder。若这些形状有不同 caret stop、输入/删除目标或空容器行为，三态导航就会随文件历史改变按键次数。任务需要明确允许的树形和容器边界，并保证等价内容具有一致的可达停靠行为；不要求不同合法编辑历史产生逐字段相同的持久化树。

本任务只处理内容树形状等价性、空公式槽及宿主容器边界；已知 schema 字段和属性值校验由 085 处理。不得向已完成的 021 追加待办。

## 必读

- [文档树与结构](../architecture/editor/document-model/document-structure.md)
- [数学公式架构](../architecture/editor/math.md)
- [文档位置](../architecture/editor/document-model/document-position.md)
- [文字格式语义](../architecture/editor/text-formatting.md)
- [任务 025：字符与段落格式](025-text-formatting.md)、[任务 032：未知节点安全预览](032-unknown-node-preview.md)
- [验证与评审标准](../standards/validation-and-review.md)

## 范围与非目标

- 定义正文 TextFlow、Run、InlineEnvironment 及 MathFlow 的合法树形；区分可无损合并的相邻/嵌套包装和必须保留的空内容、ID、属性、未知扩展与宿主边界。对每类问题选择安全的规范化或统一停靠仲裁，不强制树形完全一致。
- 针对数学根流、固定操作数槽与 Placeholder 分别判断是否等价；不把“视觉为空”直接推导成可删除或可合并。
- 为文档正文、Align 行/cell、Section/Custom 子项等容器各自定义允许的最小数量及空态编辑位置；不套用一个全局非空规则。
- 明确打开、编辑、撤销/重做和保存阶段何时校验、何时规范化、保留或拒绝；规范化必须有确定的 ID 归属及位置映射。
- 不实现通用字段白名单或未知字段策略（085）；不清理无关 DTO 字段（082）；不压缩 JSON 字节输出或改动独立格式版本。

## 前置条件与待决策

- 025 的格式树与下划线编辑契约、032 的未知数据预览边界、081/082 的宿主三态及文档位置契约可供复用。
- 先枚举当前编辑器可生成的形状，再区分编辑器生成、可安全读取但非规范、和结构损坏三类；不得仅因探针 JSON 可以反序列化就认定其可编辑。
- 文字格式文档允许不同命令历史留下不同但有效的树形。兼容包装只有在 kind、definition、props、扩展元数据、稳定 ID、选区映射和撤销语义都有确定处理时才可合并；否则保持原树并统一运行时停靠行为。
- 数学空流、显式 Placeholder 和可选空操作数的关系须在数学架构文档中作最终定义；保留 Math 文本岛和宿主空流的既定语义。

## 实施步骤

1. 为正文/数学内容流及每类容器建立“编辑器可生成 / 可接受但需统一交互 / 拒绝”的形状表和复现夹具。
2. 对相邻 Text、空 Run、同类格式包装、空 MathFlow 与 Placeholder 逐项定义语义等价条件、规范化或运行时仲裁策略及 ID/元数据/位置映射。
3. 按容器分别确定空态、最小子项数、caret 内部位置和创建/删除规则；验证空 Section、空 Equation/Align 与空 cell 不会失去可达编辑状态。
4. 只在有损风险可排除时实现事务内规范化；否则对合法异形复用统一停靠/编辑规则，对损坏结构明确拒绝，不在打开时静默丢弃内容、ID 或未知数据。
5. 更新权威模型/数学文档和本 task；补充从 JSON 打开、编辑、序列化往返、caret 移动、删除及 Undo/Redo 的行为测试。
6. 委派 Terra/medium 内容模型评审；修复后由未参与实现的 Sol/medium 对最终 diff 独立只读评审并复验。

## 预计改动

可能涉及 `caret-doc` 的正文/数学节点校验与转换器、`caret-engine` 的格式树事务/位置映射、`caret-math` 的公式操作数验证，及对应架构文档和固定 JSON 夹具。只按实证改动消费者，不新建通用 crate。

## 验收标准

- [x] [文档树](../architecture/editor/document-model/document-structure.md#内容形状与空容器)定义 TextFlow、Run、InlineEnvironment 和 MathFlow 的合法形状与等价判定。
- [x] 打开和保存保留合法树及身份。局部编辑按既定归并规则处理文本，回归比较完整文档、方向与 Undo/Redo 状态。
- [x] 数学根流、必需槽、可选槽及 Placeholder 按各自空态契约编辑。两种可入的空显示公式根均保留身份和历史。
- [x] Body、Align rows/cells、Section/Custom children 和 Table cell blocks 的最小数量与编辑入口逐类记录。缺少必要编辑落点时有明确错误。
- [x] 逻辑字素的导航和编辑结果与 Run 或 TextFlow 拆分无关。带语义身份的包装保留单独的交互边界。
- [x] 持久化、编辑、校验失败与撤销重做由固定夹具和文档模型回归覆盖。
- [x] 权威架构文档只保留最终规则；验证与独立评审按仓库标准通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-28 | `caret-doc` 的 `.cdoc` 内容形状回归 | 合法形状和身份往返 | 通过。覆盖相邻 TextFlow、默认与带语言的空 Run、空环境、空 MathFlow、显式 Placeholder 和缺少行或 cell 的 Align。 |
| 2026-11-28 | `caret-engine` 字素、格式和数学宿主回归 | 等价文本不增加停靠，输入和历史保留身份 | 通过。正文矩阵测试跨 Run 与跨 TextFlow 的组合附标、中插继承 underline、反向范围及完整文档和选区 Undo/Redo。空 Equation/Align cell 的根流和显式 Placeholder 可进入并输入，根 ID 保留。 |
| 2026-11-28 | `cargo format --check`、`cargo lint --check`、`cargo test --locked --offline --workspace` | 代码、分层和跨层消费者一致 | 通过。完整 lint 9/9，workspace 2126 项通过、1 项忽略，原生 CTest 20/20。 |
| 2026-11-28 | Terra/medium 与 Sol/medium 独立评审 | 形状、身份与空态规则没有遗漏 | 通过。复核真实 `.cdoc` 形状往返、字素停靠及空公式根进入和历史恢复。 |

## 风险与回退

过度规范化可能重写稳定 ID、改变格式继承或破坏未知数据；过严拒绝可能损伤当前文档可编辑性。若不能证明等价、身份归属和位置映射，则回退到保留原树并拒绝该次不安全编辑，不做静默修复。

## 决策与工作记录

- 2026-11-10：从 Issue #129 的规范形、空公式槽和空容器发现拆分；拒绝把视觉为空直接当成可合并，也不向已完成的 021 追加工作。

- 2026-11-28：默认空 Run 原先会切断前后同属性范围，导致逻辑光标在同一位置重复停靠。属性范围收集现在跳过完全默认的空 Run，保留持久化形状和带元数据的运行项。容器最小值、存储空态与编辑入口分别记录在[文档树](../architecture/editor/document-model/document-structure.md#内容形状与空容器)。

## 完成摘要

已记录内容形状与容器的存储、编辑边界。不同合法文本形状使用同一字素导航，空公式流和显式占位符分别保持身份及历史。最终验证与本轮外部合入状态见 [106](106-table-shared-host-interaction.md#brooks-health-后续优化)。
