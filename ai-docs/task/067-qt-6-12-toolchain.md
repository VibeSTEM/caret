# 067 — Qt 6.12 托管工具链升级

- 状态：in-progress
- 依赖：004、006
- 优先级：P1
- 创建 / 更新：2026-11-06 / 2026-11-08

## 目标与背景

将仓库托管 Qt 从 6.11.2 升级到当前可用的 6.12.0，覆盖 macOS、Windows、Linux 的官方预编译归档及 Linux ICU。Qt 官方将 6.12 定为 LTS；按 2026-11-06 可用发布版锁定 6.12.0，不预用尚未发布的维护版。同时审查 6.12 新能力与 Caret 画布的适配度，只有能解决现有性能问题并通过测量的改动才落地。

## 必读

- [依赖获取](../standards/dependency-acquisition.md)、[技术基线](../standards/baseline.md)
- [验证与评审](../standards/validation-and-review.md)、[测试规范](../standards/testing.md)
- [Qt 6.12 发布公告](https://www.qt.io/zh-cn/blog/qt-6.12-released)
- [Qt 6.12 新功能](https://doc.qt.io/qt-6/whatsnew612.html)、[Qt Canvas Painter](https://doc.qt.io/qt-6.12/qtcanvaspainter-index.html)、[Qt Quick 性能建议](https://doc.qt.io/qt-6.12/qtquick-performance.html)

## 范围与非目标

更新托管归档、Qt 查找最低版本、技术基线、受影响的工具版本注释及 sanitizer 例外说明；审查 Qt Canvas Painter、QML Canvas2D、QML 热重载及 Qt Quick 6.12 对画布的相关改进；验证供给、native/QML 构建和行为测试。若升级暴露兼容问题，仅修复为通过 6.12 所必需的差异。

不因发布公告的新颖性直接替换现有 QSG 绘制后端；不迁移安装器或系统 Qt，不提前采用 6.12.1。

## 前置条件与待决策

Qt 在线仓库的 `Updates.xml` 提供官方组件版本、归档名和 SHA1；仓库供给器要求固定 SHA256。每个平台、每个使用归档都必须在登记前取得并复核 SHA256，不能把 SHA1 转写或猜作 SHA256。

## 实施步骤

1. 核对 6.12.0 三平台官方归档、ICU、macOS 部署兼容性和组件清单。
2. 更新供给脚本、最低查找版本、技术基线和与版本相关的维护说明。
3. 用托管 6.12.0 验证 CMake 配置、构建、格式 / lint 和 native/QML 行为。
4. 由独立代理完成分领域检查及最终完整 diff 只读评审，修复并复验。

## 预计改动

- `native/cmake/qt-provision.cmake`
- `native/CMakeLists.txt`
- `crates/caret-build/src/qt.rs`
- `ai-docs/standards/baseline.md`
- 受升级影响的 sanitizer 注释 / 夹具及验证记录

## 验收标准

- [ ] macOS、Windows、Linux 使用官方 Qt 6.12.0 归档；各归档 SHA256 来源可复核。
- [ ] Linux ICU 与 Qt 6.12.0 配套，并由托管供给脚本校验。
- [ ] 三平台 CMake 要求和技术基线一致；无残留的当前版本锁 6.11.2。
- [x] macOS 构建拒绝低于 14.4 的 `MACOSX_DEPLOYMENT_TARGET` 覆盖。
- [x] 托管供给、应用构建、相关 native/QML 检查通过；未运行平台明确记录。
- [ ] sanitizer 规则经 6.12.0 实际报告复查，无未经确认的旧版专用抑制。
- [ ] 分领域检查及未参与实现者的 final review 完成，发现已修复并复验。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-11-06 | Qt 官方在线仓库 `Updates.xml` 与 MirrorBrain `Digest: SHA-256` 响应 | 归档名、版本和每个归档 SHA256 可复核 | 通过；三平台各 4–5 个归档及 Linux ICU 的摘要与大小已核对；macOS 实际供给通过 SHA256 校验。 |
| 2026-11-06 | Qt 6.12 官方新功能 / Quick 性能文档与 Caret QSG/QML 源码盘点 | 识别与现有画布相关的新能力及适用性 | Canvas Painter / Canvas2D 是额外 GPU 绘制后端；当前 QSG 已复用节点并裁剪可见页。QRangeModel 面向集合视图，当前候选列表较小；QML 热重载只影响开发预览。没有与当前功能匹配、足以支持迁移的能力；DirectWrite GASP 改进由 Qt 自动应用于 Windows UI 文本，无需调用新 API。 |
| 2026-11-06 | Qt 6.12.0 官方包 Mach-O / `moc -v` / Linux ICU archive | 检查 macOS 最低系统版本、moc 宏修订和 ICU ABI | QtCore Mach-O `minos 14.4`；`moc 6.12.0`，`Q_MOC_OUTPUT_REVISION 69`；Linux ICU archive 与其 SHA256 一致，包含 `libicu*.so.73.2`。用户确认将 Caret 最低 macOS 版本提高到 14.4；构建常量、打包环境和基线已同步。 |
| 2026-11-06 | 托管 Qt 6.12.0 供给器；Mac arm64；`cargo format --check`；`MACOSX_DEPLOYMENT_TARGET=14.4 cargo build --locked -p caret-launcher`；`MACOSX_DEPLOYMENT_TARGET=14.4 cargo lint --check` | 固定摘要的 Qt 供给、应用构建、native/QML 静态检查通过 | 供给器下载、SHA256 校验并解包五个 macOS Qt 归档；重复运行命中完整 kit 指纹并幂等跳过。格式、构建和 lint 均通过。低于 14.4、边界及非法部署目标输入未实测；sanitizer 与 Windows/Linux 构建仍未运行。 |
| 2026-11-06 | `MACOSX_DEPLOYMENT_TARGET=14.4 CARET_TEST_CTEST_LABELS=qml cargo test --locked -p caret-tests --test native`；Mac arm64、Qt 6.12.0 | QML ShellLoad、无 runtime 偏好和 QML 格式检查通过 | 3/3 通过。首轮 Linux CI 暴露 `ShellLoadTest::closeFailureDoesNotReopenPendingConfirmation` 仍查找已改名的 `window` ID；测试断言已更新为 `rootWindow`。 |
| 2026-11-06 | MacBook Air Mac16,12 / Apple M4 / 24 GB，macOS 27.2 (26B123)，arm64，Release；`MACOSX_DEPLOYMENT_TARGET=14.4 cargo performance qt cpu --samples 10` 两次 | 在 Qt 6.12 下复测 6.11.2 的 400 页跨页滚动场景 | Software renderer，1280×960；6.12 两次 median/P95 为 2.460/4.433 ms 与 1.888/1.943 ms，旧基线为 2.047/2.215 ms（Mac arm64、Apple M4、macOS 27.0.1，10 个样本）。小样本、OS 版本变化不支持稳定比较；数据只作本机诊断，不用于宣称性能优化。 |
| 2026-11-08 | `MACOSX_DEPLOYMENT_TARGET=14.3 cargo build --locked -p caret-launcher`；Nightly run 42399746940 的三平台 workspace tests、quality 与 sanitizer | 验证低部署目标被拒绝并复核跨平台构建/测试 | 14.3 在 caret-ffi build script 被明确拒绝；macOS/Windows/Ubuntu workspace tests 与 Ubuntu quality 通过。macOS sanitizer 的 `Qml.ShellLoad` 因 LaunchServices/CoreFoundation 泄漏报告失败；Windows/Ubuntu sanitizer 通过。Rust coverage 首次为 84.96%，补测后本地达标，远程待复验。 |
| 2026-11-08 | `cargo sanitize`；带托管 `llvm-symbolizer` 的单项 `Qml.ShellLoad` CTest（macOS 27.2 / Qt 6.12.0） | 验证 sanitizer 执行并检查系统泄漏栈 | 完整 profile 因 macOS 27.2 `atos` symbolizer 未启动及系统缓存泄漏失败；单项测试断言 49 项通过，但 LSan 仍报 1392 字节 / 33 个 Apple FontRegistry/CoreText 与字形缓存分配。该栈与 Nightly macOS 26.7.2 的 LaunchServices `NotifyToken::RegisterDispatch` 5224 字节报告不同；新增规则尚未在本机复现同一分配，须由 Nightly 确认。 |
| 2026-11-08 | Nightly run 42403037844 | 复验三平台 Qt 6.12 workspace tests 与 sanitizer | 三平台 workspace tests 和 sanitizer 全部通过；macOS ASan/UBSan、TSan CTest 均 9/9，`Qml.ShellLoad` 两轮通过。run 尚因 Rust CPU performance job 保持 in-progress，本次未等待该 job。full quality 的 Clippy 失败来自 070 FFI 测试，与 067 sanitizer 结果无关。 |
| 2026-11-08 | gpt-6.1-terra/medium 领域检查与 gpt-6.1-sol/medium 最终只读评审（PR #101） | 审查 LSan 单符号范围、风险和任务状态 | 通过；要求保留同栈未来泄漏可能被隐藏的风险，以及 macOS 26 Nightly 复查条件，均已记录。follow-up 测试代码仍待最终只读复审。 |
| 2026-11-08 | PR #101 Nightly 和 follow-up `cargo quality` | 复核 sanitizer、三平台构建与完整质量门禁 | macOS/Windows/Ubuntu sanitizer 与 workspace tests 均通过；Mac `Qml.ShellLoad` ASan/UBSan 与 TSan 两轮 9/9。Nightly full quality 因 070 FFI 测试的 `expect` 用法失败；修复后的本机 `cargo quality` 全部通过，包括 lint、依赖审计、workspace 与 native/QML tests。 |

## 风险与回退

Qt 6.12 官方 macOS 包的 Mach-O 最低版本为 14.4；用户已确认同步提高 Caret 的最低 macOS 版本。回退到旧版供给脚本前必须清理 `.toolchain/qt`（或 `$CARET_TOOLCHAIN_ROOT/qt`），详见依赖获取规范。若官方包不兼容、Linux ICU ABI 不符或组件不齐，停止升级并保留 6.11.2 基线，等待可验证的兼容方案。LaunchServices 单符号 LSan 规则会抑制任何经过该通知回调的泄漏栈，包括未来相同栈的新问题；必须在 macOS 26 Nightly 确认原报告消失且没有 Caret 分配栈，若报告变化则按新栈重新审查，不扩展泛匹配。

## 决策与工作记录

- 2026-11-06：开始升级审计。官方发布页称 Qt 6.12 为 LTS；Qt release plan 记载 6.12.1 计划于 2026-11-17 发布，因此本次目标为 6.12.0。issue #76 已关闭；本地分支重命名为 `yuki/qt-6.12-upgrade`。
- 2026-11-06：按用户补充，审查 Qt 6.12 与 Caret 当前 QSG 画布相匹配的能力，并运行现有 CPU 软件渲染基准两次。样本方向不一致且与既有记录的 OS 版本不同，证据不足以判断细微升降；本轮不声称性能提升，不为采用新 API 而重写绘制后端。
- 2026-11-06：新能力审查没有找到能替代现有文档画布的直接方案：Canvas Painter 的 QML Canvas2D 仍为技术预览，且不能复用现有 QSG 节点/字形纹理路径；QRangeModel 与 QML 热重载也不针对当前画布运行时瓶颈。决定不为了追新功能引入额外模块或未经测量的渲染器迁移。Qt 6.12 的 DirectWrite GASP 改进会由 Qt 自动提供给 Windows UI 文本，不需额外调用 API。
- 2026-11-06：最终复审发现环境变量可将 macOS 部署目标下调至 13.0；已统一通过 `caret-build::macos_deployment_target()` 校验，代码路径会拒绝低于 14.4 的值。负向/边界/非法输入未实测，相关验收项保持未完成。复审要求性能记录包含机器、系统和 Release 配置；已补齐。
- 2026-11-08：部署目标 14.3 被构建脚本拒绝；Nightly 三平台 workspace tests 与 Ubuntu quality 通过，但 macOS sanitizer 的 Qml.ShellLoad 泄漏检查失败，sanitizer 验收未完成，任务状态维持 in-progress。
- 2026-11-08：日志两笔泄漏均来自 LaunchServices `NotifyToken::RegisterDispatch` 系统回调；按经确认的 macOS 系统框架抑制模式增加单符号 LSan 规则。macOS 27.2 本机 Qt 6.12 单项测试仍报告另一类 Apple 字体注册 / CoreText 缓存；因它与 Nightly macOS 26.7.2 栈不同，未扩大抑制。待远程 Nightly 验证原失败是否消失。
- 2026-11-08：PR #101 的 Nightly run 42403037844 在 macOS 26.7.2 上通过 ASan/UBSan 与 TSan 全部 9 项 CTest，`Qml.ShellLoad` 均通过；Windows/Ubuntu sanitizer 与三平台 workspace tests 也通过。full quality 的失败仅是 070 FFI 测试违反 Clippy `expect_used`，与 067 sanitizer 结论无关；保留独立 macOS 27.2 字体缓存报告。

## 完成摘要

Qt 6.12.0 供给、最低 macOS 部署版本、三平台 workspace tests 和 Nightly macOS 26 sanitizer 已验证；macOS 27.2 本机 Qt 6.12 仍有独立字体缓存泄漏报告。PR #101 上的 067 review 已完成，Nightly full quality 的 Clippy 失败属于 070 测试，修复后的完整质量检查通过；follow-up 最终只读评审和远程复验完成后再收尾。
