# 096 — caret-engine 布局模块生产/查询拆分

- 状态：done
- 依赖：093
- 优先级：P1
- 创建 / 更新：2026-11-17 / 2026-11-18

## 目标与背景

`crates/caret-engine/src/layout.rs` 5191 行同时承担“生产布局快照”（`DocumentLayoutService`、`collect_blocks`、塑形与投影）与“页面查询/几何”（30 个 pub 查询：三类 hit test、caret/selection/composition 几何、公式文本移动），6 个月内 18 次提交，单次修复（6641bf43）在本文件改动 695 行；`page_glyphs.rs` 还经 `pub(crate)` 直接读取快照缓存。`layout_with_reuse` 已在本轮收敛为单次派生，但函数仍 389 行混合校验/塑形复用/分页装配/索引构建。

完成后：生产侧与查询侧以命名中间结构（PageGeometries/MathProjectionIndex）交接，`layout.rs` 只保留类型与重导出；查询方法按 hit/geometry/math_text 分组到子模块。

## 必读

[仓库文件结构](../standards/repository-hygiene.md#模块文件结构)、[分层](../standards/layering/rust.md)、[架构：页面排版](../architecture/editor/page-layout.md)、[验证与评审](../standards/validation-and-review.md)。

## 范围与非目标

范围内：模块文件拆分与可见性收敛、`layout_with_reuse` 阶段化拆函数。非目标：改排版算法、性能优化（除非有基线与测量）。

## 前置条件与待决策

`layout/tests.rs` 4652 行、`page_glyphs/tests.rs` 1407 行覆盖入口；拆分前记录基线测试数与耗时。确认 `pub(crate)` 缓存字段的消费者清单（`page_glyphs.rs`、`caret-app`）。

## 实施步骤

1. 列出 `impl DocumentLayoutSnapshot` 与生产段的全部方法，按职责分组。
2. 先抽 `shape_paragraphs`、`paginate_and_index`、`LaidOutParagraph::can_reuse` 三个具名阶段。
3. 拆 `layout/produce.rs`、`layout/query.rs`、`layout/hit.rs`、`layout/geometry.rs`、`layout/math_text.rs` 与共享查询项模块，入口只保留类型与错误 trait 实现。
4. 每步跑 `cargo test -p caret-engine -p caret-app`，并检查 `caret-layout`/`caret-app` 编译；完成后全量门禁与独立评审。

## 预计改动

`crates/caret-engine/src/layout.rs` 及 `layout/` 新子模块；`page_glyphs.rs` 的读取改为命名查询；可能少量 `caret-app` 调用点改名（不改行为）。

## 验收标准

- [x] `layout.rs` 仅保留类型、错误 trait 实现与模块导出，生产与查询分文件。
- [x] 三类 hit test 与几何查询按 hit/geometry/math_text 分组；缓存字段私有化，消费者使用具名查询。
- [x] 全量质量门禁、布局/字形回归测试与独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-18（基线） | `cargo test --locked -p caret-engine -p caret-app` | 记录拆分前测试数与耗时 | app 336 项、engine 876 项均通过（共 1,212）；初次编译 34.22 秒，engine 用例报告 4.08 秒；app 单独耗时未保留在首次命令输出中。 |
| 2026-11-18（完成后） | `/usr/bin/time -p cargo test --locked -p caret-engine -p caret-app` | 两个直接消费者全绿 | 1,212 项通过；engine 用例 3.84 秒；整条命令实耗 50.30 秒（含编译）。 |
| 2026-11-18（最终修复后） | `cargo quality`（提升权限运行） | 完整格式、lint、依赖审计、workspace/native/QML 测试通过 | 全部通过；依赖审计提示 5 项既有 license allowlist 未遇到，native/QML CTest 20/20 通过。 |

## 风险与回退

布局几何对顺序/浮点舍入敏感；只搬移不改表达式。每步独立 commit 便于回退；涉及 `caret-app` API 改名时同步更新调用点。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已完成该文件内 3 处重复收敛（基线映射、命中规则、单次派生），结构拆分留本任务。
- 2026-11-18：生产代码移至 `layout/produce.rs`；查询实现分至 `layout/query.rs`、`layout/hit.rs`、`layout/geometry.rs`、`layout/math_text.rs`；共用路径与轻量几何工具放入 `layout/shared.rs`。`shape_paragraphs`、`PageGeometries::paginate_and_index`、`MathProjectionIndex` 和 `LaidOutParagraph::can_reuse` 明确生产阶段。分页阶段在原输入遍历中借用并索引公式环境 ID，避免额外扫描。
- 2026-11-18：`page_glyphs` 改为使用快照的公式数据查询方法；`inline_math_ids`、公式文本 sidecar 与投影索引字段收为私有。分页索引借用环境 ID 切片，避免新增深克隆。
- 2026-11-18：Terra/medium 布局架构与消费者领域复核通过；指出的缓存直读、查询辅助代码归属及环境 ID 克隆均已修复。Sol/medium 对最终完整 diff 只读复核通过，确认无行为/API/规则阻断，注释与单次输入遍历修复已核对。最终 `cargo quality` 全部通过；依赖审计有 5 条既有未命中 license allowlist 警告，CTest 20/20 通过。

## 完成摘要

布局生产、快照查询和几何/公式文本查询已按职责拆分。`layout.rs` 仅保留核心数据类型、错误 trait 实现和模块导出；公开布局查询行为保持不变。基线与拆分后 caret-engine/caret-app 测试均为 1,212 项通过。未做性能优化，也未声称性能提升。
