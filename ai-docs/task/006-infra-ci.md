# 006 — CI 与打包管线

- 状态：implemented
- 依赖：005
- 优先级：P0
- 创建 / 更新：2026-10-25 / 2026-10-25

## 目标与背景

005 之后仓库里已经有 Rust、QML 和 native 入口。本任务把来源的质量、夜间、打包和 GoogleTest SDK 工作流移植过来，并改成 caret 的名字和 `.toolchain/` 缓存。移植完成后，基础设施阶段结束。

## 必读

- [持续集成](../standards/ci.md)、[依赖获取](../standards/dependency-acquisition.md)、[测试规范](../standards/testing.md)
- [Cargo](../standards/cargo.md)

## 范围与非目标

移植 `ci.yml`、`nightly.yml`、`package.yml`、`sdk-googletest.yml` 和配套 composite action。环境变量 `PANTA_*` 改为 `CARET_*`。工具缓存指向 `.toolchain/`。

本次先检查现有代码并修复 CI 前置：补齐 CMake 分层夹具和真实构建图检查，清除测试选择器中来源项目的无效模块映射，核对质量聚合与打包入口的参数和注册。只优化影响本任务正确性或验证覆盖的代码，不做无关重构。移植基准为来源仓库 `ee84d95`，实际取用文件记录在工作记录中。按用户后续要求，日常 push/PR 只运行 Ubuntu；macOS、Windows 构建测试与 Objective-C++ 检查放入 nightly，手动打包发布仍验证三平台。

按用户补充要求，`format`、`lint`、`test` 沿用 panta 的统一调度与检查机制；GoogleTest 另参考 [sdk-packaging](https://github.com/Jack-Lab-at-EIT/sdk-packaging) `a43519510326d26e3118b5d3f888285d4c38c955` 的三平台生产、自检、归档和发布方式，在 Caret 仓库独立打包，补齐带固定摘要的懒供给消费入口。不依赖其他组织的 Release。发布默认关闭，只有发布 job 有写权限。

不移植几何、网格和渲染 SDK 的工作流。不把夜间覆盖率改成编辑器性能门禁。函数和行覆盖率门槛仍是 85%，只在夜间阻断。

## 前置条件与待决策

005 已 done，空白窗口的构建是 CI 要覆盖的 native 目标。

## 实施步骤

1. 移植 workflow 和 action，按规范改名、改缓存键。记录来源 commit。
2. 核对纯文档变更不创建运行，未知路径扩大到完整范围。
3. 本地跑通 CI 会调用的 fmt、clippy 和已有测试。workflow 本身在推送或 `gh` 手动运行后把结论写回本文件。
4. 确认没有来源前缀残留在 `.github/`。
5. 独立评审后复验。

## 预计改动

新建 `.github/workflows/`、`.github/actions/`、`tools/sdk/`、native 分层及 SDK 供给夹具、质量工具配置和 native 聚合测试；修复质量调度、native 入口异常边界及桌面 Runtime 安装部署。正文编辑行为不在本任务。

## 验收标准

- [x] 四条要移植的 workflow 都在，排除的 SDK workflow 不在。
- [x] `.github/` 里没有 `panta` / `PANTA_` 前缀。
- [x] 缓存键不包含 `target/` 里的工具目录。
- [x] 本地 fmt、clippy 和已有测试通过。远程运行若尚未推送，验证表写明未执行，不记为通过。评审发现已处理。
- [x] GoogleTest 独立生产、归档搬迁自检与供给正反夹具通过，未发布资产不伪造固定摘要。
- [x] macOS 桌面包在清除开发环境变量后可启动；Windows、Linux 远程结果另记。
- [ ] 日常 Ubuntu CI、三平台 nightly 和桌面打包有对应远程证据后，再将任务标为 done；SDK 生产发布已由 016 完成。

## 验证计划与结果

| 日期 | 环境 / 命令 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| 2026-10-25 | 检索 `.github/` 来源前缀、`git diff HEAD --check` | 无残留、无空白错误 | 通过 |
| 2026-10-25 | `cargo quality`，macOS arm64，锁定工具链 | 跨语言格式、完整 lint、审计、workspace 测试通过 | 通过；含 4 项 CTest。审计仅有允许但当前未用到的许可证提示，无失败；不隐藏提示 |
| 2026-10-25 | 最终 SDK 事务修复后：`cargo format --check`、`cargo lint cmake --check`、`cargo test --locked -p caret-tests --test native` | 受影响检查复验 | 通过；4 项 CTest 全通过 |
| 2026-10-25 | `cargo run --locked -p caret-tests -- toolchain` | 实际使用托管工具和真实编译数据库 | 通过；不再要求没有消费者的 SDK 或不存在的手写 adapter |
| 2026-10-25 | `actionlint` 四条 workflow、`shellcheck` 五段 composite shell、本地链接检查 | 语法、脚本及文档入口可用 | 通过；65 个 Markdown 文件的本地链接与锚点通过 |
| 2026-10-25 | `cargo caret-package` 生成新目录，macOS 签名检查与清空 Qt/QML/DYLD 开发变量启动 | 安装树独立运行 | 通过；`--quit-after-load` 退出码 0，修复前缺失 QtQuick.Layouts 的情况已消除 |
| 2026-10-25 | `cargo run --locked -p caret-tests -- sdk-googletest target/sdk-production/googletest target/sdk-production/work macos-arm64` | 生产固定源码 SDK 并消费搬迁归档 | 通过；解包目录中静态链接及运行 1/1，归档在 `target/sdk-production/work/dist/` |
| 2026-10-25 | `caret_require_googletest()` 真实归档消费与失败路径夹具 | 强摘要、懒供给、CONFIG 和升级失败保留旧 SDK | 通过；真实 `GoogleTest.ConsumerSmoke` 1/1；错误摘要、缺 target、错误版本等均拒绝，旧安装保持有效 |
| 2026-10-25 | subagent `review_006`，`gpt-6.1-sol`、high，完整 diff 及最终增量 | 独立评审发现已处理并复验 | 通过；配置缺失、Windows 安装转义、manifest、发布保留资产及 SDK 升级事务均已修复，最终无剩余实质问题 |
| 2026-10-26 | [PR #26 Ubuntu CI](https://github.com/VibeSTEM/caret/actions/runs/40211041328)、[PR #27 Ubuntu CI](https://github.com/VibeSTEM/caret/actions/runs/40212390890) 与[三平台 Nightly](https://github.com/VibeSTEM/caret/actions/runs/40212431385) | 日常 CI 和三平台 nightly 全部通过；桌面打包分别有三平台证据 | PR #26 CI 全部检查通过；PR #27 相关 Ubuntu 检查通过，按变更范围跳过无关 job。Nightly 12/12 job 成功，覆盖率函数 87.12%、行 88.32%，macOS sanitizer 两个 profile 的 CTest 均 4/4。Windows/Linux 桌面包仍待远程验证；SDK 另见 016 |
| 2026-10-25 | 016 的 [三平台运行](https://github.com/VibeSTEM/caret/actions/runs/40172423089)及[正式 Release](https://github.com/VibeSTEM/caret/releases/tag/sdk-googletest-1.18.0)，下载三归档复算 SHA256 | 当前仓库有完整、不可变资产 | 全部成功；正式三平台摘要固定在 `native/cmake/googletest-sha256.json`，与各校验文件和 GitHub 资产 digest 一致 |
| 2026-10-25 | 不设本地 ROOT/URL/SHA 覆盖，经默认正式 URL 和固定清单供给 SDK，再编译链接消费 | 正式发布与消费者端到端可用 | macOS 通过；`GoogleTest.ConsumerSmoke` 1/1，构建树 `target/gtest-official-consumer` |
| 2026-10-25 | subagent `final_index_review`，`gpt-6.1-sol`、high，完整暂存区及修复增量 | 无未处理实质问题 | 79 文件完整审查发现 1 项分层表达式绕过；修复后独立复跑全部分层夹具通过，原绕过被拒绝、合法 namespace 包装可用，最终无剩余实质发现 |
| 2026-10-26 | [Nightly run 40209293853](https://github.com/VibeSTEM/caret/actions/runs/40209293853) | 三平台 nightly、覆盖率与 sanitizer 检查通过 | 覆盖率门槛失败（函数 77.89%、行 82.12%）：`caret-tests --test document` 的 18 项公开文档测试没有被 coverage runner 采集；macOS `Qml.ShellLoad` 因 Qt/CoreText/CoreGraphics 进程缓存泄漏报告失败。修复已加入文档测试采集、行为覆盖及窄符号抑制，等待新 Nightly 复验 |
| 2026-10-26 | [Nightly run 40211105819](https://github.com/VibeSTEM/caret/actions/runs/40211105819) | 重跑覆盖率与三平台 nightly | 覆盖率门禁通过；Miri、Ubuntu quality、三平台 workspace tests、Linux/Windows sanitizer、native coverage、macOS Objective-C++ 和性能样例均通过。仅 macOS sanitizer 的 `Qml.ShellLoad` 仍报告 8,824 字节 / 69 个 Apple CoreText/CoreGraphics/FontParser 字体缓存分配；新增按失败栈符号限定的抑制，等待再次 Nightly 复验 |
| 2026-10-26 | 本机 macOS 27：`ASAN_SYMBOLIZER_PATH=<managed llvm-symbolizer> cargo sanitize` | 本机 macOS ASan/UBSan 与 TSan 全套通过 | 精确 CoreText/Qt 规则将 `Qml.ShellLoad` 字体缓存报告从 17,812 字节降为 192 字节；`QmLoad` 和 `ShellLoad` 各仍有 192 字节未符号化的 CoreFoundation preferences/XPC 栈。现有 `CFPrefsPlistSource` 抑制无法在本机命中；未添加通用屏蔽。因 ASan/UBSan profile 失败，TSan profile 未运行 |
| 2026-10-26 | [Nightly run 40212431385](https://github.com/VibeSTEM/caret/actions/runs/40212431385) | 覆盖率门槛、三平台完整测试、所有 sanitizer 与质量检查通过 | 全部 12 个 job 成功；函数覆盖率 710/815（87.12%），行覆盖率 8477/9598（88.32%）；macOS sanitizer 两个 profile 的 CTest 各 4/4，Linux/Windows sanitizer、Miri、native coverage、三平台 workspace tests、Objective-C++、quality 和 CPU performance 均通过 |
| 2026-10-27 | [Nightly run 40530614729](https://github.com/VibeSTEM/caret/actions/runs/40530614729) | PR #37 合并后的三平台回归验证 | 失败项：Ubuntu quality 的 clang-format（已由 PR #37 修复）；Qt GPU 的 xcb cursor 运行库缺失；Miri 外层 `cargo ub-check` 未继承 Fontconfig dlopen 环境变量；Windows ASan 检查目标未构建；Windows IME 分页夹具依赖固定段落数量；Rust 函数覆盖率 82.18%、行覆盖率 91.99%。其余工具/Windows 问题与覆盖率检查在本分支修复，未经新 Nightly 复验不标记通过 |
| 2026-10-28 | [PR #38 CI](https://github.com/VibeSTEM/caret/actions/runs/40545791759)、[手动 Nightly](https://github.com/VibeSTEM/caret/actions/runs/40545878360)、[scheduled Nightly](https://github.com/VibeSTEM/caret/actions/runs/40591289048) | 合并后确认 CI 与 Nightly 故障 | PR CI 与 Nightly 的部分 Ubuntu job 在 apt 下载中文字体包时超过 90 秒操作上限；Qt GPU job 的 apt 安装成功且已安装 `libxcb-cursor0`，但 Qt 仍无法加载 xcb plugin，表明还缺其他运行库。按 [Qt for X11 的平台插件依赖清单](https://doc.qt.io/qt-6.10/linux-requirements.html#platform-plugin-dependencies)补齐常见 XCB/XKB runtime 库，并在 GPU benchmark 前运行 `ldd` 诊断。Miri 未启用 Fontique `fontconfig-dlopen` feature，触发 `fontconfig_sys` 条件编译不匹配；Windows ASan 通过硬编码路径未找到 CTest 注册的 QML 测试程序。修复中将 apt 单次操作上限提高到 300 秒、启用正确 Cargo feature、改从 CTest JSON manifest 读取测试二进制路径；等待远程复验 |
| 2026-10-28 | 本地 `cargo coverage`（修复分支，macOS arm64） | Rust 覆盖率函数与行达到 85% | 通过；函数覆盖率 1003/1152（87.07%），行覆盖率 12337/13436（91.82%）。大体量私有测试子模块移入各模块目录的 `tests.rs`，cargo-llvm-cov 默认排除测试文件后不再把测试自身的失败诊断闭包计入生产函数分母；测试内容保持不变。Nightly 远程复验待 PR 合并后执行 |

## 风险与回退

来源 workflow 假设的目录这阶段已经存在。若某一步仍引用排除掉的 SDK，删掉该步，不把 SDK 加回来。回退是移除 `.github/workflows` 中对应文件。

## 决策与工作记录

- 2026-10-25：创建任务。CI 放在 launcher 之后，这样第一次远程运行面对的是能编译的树。
- 2026-10-25：开始代码检查与 006 实施；工作分支 `yuki/infra-ci`。本地验证与远程 workflow 结果分开记录，006 主体尚未提交或推送；016 独立 SDK 后续获得单独提交、合并和发布授权。
- 2026-10-25：从 panta `ee84d95` 移植 CI/nightly、六个 composite action、CMake 分层检查及正反夹具、Python 锁文件与 Qt 静态分析模型；按本仓库现有 crate 和 CTest 标签裁剪。`cargo test --workspace` 现在执行 runner 自身回归测试和 native 聚合；删除不存在的几何、网格、工程基准、Miri 目标和无证据 sanitizer 豁免。
- 2026-10-25：实际 lint 发现并修复入口异常逃逸、缺失直接头文件、Cppcheck 导入字符串宏丢引号、无基准时强求 moc 目标、缺失配置文件及过期依赖审计豁免。完整质量入口包含格式、九阶段 lint、审计与 workspace 测试，未改变夜间覆盖率阈值。
- 2026-10-25：桌面包补齐静态 QML imports 扫描、Qt/Cargo 许可资料、Windows manifest 的 SDK mt 嵌入及 PE 验收、Windows 安装路径规范化。SDK 固定 GoogleTest `063de7e9578f82b369302001269680b4b1553359`、LLVM 22.1.7、C++20 和 Windows `/MD`；先完整上传 draft，已发布制品不可覆盖。
- 2026-10-25：独立评审修复 SDK 升级事务：staging 先在隔离 CXX 工程验证 CONFIG 精确版本及两个必需 targets，再写摘要和替换目录。正确摘要但 CONFIG 损坏时保留旧安装及旧标记。新增回归并由评审者独立执行夹具通过。本地聚合和受影响检查复验后标为 implemented，等待三平台远程证据。
- 2026-10-25：按用户要求收敛日常 CI 为 Ubuntu，移除 push/PR 的 macOS/Windows matrix 和 Objective-C++ job；nightly 保留三平台全量测试、sanitizer 和 macOS Objective-C++。手动 SDK 与桌面打包仍保留三平台矩阵。
- 2026-10-25：独立 SDK 已从 006 拆出为 016，并在 `main` 合并；本分支快进跟进，SDK 生产只保留 `tests/src/sdk.rs`，桌面 `package.rs` 不再重复实现。日常 Ubuntu check 与 nightly 三平台 full-tests 在成功后保存可信 main 缓存，避免 macOS/Windows 每晚冷启动；PR 不保存缓存。
- 2026-10-25：用户确认 SDK 已发布，要求更新 SHA256、对暂存区 subagent review，确认后创建 PR 合并。下载六项正式资产、逐归档复算并固定平台摘要；默认供给路径的真实消费自检通过，不再使用本机临时摘要。
- 2026-10-25：最终完整暂存区由 subagent `final_index_review`（`gpt-6.1-sol`、high）只读审查，发现分层生成表达式解析把 namespace 的冒号误作分隔符，可通过拼接 alias 绕过检查。改为只记录首个顶层分隔冒号，补 `$<LINK_ONLY:hidden::$<1:app>>` 反例后复验；其余暂存区未发现实质问题。
- 2026-10-25：最终修复增量由同一 subagent 只读复核并独立执行夹具，确认无剩余实质问题。按用户明确授权提交、创建 PR，待当前 Ubuntu CI 通过后合并；nightly 与跨平台桌面包仍保持待验收。
- 2026-10-25：按代码域拆分 PR CI 选择与工具触发：Rust package/test、C++/CMake、QML 静态检查、QML CTest 和分层检查分别选择；C++ 与 QML CTest 使用独立标签。Rust harness 普通测试改动只选择 `caret-tests`；测试选择器自身改动则验证完整路由。三个 C++ lint 工具共用一个 runner，`format` 继续按变更文件运行，不参与编译。
- 2026-10-25：Nightly run 40206639630 暴露 Linux 覆盖率缺少 Fontconfig 前置、Pest/stacker 的外部汇编不受 Miri 支持，以及 macOS 系统框架泄漏缓存和 Linux Qt 6.11 QML loader 的 TSan 报告。修复分支补齐 Linux 前置、缩窄 Miri 到可运行的纯 Rust 测试、仅对已确认的 macOS 系统缓存使用 LSAN 抑制；Linux TSan 只排除 `Qml.ShellLoad` 并要求升级托管 Qt 后复查。修复后的三平台 Nightly 尚未运行，不据此标记验收完成。
- 2026-10-26：Nightly run 40209293853 通过 Miri、Linux workspace/native coverage 前置、Linux 与 Windows sanitizer、质量检查和 CPU 性能；覆盖率汇总未包含 `caret-tests --test document`，实际门槛为函数 77.89%、行 82.12%。将该公开集成测试纳入独立 coverage pass，并按 Cargo workspace 动态生成报告 package 范围，避免新增 crate 被白名单遗漏；增加存储错误路径、编辑状态和 FFI 成功路径断言以补齐可达行为。macOS `Qml.ShellLoad` 的 LSan 报告指向 Qt/CoreText/CoreGraphics 系统缓存，新增针对日志栈符号的有限抑制。待 PR 合并后的 Nightly 复验，不先标记任务完成。
- 2026-10-26：Nightly run 40211105819 通过 Rust 覆盖率、三平台 workspace tests、Miri、质量、Linux/Windows sanitizer、native coverage、macOS Objective-C++ 与性能样例。macOS sanitizer 剩余 8,824 字节 / 69 个分配，日志栈为 CoreText fallback/font variation、Apple FontParser glyph cache 和字体注册 XPC URL 解码，没有 Caret 代码分配帧；再补充对应系统符号抑制，需再次 Nightly 验收。
- 2026-10-26：subagent `nightly_fixes_review` 独立复核第二轮 LSAN 配置，要求移除类级泛匹配并仅保留日志中三个直接分配入口 `TDescriptorSource::CopyDescriptorUncached`、`TCGFontCache::CopyFontWithOpticalSizeVariation`、`XTCopyPropertiesForFontsMatchingRequest`；收窄后复核通过，无剩余实质发现。新 Nightly 仍须验证这些根对象抑制能否覆盖相关间接缓存。
- 2026-10-26：本机 macOS 27 sanitizer 验证发现 Qt 6.11 的 CoreText 路径使用不同于 macOS 26 的具体 cache 函数名。针对本机报告的 Qt font database / font engine 入口增加精确规则，使 ShellLoad 字体缓存报告从 17,812 字节降为 192 字节。`qt_apple_urlFromPossiblySecurityScopedURL` 的三组 64 字节栈来自 `QDirListing`→`QFactoryLoader`→`QPlatformIntegrationFactory` 插件目录扫描；它是 stripped Qt 下最近的导出栈帧，不是完整所有权证明。CoreFoundation preferences/XPC 栈仍不可符号化，未扩大匹配。subagent 复核认为保留剩余 192 字节为失败是合适的；需在 Nightly macOS runner 验证。
- 2026-10-26：PR #26 Ubuntu CI 全检查通过（run 40211041328）；PR #27 变更相关 Ubuntu 检查通过（run 40212390890）。合并后的 Nightly run 40212431385 全 12 个 job 通过，包括 macOS ASan/UBSan 与 TSan、Linux/Windows sanitizer、三平台 workspace tests、Rust 覆盖率 87.12% / 88.32%、quality、Miri、native coverage、Objective-C++ 和 CPU performance。PR #27 CI 与 Nightly 均未复现本机 macOS 27 未符号化的 192 字节 preferences/XPC 报告。Windows/Linux 桌面包证据仍待补齐，因此 006 继续保持 implemented，尚不标记 done。
- 2026-10-28：Nightly run 40530614729 暴露 5 类仍有效的问题（另有 clang-format 已随 PR #37 修复）。为避免 Ubuntu 包管理因网络长时间挂起，新增通用 apt composite action：APT 自身最多重试 3 次，HTTP(S) 请求限时 20 秒，每个 update/download-only 命令总时限 90 秒并在失败后重试一次；dpkg 安装阶段只用已下载包，不受硬杀超时影响。Linux GL 前置与 Qt GPU 的 `xvfb` 安装共用该入口。Qt GPU 缺少 `libxcb-cursor0` 已补齐。Miri 的 Fontique 配置随后改为启用实际 `fontconfig-dlopen` Cargo feature，而不是依赖底层构建环境变量。Windows ASan 在 import-order 检查前显式构建 QML load probe。IME 跨页用例改为按当前字体和行高测量页面内容高度，不再依赖重复 100 段。新增打开/保存、跨段粘贴、FFI 与端到端 session 行为覆盖；覆盖率 gate 通过后，还需 Nightly 远程验收。
- 2026-10-28：合并 PR #38 后的 CI 40545791759 与 Nightly 40545878360 / 40591289048 暴露仍有效问题。APT 字体包下载超过 90 秒上限；将单次 update/download 操作上限提高到 300 秒，保留有限重试和 dpkg 离线安装边界。Qt GPU job 的 apt 已成功安装 `libxcb-cursor0`，但 Qt 仍拒绝加载 xcb plugin；按 Qt 官方 X11 运行依赖补齐常见 XCB/XKB 库，并在 benchmark 前检查 `libqxcb.so` 的 `ldd` 输出，直接报告未解析库。Miri 之前只设置 yeslogic 构建脚本环境变量，未启用 Fontique 包装层 feature；现由 workspace 依赖统一启用 `fontconfig-dlopen` 并删除临时环境变量。Windows ASan 验证改由 CTest JSON manifest 获取 smoke test 的实际路径。对应回归测试与 CI/Nightly 复验待提交。
- 2026-10-28：为保留 85% 生产函数覆盖率门槛，审查并将多个大型私有单测模块拆到 `module/tests.rs`，入口使用 Rust 推荐的 `module.rs`，并保留 `#[cfg(test)] mod tests;` 私有访问。cargo-llvm-cov 默认路径过滤会忽略这些测试源文件；迁移前函数覆盖率 82.86%，此次全量整理后 87.07%，行覆盖率从 92.75% 到 91.82%（测试源码退出统计范围后按生产代码重新计算）。原测试仍全部执行，函数体没有删改。测试文件组织约定已写入测试与仓库结构规范。

## 完成摘要

本地实现与验证完成，保持 panta 的统一 format/lint/test 调度机制。GoogleTest 已由 016 独立完成三平台生产、自检与正式发布，006 已固定正式摘要并跑通默认消费路径。日常 Ubuntu CI 和三平台 Nightly 已有成功的远程证据，覆盖率和动态检查全部通过；Windows/Linux 桌面包仍待远程验证，因此本任务保持 implemented，不标记 done。
