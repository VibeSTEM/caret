# 100 — 测试确定性与诊断基础设施

- 状态：done
- 依赖：093
- 优先级：P2
- 创建 / 更新：2026-11-17 / 2026-11-18

## 目标与背景

Brooks sweep 在测试维度留下的残余：
- `crates/caret-engine/src/layout/tests.rs` 有 49 个系统字体目录调用。48 个测试调用点和 1 个共享布局 helper 都经过布局服务计算正文行框；空白、ASCII 和纯数学文档也依赖正文字体度量。FontCatalog 当前只提供系统目录构造入口；仓库只有 JetBrains Mono 与 STIX Two Math 两种内嵌字体，分别承担代码和数学角色，不能代表正文默认字体。仅使用内嵌字体的试跑中，空白页命中及空白/中文正文行框用例返回 `layout.inconsistent_parley_output`；中英/CJK 混排回退还需要中日韩字形，因此显式声明系统目录依赖；
- `crates/caret-engine/src/editing/session/math_edit/tests.rs` 原有 14,991 行、309 个测试无模块分区；
- 数学编辑测试中有 166 个忽略错误值的 `map_err`，其中 163 个把错误替换为 `EditError::InvalidPosition`，另外 3 个也丢弃错误文本；另有 441 个 `ok_or(EditError::InvalidPosition)`，它们是在表达缺失的 `Option`，并非被替换的底层错误；
- `LayoutViewModelTestProbe`（friend struct）用于注入私有协议入口并检查 timer 是否激活并停止 timer，以免测试期间实际触发自动保存；`paragraphTextBoundaries` 在当前仓库中已不存在；
- 恢复安装失败（`installSnapshot`）路径不刷新恢复列表（评审发现的既有缺口）。

完成后：布局测试对系统字体依赖给出明确范围和原因；数学编辑测试按职责分区（`mod`/子文件）；被映射的底层错误文本得以保留；不可由公开 API 替代的测试探针有明确理由。

## 必读

[测试规范](../standards/testing.md)、[Rust 断言风格](../standards/rust.md)、[验证与评审](../standards/validation-and-review.md)。

## 范围与非目标

范围内：测试夹具与结构；不改生产行为（恢复刷新缺口需单独确认是否属既有契约）。非目标：提高覆盖率数字；不为覆盖率增加无行为意义的测试。

## 前置条件与待决策

核对仓库字体资源和 `FontCatalog` 构造入口后，确认系统字体是正文与 CJK 布局几何测试的显式依赖；逐项核对测试探针后，保留仅供注入私有协议载荷、检查 timer 激活状态并停止 timer 的窄包装。

## 实施步骤

1. 将 48 个布局测试调用点和 1 个共享布局 helper 的系统字体目录获取集中到明确命名的测试 helper，并在模块说明 `FontCatalog` 没有独立正文字体夹具入口、内嵌代码/数学字体不能代表默认正文度量的原因。
2. `math_edit/tests.rs` 按 `mod` 分区（源码提交、树操作、选择/光标、符号输入、IME），用 `#[path]` 或子文件组织，保持测试名与数量不变。
3. 测试错误诊断：将 166 个丢弃错误的映射改为保留原始错误文本；`ok_or` 仅用于 `Option` 缺失状态，保持不变。
4. 探针与其消费者逐项核对；私有协议注入和 timer 激活状态检查/停止无公开等价 API，保留窄包装并注明理由。
5. 全量门禁 + 独立评审。

## 预计改动

`crates/caret-engine/src/layout/tests.rs`、`crates/caret-engine/src/editing/session/math_edit/tests.rs`（拆分为子文件）、`tests/cpp/shell/shell_document_interaction_test.cpp`。本次未改生产源码；探针 friend 声明和既有生产行为保持不变。

## 验收标准

- [x] 系统字体依赖通过具名 helper 显式表达；说明当前 `FontCatalog` 只提供系统目录入口、空白页命中与空白/中文正文行框的内嵌字体试跑失败，并记录内嵌字体不提供默认正文角色，也不覆盖中日韩回退字形。
- [x] `math_edit/tests.rs` 按职责分为源码编辑、树操作、选择/光标、符号输入、IME 五个子模块；测试函数名、309 个测试和断言不变。
- [x] 166 个错误映射保留底层诊断文本；缺失 Option 的 441 个 `ok_or` 保持原语义。
- [x] 全量门禁、覆盖率门槛核对与独立评审通过。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-18 | `cargo test --locked -p caret-engine layout::tests` | 布局定向测试全绿 | 66 passed |
| 2026-11-18 | `cargo test --locked -p caret-engine editing::session::math_edit::tests` | 数学编辑测试全绿 | 309 passed；原测试函数名和 1826 个断言宏调用数保持一致 |
| 2026-11-18 | `cargo test --workspace --locked` | 全 workspace 测试通过 | 通过；原生 CTest 20/20 |
| 2026-11-18 | `cargo clippy --locked --workspace --all-targets -- -D warnings` | 无警告 | 通过 |
| 2026-11-18 | `cargo format --check` / `cargo lint --check` | 跨语言格式、静态检查通过 | 均通过 |
| 2026-11-18 | `cargo quality` | 聚合质量门禁通过 | 通过；cargo-deny 有 5 条现有未命中许可证白名单 warning，advisories/bans/licenses/sources 均 ok |
| 2026-11-18 | `cargo coverage` | Rust workspace 函数 / 行覆盖率均 ≥85% | 通过：86.64% / 89.88%；报告在 `target/rust-coverage/`（caret-app 与 caret-ffi 的单 crate 函数覆盖率低于 85%，不影响全局门槛） |
| 2026-11-18 | `cargo coverage native` | native 产品源码函数 / 行覆盖率均 ≥85% | 通过：90.80% / 86.06%；原生 CTest 20/20 |
| 2026-11-18 | 独立 final review、`git diff HEAD --check` | 覆盖全部变更，问题已修复 | Sol/medium final review 通过；10 个变更文件，无阻塞项；diff check 通过 |

## 风险与回退

布局测试仍显式使用系统字体目录；未来若引入正文角色的内嵌字体夹具，需逐项对比几何断言。数学测试拆分仅搬移测试代码；验证前后函数名、309 个测试及 1,826 个断言宏调用完全一致。

## 决策与工作记录

- 2026-11-18：Sol/medium final review 检查 `eab0eb0b` 至最终工作树的全部 10 个变更文件；确认测试函数名、断言、Option 映射与诊断保留正确，无阻塞问题。评审提出的注释范围和任务记录细节已修复。
- 2026-11-17：Brooks sweep 已完成 eager 测试拆分与重复测试合并，本任务承接剩余测试基础设施。
- 2026-11-18：验证仅使用内嵌字体目录的代表性尝试：macOS 上执行 `cargo test --locked -p caret-engine layout::tests::empty_and_nonempty_paragraphs_share_the_same_line_grid -- --exact --nocapture`，返回 `layout.inconsistent_parley_output`；完整布局组同样失败。试验性构造器已撤销，现有布局测试通过具名 helper 明确声明系统目录依赖。

## 完成摘要

状态：done。测试拆分、错误诊断保留、系统字体依赖说明、探针理由和验证记录均已完成。未改生产源码。
