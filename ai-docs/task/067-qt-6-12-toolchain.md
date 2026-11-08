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

- [x] macOS、Windows、Linux 使用官方 Qt 6.12.0 归档；各归档 SHA256 来源可复核。
- [x] Linux ICU 与 Qt 6.12.0 配套，并由托管供给脚本校验。
- [x] 三平台 CMake 要求和技术基线一致；无残留的当前版本锁 6.11.2。
- [x] macOS 构建拒绝低于 14.4 的 `MACOSX_DEPLOYMENT_TARGET` 覆盖。
- [x] 托管供给、应用构建、相关 native/QML 检查通过；未运行平台明确记录。
- [ ] sanitizer suppression 与测试排除规则经 Qt 6.12 实际报告复查，不保留未经确认的 Qt 6.11 专用例外。macOS 27 本机完整 ASan/UBSan CTest 仍有泄漏报告；删除 URL suppression 后的 macOS Nightly 和 Linux 移除 TSan 排除 / `FcInit` suppression 后的 Nightly 均待复验。`App.InvalidArgument` 已改为显式检查退出码，避免 `WILL_FAIL` 把 sanitizer 非零退出反转为通过。
- [x] 分领域检查及未参与实现者的 final review 完成，发现已修复并复验。

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
| 2026-11-08 | `cargo sanitize`；带托管 `llvm-symbolizer` 的单项 `Qml.ShellLoad` CTest（macOS 27.2 / Qt 6.12.0） | 验证 sanitizer 执行并检查系统泄漏栈 | 完整 profile 因 macOS 27.2 `atos` symbolizer 未启动及泄漏报告失败；单项测试断言 49 项通过，但 LSan 仍报 1392 字节 / 33 个泄漏，调用栈经过 Apple FontRegistry/CoreText 与字形处理代码。实际归属未确认。该栈与 Nightly macOS 26.7.2 的 LaunchServices `NotifyToken::RegisterDispatch` 5224 字节报告不同；新增规则尚未在本机复现同一分配，须由 Nightly 确认。 |
| 2026-11-08 | Nightly run 42403037844 | 复验三平台 Qt 6.12 workspace tests 与 sanitizer | 三平台 workspace tests 和 sanitizer 全部通过；macOS ASan/UBSan、TSan CTest 均 9/9，`Qml.ShellLoad` 两轮通过。run 尚因 Rust CPU performance job 保持 in-progress，本次未等待该 job。full quality 的 Clippy 失败来自 070 FFI 测试，与 067 sanitizer 结果无关。 |
| 2026-11-08 | gpt-6.1-terra/medium 领域检查与 gpt-6.1-sol/medium 最终只读评审（PR #101） | 审查 LSan 单符号范围、风险和任务状态 | 通过；要求保留同栈未来泄漏可能被隐藏的风险，以及 macOS 26 Nightly 复查条件，均已记录。follow-up 测试代码仍待最终只读复审。 |
| 2026-11-08 | PR #101 Nightly 和 follow-up `cargo quality` | 复核 sanitizer、三平台构建与完整质量门禁 | macOS/Windows/Ubuntu sanitizer 与 workspace tests 均通过；Mac `Qml.ShellLoad` ASan/UBSan 与 TSan 两轮 9/9。Nightly full quality 因 070 FFI 测试的 `expect` 用法失败；修复后的本机 `cargo quality` 全部通过，包括 lint、依赖审计、workspace 与 native/QML tests。 |
| 2026-11-08 | `rg -n '6.11.2' native/cmake/qt-provision.cmake native/CMakeLists.txt crates/caret-build/src/qt.rs ai-docs/standards/baseline.md`；Nightly run 42405417514 | 确认版本锁与技术基线，并复验修复后的质量、三平台测试和 sanitizer | 目标配置文件无 6.11.2 残留；run 的 full quality、Rust/native coverage、Miri、三平台 workspace tests 与 sanitizer 均通过。macOS ASan/UBSan、TSan 两个 profile 的 CTest 各 9/9，均包含 Qml.ShellLoad。本机 macOS 27.2 的独立泄漏报告（栈经过 FontRegistry/CoreText）和 Qt 6.11 旧规则审查仍未解决。Rust CPU performance 仍运行，未作为阻塞条件。 |
| 2026-11-08 | gpt-6.1-terra/medium 领域检查与 gpt-6.1-sol/medium 最终只读评审（PR #102 follow-up） | 复核质量修复、文档证据、LSan 风险与任务收尾 | 通过；LSan 同栈未来泄漏风险已保留，本机 macOS 27.2 独立泄漏报告及其未确认的实际归属已明确标注。 |
| 2026-11-08 | macOS 27.2 / Qt 6.12.0：托管 `llvm-symbolizer` 下完整 ASan/UBSan CTest（9 项） | 逐项观察旧 Qt suppression 命中与退出期泄漏 | 3/9 被标为通过，6/9 因 LSan 报告失败；`App.InvalidArgument` 虽显示通过，实际有 192 字节 / 4 分配泄漏，被 `WILL_FAIL` 反转。`Qml.ShellLoad` 报 1256 字节 / 30 个分配。完整日志确认旧规则 `invalidate`、`fallbacksForFamily`、`ct_getSfntTable`、`boundingBox`、`QCoreTextFontEngine::QCoreTextFontEngine` 和 `loadAdvancesForGlyphs` 都在 Qt 6.12 的测试报告中命中；`qt_apple_urlFromPossiblySecurityScopedURL` 未命中，本轮另有不同的 Qt 6.12 `qt_apple_bundleType` 栈，待远程复验且不认定两者为同一路径。移除未命中的旧规则，不替换为 bundle suppression。剩余栈分别经过 Fontique/Parley/Caret 字体 fallback、Qt bundle 路径初始化、CoreText glyph rendering 与 Apple XPC/CoreFoundation；所有权和生命周期仍未证明，不添加泛匹配。 |
| 2026-11-08 | Nightly run 42405417514 Ubuntu sanitizer ASan/UBSan 与 TSan 日志 | 核对 Qt 6.11 Linux sanitizer 例外是否实际覆盖 | ASan/UBSan 9/9 通过，TSan 7/7 通过；TSan 仍排除 `Qml.ShellLoad` 和 `App.HeadlessWindowLoads`，而 code-font CTest 的 suppression 命中明细未出现在通过测试日志。已移除 Qt6.11 TSan 排除与 `FcInit` 规则，合并后的 Nightly 待验证无旧例外运行结果。 |
| 2026-11-08 | `cargo fmt --all --check`；`cargo test --locked -p caret-tests --bin caret-tests sanitize::tests::sanitizer_profiles_keep_checks_enabled`；`cargo lint --check` | 校验 sanitizer runner 改动和仓库格式/lint | 全部通过；Linux sanitizer 的改动需由 Ubuntu Nightly 运行时复验。 |
| 2026-11-08 | CMake CLI 结果验证回归；常规与 ASan/UBSan build 的 `ctest -R 'Build.CliResultSanitizerDetection|App.InvalidArgument' --output-on-failure` | 覆盖无效参数的错误码/诊断，以及 LSan 报告不得被接受为预期 CLI 失败 | 合成用例确认退出码 64 且含 `LeakSanitizer` 报告会被拒绝，正常退出码 64 和错误码 1 分别按契约接受/拒绝；另确认 `LSAN_OPTIONS=log_path=...` 会被拒绝而保留 suppression 选项可用；两种 build 的 CTest 各 2/2 通过。本机 sanitizer app 此次未复现之前观察到的 192 字节泄漏；托管 LLVM 的真实泄漏 fixture 在 stderr 模式下被判失败、在 log_path 模式下被明确拒绝。远程 Nightly 仍待复验。 |
| 2026-11-08 | Terra/medium 领域检查与 Sol/medium final review（当前 13 文件完整 diff） | 复核 sanitizer 选项重定向拦截、Linux sanitizer 矩阵恢复、测试覆盖与文档证据 | 均无阻断发现；真实 LSan fixture 与 CMake 合成回归通过。macOS 27.2 的本机 LSan 报告和修改后远程 Nightly 未验证范围继续保留为未完成验收。 |

## 风险与回退

Qt 6.12 官方 macOS 包的 Mach-O 最低版本为 14.4；用户已确认同步提高 Caret 的最低 macOS 版本。回退到旧版供给脚本前必须清理 `.toolchain/qt`（或 `$CARET_TOOLCHAIN_ROOT/qt`），详见依赖获取规范。若官方包不兼容、Linux ICU ABI 不符或组件不齐，停止升级并保留 6.11.2 基线，等待可验证的兼容方案。LaunchServices 单符号 LSan 规则会抑制任何经过该通知回调的泄漏栈，包括未来相同栈的新问题；必须在 macOS 26 Nightly 确认原报告消失且没有 Caret 分配栈，若报告变化则按新栈重新审查，不扩展泛匹配。任务收尾还需逐项审计仍标注 Qt 6.11 的 suppression，并查明 macOS 27.2 泄漏栈所经 FontRegistry/CoreText 分配的实际归属与生命周期。

## 决策与工作记录

- 2026-11-06：开始升级审计。官方发布页称 Qt 6.12 为 LTS；Qt release plan 记载 6.12.1 计划于 2026-11-17 发布，因此本次目标为 6.12.0。issue #76 已关闭；本地分支重命名为 `yuki/qt-6.12-upgrade`。
- 2026-11-06：按用户补充，审查 Qt 6.12 与 Caret 当前 QSG 画布相匹配的能力，并运行现有 CPU 软件渲染基准两次。样本方向不一致且与既有记录的 OS 版本不同，证据不足以判断细微升降；本轮不声称性能提升，不为采用新 API 而重写绘制后端。
- 2026-11-06：新能力审查没有找到能替代现有文档画布的直接方案：Canvas Painter 的 QML Canvas2D 仍为技术预览，且不能复用现有 QSG 节点/字形纹理路径；QRangeModel 与 QML 热重载也不针对当前画布运行时瓶颈。决定不为了追新功能引入额外模块或未经测量的渲染器迁移。Qt 6.12 的 DirectWrite GASP 改进会由 Qt 自动提供给 Windows UI 文本，不需额外调用 API。
- 2026-11-06：最终复审发现环境变量可将 macOS 部署目标下调至 13.0；已统一通过 `caret-build::macos_deployment_target()` 校验，代码路径会拒绝低于 14.4 的值。负向/边界/非法输入未实测，相关验收项保持未完成。复审要求性能记录包含机器、系统和 Release 配置；已补齐。
- 2026-11-08：部署目标 14.3 被构建脚本拒绝；Nightly 三平台 workspace tests 与 Ubuntu quality 通过，但 macOS sanitizer 的 Qml.ShellLoad 泄漏检查失败，sanitizer 验收未完成，任务状态维持 in-progress。
- 2026-11-08：日志两笔泄漏均来自 LaunchServices `NotifyToken::RegisterDispatch` 系统回调；按经确认的 macOS 系统框架抑制模式增加单符号 LSan 规则。macOS 27.2 本机 Qt 6.12 单项测试仍报告另一组泄漏，调用栈经过 FontRegistry/CoreText；因它与 Nightly macOS 26.7.2 栈不同，未扩大抑制。后续 Nightly 对原报告的复验结果见上表。
- 2026-11-08：PR #101 的 Nightly run 42403037844 在 macOS 26.7.2 上通过 ASan/UBSan 与 TSan 全部 9 项 CTest，`Qml.ShellLoad` 均通过；Windows/Ubuntu sanitizer 与三平台 workspace tests 也通过。full quality 的失败仅是 070 FFI 测试违反 Clippy `expect_used`，与 067 sanitizer 结论无关；保留 macOS 27.2 独立泄漏报告，实际归属待查。
- 2026-11-08：Qt 6.12/macOS 27 完整 verbose CTest 确认旧版 CoreText 函数 suppression 仍会匹配 6.12 栈；`qt_apple_urlFromPossiblySecurityScopedURL` 未命中，本轮另观察到不同的 `qt_apple_bundleType` 栈，证据不足以证明插件路径相同，故移除旧 URL 规则且不新增 bundle suppression。删除 URL 规则后的 macOS 26 远程 sanitizer 复验仍待执行；残余的 Fontique/Caret 字体 fallback、Qt bundle 初始化、CoreText glyph 和 XPC 栈保留为 LSan 失败。
- 2026-11-08：Nightly #42405417514 Linux TSan 仍排除两项启动测试，Ubuntu code-font suppression 的规则命中明细无法从通过测试日志获知。为按 Qt 6.12 实际行为复验，删除 Linux `FcInit` suppression 并恢复两项 Linux TSan 测试到矩阵；待 PR 合并后的 Nightly 结果。
- 2026-11-08：本机 verbose CTest 发现 `App.InvalidArgument` 的 `WILL_FAIL` 会把 LSan 的 192 字节泄漏也当成预期失败。改用 CMake wrapper 同时校验退出码 64、stderr 用法诊断并拒绝 ASan/LSan/TSan/UBSan 报告；若 sanitizer 的 `log_path`（冒号、逗号、空白、CR 或 LF 分隔）将报告移出捕获流，则明确失败。新增独立 CMake 回归覆盖“正确错误码 + LSan 报告”和各类 log_path 重定向。同步移除已无调用者的 CTest `exclude` 参数和 `-E` 分支。
- 2026-11-08：sanitizer runner 定向单测、完整格式与九项跨语言 lint 通过；Linux sanitizer 调整待远程运行时验证。

## 完成摘要

Qt 6.12.0 三平台供给、官方归档与 Linux ICU 校验、基线和最低 macOS 部署版本均已验证。任务维持 in-progress：Mac 26 Nightly 原有 sanitizer 失败已修复；Mac 27.2 本机完整 CTest 仍有多项 LSan 报告，尚不能证实相关 Apple/Qt 分配生命周期无害。旧 Qt6.11 URL suppression 已移除；Linux Qt6.11 TSan 排除与 `FcInit` suppression 也已移除，待合并后的 Nightly 验证恢复的测试矩阵。旧 CoreText Qt 函数 suppression 仍在 Qt6.12/macOS27 报告中命中，需结合残余泄漏调查复核其所有权风险。Nightly CPU performance 不属于本任务验收。
