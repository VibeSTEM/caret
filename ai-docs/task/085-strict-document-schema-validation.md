# 085 — 已知节点严格字段与消费者校验

- 状态：draft
- 依赖：025、032、084
- 优先级：P1
- 创建 / 更新：2026-11-10 / 2026-11-10

## 目标与背景

已知 DTO 字段、数学节点参数、Run/Paragraph/环境属性和布局消费者必须采用同一套当前 schema 规则。Issue #129 建议已知节点拒绝未知字段并删除 `extra`/`flatten`，只把未知节点作为 Opaque；但当前 `.cdoc` 权威文档明确要求同版本未知可选字段往返保留。两项契约冲突，Issue 中的“已拍板”不足以替代权威文档决策，因此本任务在该兼容策略决议前保持 draft。

## 必读

- [`.cdoc` 存储格式](../architecture/editor/document-model/document-storage.md)
- [文档树与结构](../architecture/editor/document-model/document-structure.md)
- [数学公式架构](../architecture/editor/math.md)
- [文字格式语义](../architecture/editor/text-formatting.md)
- [任务 032：未知节点安全预览](032-unknown-node-preview.md)、[任务 084：内容形状与容器边界](084-document-content-shapes-and-boundaries.md)
- [验证与评审标准](../standards/validation-and-review.md)

## 范围与非目标

- 统一已知数学 kind 的操作数个数、槽主类型和属性验证；当前 schema 内结构损坏必须报错，未来未知 kind 仅按 032 的可定位 Opaque/SafePreview 契约处理。
- 对 Run、Paragraph、InlineEnvironment、MathNode、主题/模板及其他持久化 `props` 定义允许键、值类型和值域，并保证 validator、能力推导、序列化和实际布局/编辑消费者一致。
- 复核 `Paragraph.markStyle` 与 `ExplicitBreak.kind`：只接受布局真正实现的值；若应删字段，先证明没有 DTO、测试夹具或外部构造消费者。
- 明确已知节点未知字段政策；若采用拒绝策略，先修改 `.cdoc` 权威兼容契约，并定义显式扩展/未知节点的数据保留边界，再调整 DTO 与 Rust 调用方。
- 不删除模型字段候选（由 082 证据审计）；不把 `.cdoc` version、MathTree schemaVersion、theme formatVersion 与 `requiredFeatures` 合并为一层。若接受的不兼容变更要求升级 `.cdoc` 主版本，按其独立格式语义处理。
- 不把紧凑 JSON 或字节级输出一致当作文档语义；序列化体积优化须有独立用户价值和测量依据。

## 前置条件与待决策

- 维护者需先决定是否撤销“同版本未知可选字段保留”契约，或设计显式扩展命名空间以兼顾严格已知字段和可选扩展；在权威文档与任务验收一致前不得实现删除 `extra` 的批量变更。
- 确认 `.cdoc` 是否已有发布/外部生成消费者；枚举 MathTree、主题版本和能力 ID 的独立兼容责任。
- 084 明确各容器及数学空槽的有效形状，032 明确未知 kind 的 SafePreview 边界。

## 实施步骤

1. 绘制每个当前 schema 节点的字段、类型、值域、布局/编辑消费者和扩展载体矩阵；统一四处数学槽主类型判定。
2. 解决未知字段兼容策略并更新唯一权威文档；明确已知节点字段、显式扩展数据、未知节点及未知必需能力的边界。
3. 实现共享模型验证入口，使打开、编辑事务、feature 推导与布局消费同一有效规则；畸形已知结构不得退化为未知节点预览。
4. 按验证结果更新/删除 DTO 字段或 `extra`，同步审计 Rust 公共构造器、测试 fixture 和跨 crate 调用方；有 source breaking change 时提供明确迁移和编译验证。
5. 为已知/未知字段、错误类型、非法值、未知 kind、SafePreview、序列化往返及布局拒绝补固定回归用例。
6. 更新格式文档与相关 task；完成 Terra/medium 分域评审，修复后由未参与实现的 Sol/medium 独立评审最终完整 diff。

## 预计改动

可能涉及 `caret-doc` 的 DTO/codec/validation/features/storage_format、`caret-engine` 的布局输入校验、`caret-app` 打开/保存边界、外部 Rust 构造调用方、测试夹具和 `.cdoc` 权威文档。实际目录以消费者调查为准。

## 验收标准

- [ ] 未知字段兼容政策在 `.cdoc` 权威文档、模型结构和测试中一致；未决前没有破坏性代码变更。
- [ ] 当前 schema 已知数学节点缺字段、参数个数错误、槽位类型错误或属性值非法时返回稳定错误；未来未知 kind 不被误当成已知格式损坏，也不被错误地编辑。
- [ ] 数学槽主类型判定仅一份权威实现，codec、validator、feature 推导与能力检查一致。
- [ ] 每个允许的 `props` 键具有类型和值域；Paragraph、Run、环境、MathNode、markStyle 与 ExplicitBreak 的校验和布局/编辑能力一致。
- [ ] DTO/额外字段变化完成所有公开 API 和跨 crate 调用方审计；所有调用方编译通过，破坏性迁移已明确。
- [ ] `.cdoc` 主版本、MathTree schema、theme formatVersion 和 requiredFeatures 各自保持清楚语义；无错误的单版本替代。
- [ ] 错误文件打开不部分编辑或覆盖源文件；SafePreview、存储往返和失败原子性测试通过。
- [ ] 架构文档只保留最终契约，验证与独立评审按仓库标准通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `caret-doc` schema/codec/features 测试 | 所有入口对字段和数学结构作同一判断 | 未执行 |
| — | `caret-app` / `caret-engine` 打开、布局与保存测试 | 格式错误拒绝，未知数据按明示策略处理 | 未执行 |
| — | 外部 DTO 构造调用方编译检查 | 无遗漏 source/API 消费者 | 未执行 |
| — | `cargo format --check`、`cargo lint --check`、相关 crate 测试 | 验证与跨层消费者一致 | 未执行 |
| — | Terra/medium 与 Sol/medium 独立评审 | 兼容边界与 validator 共享规则正确 | 未执行 |

## 风险与回退

拒绝未知字段会改变同版本前向兼容，并可能破坏外部 Rust 调用方；删除扩展载体也会改变编辑后往返语义。未经维护者决策与调用方审计不得实施。若统一校验会误拒绝既有受支持内容，应保留源文件、报告稳定格式错误并收窄规则，不做部分加载或静默降级。

## 决策与工作记录

- 2026-11-10：依据 Issue #129 建立草案。严格拒绝未知字段与当前存储权威的未知可选字段保留规则冲突，需先完成兼容决策；拒绝将所有版本层折叠为单一版本。

## 完成摘要

未完成。
