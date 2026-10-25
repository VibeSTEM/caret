# 005 — Rust 文档与排版任务的库审查

审查日期：2026-10-25。范围为当前 021 实现及 022–027、009 的 Rust 工作。标准算法优先复用维护中的实现；依赖是否采用仍须满足 Caret 的分层、Unicode 版本锁定、性能和跨平台验收。

## 结论

| 能力 | 建议 | 依据与边界 |
| --- | --- | --- |
| `.cdoc` 媒体 Base64 | 已采用 `base64` 0.23.1 | 021 原来手写字符集、填充及规范尾随位验证。标准 `STANDARD` decoder 已执行严格填充与尾随位检查；`DecoderReader` 流式读入 sink 可复用验证而不分配完整解码副本。 |
| BCP 47 语法 | 暂保留 021 的校验器，不采用 `langtag` 1.1.0 | `langtag` 的借用式 RFC 5646 解析接口原本很适合保留原始标签；但实际用 `en-1901-1901` 坏输入调用 `LangTag::new` 时 crate 内部 panic，而不是返回错误。面对不可信 `.cdoc` 输入不能采用此版本。待上游修复后再用固定 RFC/BCP 47 语料复测。ICU4X `Locale` 也不适合直接代替：其文档说明当前拒绝超过三个字符的主语言标签，而 BCP 47 语法允许更长语言子标签。 |
| 扩展字素簇 | 022 采用 `unicode-segmentation` 候选，先锁定数据版本 | 当前 1.13.3 提供 UAX #29 并带 Unicode 17.0 数据；跨 Run 与行内环境的逻辑段落拼接/回映仍是 Caret 的领域适配，不应自己重写分段算法。 |
| 文本分析、塑形与 glyph/cluster 映射 | 024 优先评估 Parley Engine 0.12；其底层直接使用 HarfRust，不另写 shaping/bidi/cluster 管线 | Engine 暴露 paragraph analysis、font selection、shaped runs、clusters、atoms 和 grapheme 映射，适合 Caret 自己控制段落和页面布局。公开文档说明任意断行后的 reshaping 尚在开发中，当前只能在 Atom/ShapedCluster 边界断行。采用前要在 locked 本地构建验证 API、Unicode 数据来源、用例行为及许可证；保留自有文档模型和 Caret 的文档位置映射适配。 |
| OpenType shaping | HarfRust 由 Parley Engine 带入；只有 API 缺口经原型证明后才直接依赖 | HarfRust 是 HarfBuzz 组织维护的 Rust 移植，使用 Fontations `read-fonts`，官方发布匹配 HarfBuzz 15。它不集成平台字体加载、Graphite 或 ICU；由 Parley/fontique 提供字体目录与 fallback。旧 `rustybuzz` 仓库已归档并指向 HarfRust，不选旧包。 |
| 字体发现/匹配 | 024 优先评估 Parley Engine 搭配 fontique 0.12 | fontique 提供系统字体枚举、family/face 查询、fallback 和 SourceCache 字体数据缓存，且有平台后端；系统字体重复加载是 no-op，`Collection::clear` 不会清系统字体。因此显式刷新需要原子替换新 Collection/SourceCache，而非每次编辑扫描。要在本机与 CI 验证 user-level 字体覆盖、刷新、TTC/OTF 和各平台行为；Qt 的 QFontDatabase 可列 family，但其接口不能作为 HarfRust 字体源数据目录的完整替代。 |
| Unicode grapheme/bidi/line-break | 024 需选用同版数据并锁同一套 conformance corpus；Parley Engine 0.12 是首选候选但版本来源待核验 | 当前 `unicode-segmentation` 1.13.3 为 Unicode 17，`unicode-bidi` 0.3.18 为 Unicode 16；ICU4X 2.3 对 grapheme 是 17、line-break 有 15.1 稳定 API 与 17.0 unstable API，不能简单拼接这些版本。`uniworld` 0.2 宣称提供统一 Unicode 17，但项目仅 13 commits、公开使用/维护证据有限，不作为首选核心依赖。Parley Engine 的集成算法和数据表须跑官方三套 corpus、核对 UCD 版本后再决定是否让它同时成为 022 编辑边界的唯一来源。 |
| 节点 ID | 暂不引入 UUID | 021 ID 只要求文档内唯一，当前进程 ID、纳秒时间和原子序列满足生成需求；随机 UUID 不会替代事务里的 ID 保留/拆分/合并规则。若未来需要跨文档合并或外部引用，再评估 UUID。 |
| 命令语言 | 当前无需增加解析器 | 009 是固定别名和候选匹配，不是用户可扩展语法；手写小状态逻辑更合适。Caret 的 DSL 已经使用 pest，若命令后来形成正式语法可复用该生态再评估。 |

## 执行记录

- 021 的 Base64 手写校验已在完成后的依赖审查中替换为严格流式解码；锁文件和 workspace manifest 一并更新。
- 对 `langtag` 1.1.0 的坏输入测试触发库内 panic，故没有引入该依赖；恢复 021 原有验证器。Unicode/排版依赖的选择需要对应固定语料和平台验证，不能为消除手写代码而引入版本不一致或未经验证的新栈。实施入口分别记在 021 与 024。
- 2026-10-25：继续审查后将 Parley Engine 0.12 + fontique 0.12 列为 024 首选原型；公开 API 覆盖了塑形、font fallback、clusters/atoms 与段落分析。依赖源码已通过 Cargo 获取，待在本仓库验证锁定构建、Unicode 数据版本与字体目录行为。Parley Engine 的公开说明明确提示任意断行后的 reshaping 尚未完成。

## 参考资料

- [`base64` 0.23 文档](https://docs.rs/base64/0.23.1/base64/)、[`DecoderReader`](https://docs.rs/base64/0.23.1/base64/read/struct.DecoderReader.html)
- [`langtag` 文档](https://docs.rs/langtag/latest/langtag/)、[ICU4X Locale 解析说明](https://docs.rs/icu_locale/latest/icu_locale/struct.Locale.html)
- [`unicode-segmentation` 1.13.3](https://docs.rs/unicode-segmentation/1.13.3/unicode_segmentation/)、[`unicode-bidi` 0.3.18](https://docs.rs/unicode-bidi/0.3.18/unicode_bidi/)、[`icu_segmenter`](https://docs.rs/icu_segmenter/latest/icu_segmenter/)
- [`uniworld`](https://docs.rs/uniworld/latest/uniworld/)
- [Parley Engine 0.12](https://docs.rs/parley_engine/0.12.0/parley_engine/)、[Parley 文字排版栈](https://github.com/linebender/parley)、[HarfRust 官方仓库](https://github.com/harfbuzz/harfrust)、[fontique 0.12](https://docs.rs/fontique/0.12.0/fontique/)
- [`fontdb` 文档](https://docs.rs/fontdb/latest/fontdb/)
