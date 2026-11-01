# 042 — M3：数学后端选型与验证

- 状态：done
- 依赖：无
- 优先级：P2
- 创建 / 更新：2026-10-29 / 2026-11-01

## 目标与背景

确定 Caret 公式的解析与 TeX 数学排版后端，提供可被编辑模型和布局接入的稳定树与独立数学帧 API。正文段落、页面快照、命中与光标的应用接入由 043/044 验收。总体验收由 [011 M3 公式](011-impl-math.md)管理；选型依据见[研究 010](../research/010-typst-math-layout.md)。

## 必读

- [公式与表格](../architecture/editor/math-and-table.md)、[文档模型](../architecture/editor/document-model.md)
- [研究 010：Typst 数学排版移植](../research/010-typst-math-layout.md)、[源码署名](../standards/source-attribution.md)
- [测试规范](../standards/testing.md)、[验证与评审](../standards/validation-and-review.md)

## 范围与非目标

后端是工作区内的 `caret-math` crate：

- 自写 LaTeX 数学子集解析器直接产出 `caret-doc` 的基础 `MathTree` / `Align` schema，不经过第三方 AST；序列化器把树写回 LaTeX。043 在此基础上完成编辑语义和会话校验。
- `layout/` 从 Typst v0.15.1（提交 `9dfd3a0`）移植 TeX 数学排版算法，度量全部取自 OpenType MATH 表。
- 字体读取用 `read-fonts =0.45.0`（首个提供 MATH 表的版本）与 `skrifa =0.44.0`；内置 STIX Two Math（OFL）。

不实现公式编辑命令、编辑会话状态、正文页面布局/命中/光标接入或 Qt 绘制，这些属于 043/044。042 包含解析与序列化所需的基础数学 schema、持久化 codec 和安全校验。

## 前置条件与待决策

无。依赖版本固定在根 `Cargo.toml`；`deny.toml` 只为 `skrifa` 传递引入的 `read-fonts 0.41` 设置重复版本豁免。

## 实施步骤

1. 对比现有 Rust/TeX 数学排版实现，确定移植来源与许可证兼容性。
2. 实现解析器、序列化器与符号表，固定深度、长度和节点数上限。
3. 移植排版模块并以 MATH 表度量产出帧：字形、实心矩形、基线与尺寸。
4. 提供驻留字体与按公式缓存的 `MathLayoutEngine`，输出独立数学帧，供 044 接入正文布局。
5. 建立 `cargo math-backend` 别名与 CI 任务，只跑后端相关 crate。

## 预计改动

`crates/caret-math`、根 `Cargo.toml`/`deny.toml`、`.cargo/config.toml`、`.github/workflows/ci.yml`、`NOTICE`。

## 验收标准

- [x] 移植来源、提交、许可证与修改记录写在每个移植文件头，并汇总到 `NOTICE`。
- [x] 解析器对超深、超长、节点过多的输入返回错误而不崩溃。
- [x] `parse(serialize(tree))` 与原树结构一致（节点 ID 与源范围除外）。
- [x] 排版输出含字形 ID、规则线、线段、填充/描边矩形与数学节点盒，由 044 映射到 Caret 页面字形快照。
- [x] `cargo math-backend` 不依赖 Qt/原生构建即可运行。
- [x] 已解析的数学 `.cdoc` 特性在完整布局接入前进入只读安全预览，避免应用接受无法显示的公式内容。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-01 | Linux：`cargo math-backend` | 解析、往返、排版与缓存测试通过 | 通过 |
| 2026-11-01 | Linux：`cargo deny check` | 许可证、公告与重复版本检查通过 | 通过 |
| 2026-11-01 | macOS：`cargo math-backend` | 解析、往返、排版、缓存与段落适配测试通过 | 通过 |
| 2026-11-01 | macOS：`cargo test --locked -p caret-doc` | 数学 schema、单位换算、限制、安全预览与拒绝无效数据通过 | 通过，47 项 |
| 2026-11-01 | macOS：`cargo test --locked -p caret-app math_feature_stays_read_only_until_formula_layout_is_supported` | 安全预览拒绝打开数学文件，现有编辑会话与保存快照不变 | 通过，1 项 |
| 2026-11-01 | macOS：`cargo test --locked -p caret-engine` | 使用统一文档单位 API 的正文布局和跨层编辑回归通过 | 通过，189 项 |
| 2026-11-01 | macOS：`cargo clippy --locked -p caret-doc -p caret-math -p caret-layout -p caret-engine -p caret-app --all-targets -- -D warnings`、`cargo fmt --all --check`、`git diff --check`、`cargo deny check` | 受影响 crate 的 lint、格式、diff 和许可证检查通过 | clippy、格式及 diff 检查通过；deny 通过，提示 5 条未使用的许可 allow 项 |

## 风险与回退

MATH 表度量依赖字体质量；首版只支持内置 STIX Two Math。移植代码与 Typst 后续版本的差异需人工对照同步。桌面 Runtime 安装器目前只收集根 `LICENSE` 和 Cargo 依赖许可证；044 将字体嵌入应用时，必须把根 `NOTICE` 与 STIX OFL 一并纳入安装包并验证。缓存以 64 位哈希作为键且没有内容复核，存在碰撞误命中风险；4096 条整体清空策略没有性能基线，收益与退化待以代表性文档测量。

## 最终评审

2026-11-01 由未参与实现的 Sol/medium subagent 只读检查 `origin/main` 至本分支完整差异，覆盖代码、任务与架构文档、许可证及字体资源。发现两处文档把 043/044 目标写成当前能力，已修正；未发现代码阻断项。缓存风险和 Runtime 许可证打包要求保留为 044 待办。评审后 `git diff --check origin/main` 通过。

## 决策与工作记录

- 2026-11-01：采用自写解析器加 Typst 移植排版；对比与理由见研究 010。

## 完成摘要

`caret-math` 提供解析、序列化与 MATH 表排版，`cargo math-backend` 与 CI 任务覆盖后端测试。
