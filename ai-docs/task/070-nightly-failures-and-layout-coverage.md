# 070 — Nightly 失败修复与布局测试补齐

- 状态：in-progress
- 依赖：006
- 优先级：P1
- 创建 / 更新：2026-11-08 / 2026-11-08

## 目标与背景

修复 [2026-11-07 Nightly diagnostics](https://github.com/VibeSTEM/caret/actions/runs/42376387135) 的可复现失败，并补足 inline object 布局测试对占位符字体差异的覆盖。该运行中 Rust/native 覆盖率门禁通过；Miri 因新增长文本命中测试加载系统字体而失败，Windows workspace 测试因 inline object advance 断言将替换字符的字体宽度纳入固定差值而失败。

## 必读

- [验证与评审](../standards/validation-and-review.md)
- [测试规范](../standards/testing.md)
- [CI 规范](../standards/ci.md)

## 范围与非目标

- 修正 Miri 对不能在解释器中运行的 `document_view::hit_test` 字体访问测试的精确排除，并保留其 workspace 行为测试。
- 将 inline object advance 回归断言改为对比同一段落中的零宽对象与定宽对象，验证用户可见的推进宽度与对象宽度一致。
- 增加相应回归验证并记录覆盖范围。
- 不调整 Rust/native 85% 覆盖率门槛或排除范围，不扩大平台矩阵。

## 前置条件与待决策

- 指定 Actions 运行日志已读取，失败项为 Ubuntu Miri 与 Windows `caret-layout` 单测。
- 通过可用的本地验证确认 Miri skip 选择及 inline object advance 语义。

## 实施步骤

1. 针对 Miri `dlopen` 错误跳过仅在 Miri 下不兼容的命中测试用例。
2. 增加 / 调整 inline object advance 行为断言，避免系统字体、连字或字距差异影响期望值。
3. 运行最小 Rust 测试与格式检查；可用时运行 Miri 或对应验证。
4. 完成领域检查与最终只读评审，更新验证记录。

## 预计改动

- `tests/src/miri.rs`
- `crates/caret-layout/src/tests.rs`
- 本任务文档和 `ai-docs/task-index.md`

## 验收标准

- [x] Miri 不再尝试从目标命中测试加载 Fontconfig；该行为仍由完整 workspace 测试验证。
- [x] advance 回归测试断言 inline object 占用的 30 pt 推进量，与平台字体塑形结果无关。
- [x] Rust、覆盖率门禁范围和阈值没有被缩减。
- [x] Rust 全局函数与行覆盖率达到 85%；覆盖率缺口用有行为意义的测试补齐。
- [x] 相关测试、评审及未执行验证均有事实记录。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-08 | `cargo test --locked -p caret-layout --lib inline_object_uses_parley_advance_and_expands_the_shared_line_box` | advance 回归用例通过 | 通过（1 passed） |
| 2026-11-08 | `cargo test --locked -p caret-ffi --lib ffi_crash_handler_reports_an_unusable_log_directory` | FFI 错误转发行为通过 | 通过（1 passed） |
| 2026-11-08 | `cargo fmt --all --check` | Rust 格式通过 | 通过 |
| 2026-11-08 | `cargo ub-check` | Miri 用例通过 | 通过（macOS arm64：caret-app 83 passed、1 ignored；DSL 4 组通过） |
| 2026-11-07 | 指定 Nightly run 的 Rust/native coverage jobs | Rust 与 native 门禁保持通过 | 两个覆盖率 job 均成功；本次未重新运行覆盖率报告 |
| 2026-11-08 | Nightly run 42399746940 Windows workspace tests | 跨平台确认 advance 修复 | 通过；同轮 CI 已验证该修复 |
| 2026-11-08 | Nightly run 42399746940 的 Rust coverage gate | Rust 函数与行覆盖率均不低于 85% | 失败：函数 3791/4462（84.96%），行 48951/54892（89.18%）；以 FFI 错误路径的行为测试补充可达覆盖 |
| 2026-11-08 | 最终 diff 上的 `cargo coverage` | Rust 函数与行覆盖率均不低于 85% | 通过：函数 3794/4462（85.03%），行 48959/54892（89.19%） |
| 2026-11-08 | gpt-6.1-terra/medium 领域检查与 gpt-6.1-sol/medium 最终只读评审 | 覆盖测试行为、FFI 失败原子性、LSan 抑制范围与文档证据 | 通过；抑制风险及 macOS 27.2 未复现原 Nightly 栈的限制均记录。 |
| 2026-11-08 | `gh run watch 42399746940` | 获取跨平台、Miri、coverage、sanitizer 和质量检查结果 | Windows/macOS/Ubuntu workspace tests、Miri、native coverage、quality、Windows/Ubuntu sanitizer 通过；Rust coverage 低于门槛；macOS sanitizer 的 Qml.ShellLoad 泄漏检查失败；唯一剩余 Rust CPU performance job 按用户要求取消 |
| — | 修复后远程 Nightly | 跨平台确认 coverage 和两项修复 | 待推送后复验 |

## 风险与回退

回归断言必须继续验证对象 advance 对段落总推进量的影响；不得仅检查 Parley 中存在 inline box。若 Miri 的模块名匹配不符，应调整过滤模式并核对列出的执行测试。

## 决策与工作记录

- 2026-11-08：创建任务；日志确认两项失败，覆盖率门禁通过。
- 2026-11-08：Miri 过滤精确到失败测试名；布局用例使用同文本零宽对象作为基线，避免字体塑形差异。Terra/medium 领域检查与 Sol/medium 最终只读评审均未留下未处理发现；最终评审未复验 Windows 或远程 Nightly。
- 2026-11-08：合并后 Nightly 验证 Windows 修复通过；coverage 报告为 84.96% 函数覆盖率，故重开任务并寻找有用户可观察行为的覆盖缺口。独立领域检查否决只断言私有 precedence 数字的测试；保留 FFI 错误传播测试并确保临时路径可编码且失败会清理。
- 2026-11-08：最终保留的 FFI 错误传播行为测试使本地 Rust 函数覆盖率达到 85.03%，行覆盖率为 89.19%；Terra/medium 领域检查与 gpt-6.1-sol/medium 最终只读评审完成，无遗留发现。等待合并及远程 Nightly 复验。
- 2026-11-08：macOS Nightly sanitizer 日志确认两笔泄漏均来自 LaunchServices `NotifyToken::RegisterDispatch` 异步系统回调；LSan 增加仅匹配该符号的规则，等待合并后的 Nightly 复验。

## 完成摘要

最终 diff 的本地 Rust coverage 已达标；修复后远程 Nightly 需在合并后复跑。macOS sanitizer 的 Qml.ShellLoad 原始 LaunchServices 泄漏修复待 Nightly 确认；Rust CPU performance job 已按用户要求取消。
