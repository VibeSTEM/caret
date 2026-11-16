# 096 — caret-engine 布局模块生产/查询拆分

- 状态：planned
- 依赖：093
- 优先级：P1
- 创建 / 更新：2026-11-17 / 2026-11-17

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
3. 拆 `layout/produce.rs`、`layout/query.rs`、`layout/geometry.rs`、`layout/math_text.rs`，入口只保留类型与重导出。
4. 每步跑 `cargo test -p caret-engine -p caret-app`，并检查 `caret-layout`/`caret-app` 编译；完成后全量门禁与独立评审。

## 预计改动

`crates/caret-engine/src/layout.rs` 及 `layout/` 新子模块；`page_glyphs.rs` 的读取改为命名查询；可能少量 `caret-app` 调用点改名（不改行为）。

## 验收标准

- [ ] `layout.rs` 仅保留类型/重导出/公共入口，生产与查询分文件。
- [ ] 三类 hit test 与几何查询各归一位；无 `pub(crate)` 缓存直读。
- [ ] 全量门禁与 18 次/6 月的热点回归测试通过；独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | `cargo test --locked -p caret-engine -p caret-app` | 全绿 | 未执行 |

## 风险与回退

布局几何对顺序/浮点舍入敏感；只搬移不改表达式。每步独立 commit 便于回退；涉及 `caret-app` API 改名时同步更新调用点。

## 决策与工作记录

- 2026-11-17：Brooks sweep 已完成该文件内 3 处重复收敛（基线映射、命中规则、单次派生），结构拆分留本任务。

## 完成摘要

未完成。
