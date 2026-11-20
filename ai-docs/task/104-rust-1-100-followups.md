# 104 — Rust 1.100 与质量清理的后续项

- 状态：planned
- 依赖：103
- 优先级：P2
- 创建 / 更新：2026-11-20 / 2026-11-20

## 目标与背景

[103](103-rust-1-100-upgrade.md) 合并后遗留若干因工具链约束、产品阶段或独立 issue 范围而未在本次执行的事项。本任务统一跟进，避免散落在 issue 与任务记录中；开始实施时可拆成独立小任务，但决策与验证记录继续引用本任务。

1. **Miri 固定 nightly 升级与 `str::trim_prefix` 回填**：`tests/src/miri.rs` 固定 `nightly-2026-09-15`，该 nightly 尚未稳定 `trim_prefix_suffix`；1.100 的 `str::trim_prefix` 不能用于 Miri 覆盖的 crate（如 `caret-math`），103 已回退 5 处替换。升级 Miri nightly、重跑 `cargo ub-check` 后，再评估回填这 5 处。
2. **依赖冷却期评估**：`.cargo/config.toml` 的 `[registry] global-min-publish-age` 在内测阶段不需要（103 未采用）。结束内测、面向外部发布前重新评估是否启用，并记录取值、与 `cargo deny`/`cargo audit` 的分工和绕过方式。
3. **#149 snapshot 必填字段**：`caret-app` 的 snapshot 每次输出 `mode`/`prefix`/`cursorUtf16`，但 C++ 解码仍容许缺省并保留「旧快照省略 prefix 字段时使用的兼容缺省值」注释。按「不做兼容」改为必填，并删除这条兼容注释。
4. **#145 数学符号旧常量**：`caret-math/src/symbols.rs` 的 `ACCENTS`/`LEGACY_*`/`accent_char` 只被测试使用；`caret-doc/src/model/math.rs` 的「旧或未验证节点…兼容默认值」注释与 `caret-math/src/serialize.rs` 的空样式兜底需要删除，或改写为当前 schema 的正式语义。
5. **Qt TSan 排除收敛**：Linux TSan 目前排除 `Qml.ShellLoad`、`Qml.MathTextDrag`、`Qml.Close*`、`App.HeadlessWindowLoads`，根因是未插桩 libQt6Core 的 QQmlThread 分配冲突。Qt 提供 TSan 插桩或上游给出结论后复查并移除排除；此前可评估给同类 QML 测试统一打 label、按 label 排除，减少逐项补表。

## 必读

- [103](103-rust-1-100-upgrade.md)、issue #152、#149、#145
- [验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)、[依赖获取](../standards/dependency-acquisition.md)
- [文档存储](../architecture/editor/document-model/document-storage.md)、[数学公式](../architecture/editor/math.md)

## 范围与非目标

范围内：上述 5 项，按决策成熟度逐项实施、验证与提交；不适用项写出结论和原因。

非目标：不借这些后续项引入新的编辑器功能；不提前为单项造抽象；不把 #149/#145 的兼容清理扩大为格式迁移。

## 前置条件与待决策

- Miri nightly 升级：确认目标 nightly 的 `miri` 组件可用，升级决策记录在基础设施任务（001/006）；升级后需重跑 `cargo ub-check`。
- 依赖冷却期：是否启用取决于项目是否结束内测；启用时需说明与 `cargo deny`/`cargo audit` 的分工。
- #145：需要维护者决定「删除旧符号/兼容注释」还是「改写为当前 schema 的正式语义」。
- Qt TSan：取决于 Qt 版本与工具链结论；未确认前不承诺移除排除。

## 实施步骤

1. 逐项确认前置决策，拆分执行顺序（必要时拆为独立小任务）。
2. 先做低风险清理（snapshot 必填字段、数学符号旧常量），每项补行为测试并同步文档。
3. Miri nightly 升级与 Qt TSan 作为工具链/上游依赖项单独验证。
4. 全量门禁、独立评审与 PR CI 通过后提交。

## 预计改动

`tests/src/miri.rs`、`.cargo/config.toml`、`crates/caret-app/src/document_view/snapshot.rs`、`native/viewmodels/document/layout_snapshot_codec.cpp`、`crates/caret-engine/build.rs`、`crates/caret-math/src/symbols.rs`、`crates/caret-doc/src/model/math.rs`、`crates/caret-math/src/serialize.rs`、`tests/src/sanitize.rs`，以及对应架构/规范文档。实际路径以实施时的代码为准。

## 验收标准

- [ ] 每项有明确落地或书面结论（采用 / 不采用及原因），不把未验证项写成完成。
- [ ] 代码类改动补用户可观察的行为测试，跑受影响门禁并同步文档；纯文档事项更新对应 issue/任务记录。
- [ ] Miri nightly 升级后 `cargo ub-check` 通过；`str::trim_prefix` 回填有独立验证，未回填时记录原因。
- [ ] 依赖冷却期在发布阶段有明确启用/不启用决定。
- [ ] Qt TSan 排除项在证据支持下收敛，或记录继续等待上游的具体条件。
- [ ] 分领域评审与未参与实现的只读 final review 完成，发现项已处理复验。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | 待执行 | — | 未执行 |

## 风险与回退

- Miri nightly 升级可能引入新的 UB 报告或工具行为变化；回退是恢复原固定日期并撤回依赖新 API 的回填。
- snapshot 与数学符号改动触及持久化/序列化边界，需测试与文档同步，避免把兼容清理变成静默数据变化。
- Qt TSan 结论依赖上游，不能以本机通过代替 sanitizer 矩阵；继续等待时保持现有排除并更新复查条件。

## 决策与工作记录

- 2026-11-20：创建任务，汇总 [103](103-rust-1-100-upgrade.md) 的未采用/非目标项与 issue #152 的「顺带清理（Low）」；本任务不改变 103 的完成结论。

## 完成摘要

未完成。
