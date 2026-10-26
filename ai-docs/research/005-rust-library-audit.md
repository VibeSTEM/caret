# 005 — Rust 文档与排版任务的库审查

审查日期：2026-10-25。范围为当前 021 实现及 022–027、009 的 Rust 工作。标准算法优先复用维护中的实现；依赖是否采用仍须满足 Caret 的分层、Unicode 版本锁定、性能和跨平台验收。

## 结论

| 能力 | 建议 | 依据与边界 |
| --- | --- | --- |
| `.cdoc` 媒体 Base64 | 已采用 `base64` 0.23.1 | 021 原来手写字符集、填充及规范尾随位验证。标准 `STANDARD` decoder 已执行严格填充与尾随位检查；`DecoderReader` 流式读入 sink 可复用验证而不分配完整解码副本。 |
| BCP 47 语法 | 暂保留 021 的校验器，不采用 `langtag` 1.1.0 | `langtag` 的借用式 RFC 5646 解析接口原本很适合保留原始标签；但实际用 `en-1901-1901` 坏输入调用 `LangTag::new` 时 crate 内部 panic，而不是返回错误。面对不可信 `.cdoc` 输入不能采用此版本。待上游修复后再用固定 RFC/BCP 47 语料复测。ICU4X `Locale` 也不适合直接代替：其文档说明当前拒绝超过三个字符的主语言标签，而 BCP 47 语法允许更长语言子标签。 |
| 扩展字素簇 | 022 采用 `unicode-segmentation` 候选，先锁定数据版本 | 当前 1.13.3 提供 UAX #29 并带 Unicode 17.0 数据；跨 Run 与行内环境的逻辑段落拼接/回映仍是 Caret 的领域适配，不应自己重写分段算法。 |
| 文本分析、塑形与 glyph/cluster 映射 | 027 采用 Parley 0.12 高层 `Layout`，其内部通过 Parley Engine 与 HarfRust；不另写 shaping/bidi/cluster 管线 | 高层 Layout 同时提供分析、字体选择、塑形、行布局以及 caret/hit-test/selection 几何。Caret 以 ICU4X 覆盖断点并在适配层限制 EGC 光标边界，保留自有文档模型、UTF-8 位置映射、行距和分页策略。低层引擎仅作传递依赖。 |
| OpenType shaping | HarfRust 由 Parley Engine 带入；只有 API 缺口经原型证明后才直接依赖 | HarfRust 是 HarfBuzz 组织维护的 Rust 移植，使用 Fontations `read-fonts`，官方发布匹配 HarfBuzz 15。它不集成平台字体加载、Graphite 或 ICU；由 Parley/fontique 提供字体目录与 fallback。旧 `rustybuzz` 仓库已归档并指向 HarfRust，不选旧包。 |
| 字体发现/匹配 | 024 已采用 Parley Engine 搭配 fontique 0.12 | fontique 提供系统字体枚举、family/face 查询、fallback 和 SourceCache 字体数据缓存，且有平台后端；SystemUi generic family 用作默认主族，Emoji cluster 优先 Emoji generic family。系统字体重复加载是 no-op，`Collection::clear` 不会清系统字体。Caret 显式刷新时原子替换 Collection/SourceCache，不在编辑时扫描；Windows 另扫系统和当前用户标准 Fonts 目录，以绕过 0.12 不刷 DirectWrite 缓存集合的限制。字体族别名按名称缓存，canonical ID 名称用于报告实际族。非标准注册路径及 Windows 删除项需后续平台 smoke 确认。 |
| Unicode grapheme/bidi/line-break | 024 建立 Unicode 17 数据基线；027 以 Parley 高层 Layout 接管段落布局 | `unicode-segmentation` 1.13.3、Parley 0.12、ICU4X 2.3.0 分别负责字素、双向/塑形/布局和行断机会。仓库内 GraphemeBreakTest、BidiCharacterTest、LineBreakTest 三份 Unicode 17 语料均通过；LineBreakTest 的 SA 类在生产路径按 UAX #14 LB1 映射为 CM/AL，复杂脚本按语言启用词典 tailoring。 |
| 节点 ID | 暂不引入 UUID | 021 ID 只要求文档内唯一，当前进程 ID、纳秒时间和原子序列满足生成需求；随机 UUID 不会替代事务里的 ID 保留/拆分/合并规则。若未来需要跨文档合并或外部引用，再评估 UUID。 |
| 命令语言 | 当前无需增加解析器 | 009 是固定别名和候选匹配，不是用户可扩展语法；手写小状态逻辑更合适。Caret 的 DSL 已经使用 pest，若命令后来形成正式语法可复用该生态再评估。 |

## 执行记录

- 021 的 Base64 手写校验已在完成后的依赖审查中替换为严格流式解码；锁文件和 workspace manifest 一并更新。
- 对 `langtag` 1.1.0 的坏输入测试触发库内 panic，故没有引入该依赖；恢复 021 原有验证器。Unicode/排版依赖的选择需要对应固定语料和平台验证，不能为消除手写代码而引入版本不一致或未经验证的新栈。实施入口分别记在 021 与 024。
- 2026-10-25：024 实现确认 Parley Engine 0.12 + fontique 0.12；锁定的 API、字体目录与 Unicode 17 conformance 在 macOS/CoreText 本机通过。Parley Engine 的公开说明明确提示任意断行后的 reshaping 尚未完成，行布局应限制到 Atom/ShapedCluster 安全边界。Linux/Windows 字体 smoke 留待平台 CI。
- 2026-10-26：初始建议是先评估 Parley 0.12 高层 `Layout`，避免它无法消费低层 `ShapedParagraph` 后导致长期重复 shaping。原型验证通过后，决策更新为将高层 `Layout` 作为后续统一段落排版、光标/命中和 selection geometry 的实现基线；Caret 保留文档模型、字体规则、ICU 断点、行距与分页策略，不另造通用文本布局管线。Parley 0.12 在不同样式 Run 中的组合附加符边界仍须由 Caret 适配层约束，未完成适配前不能把探针当作生产实现。
- 2026-10-26：高层布局探针确认 Parley 可消费 Caret ICU 断点，并能给出混合方向文本的光标/命中；在组合附加符与基字符属于不同样式 Run 时，Parley 0.12 会暴露字素内部的视觉光标停靠点。`FontCatalog` 现拥有单一 Parley `FontContext`，Engine 与高层 Layout 共用字体集合和 SourceCache，避免重复目录及缓存。接入仍需覆盖显式字体偏好/fallback、Emoji/ZWJ、换行/Atom、selection 几何和跨 Run 字素适配。
- 2026-10-26：最初的短文本与 1 MiB 对照把 Caret ICU 断点生成纳入计时，却复用 Parley 预计算的断点；该组数据不公平，作废，不用于决策。修正后的临时 Criterion 对照在双方计时路径都包含 Unicode 断点计算、塑形与段落排版；10 MiB 英文单段落在计时前校验了逐行 UTF-8 范围完全一致。release、Rust 1.99.0、`aarch64-apple-darwin`（macOS ARM64，系统未报告具体机型），10 样本、1 s 预热、2 s 目标测量：Caret 2.4026 s，Parley 1.8044 s，Parley 快约 24.9%。Criterion 因每次运行耗时长，自动延长了采样时间。该样本只覆盖单个 10 MiB 英文段落，不包括多段分页、FFI、字体目录初始化或内存测量，也不构成跨平台性能保证；结果支持采用高层布局作为实现基线，实际切换仍需功能验证。临时 harness 与 `target/criterion` 产物已清理；Criterion 已是 workspace 依赖并由 DSL 性能基准使用。
- 2026-10-26：生产段落与文档布局已切换到 Parley 高层 Layout；删除旧 `TextShaper`、自定义 FontSelector 和探针代码。Caret 样式按 Run 字节范围送入 Parley，跨 Run/环境的 EGC 边界经命中/光标/选区适配保护；分页继续消费 Parley 同源行范围及 Caret 行距计算结果。断点 override 使用紧凑位集，脚本继承为线性扫描，字体目录 generation 变化时重建 Parley `LayoutContext`。`caret-layout` 55 项、`caret-engine` 41 项通过，两个 crate 的全 target Clippy 通过。Qt scene graph 尚未消费段落 glyph runs，故此记录不宣称 027 的完整画布验收已经完成。

## 参考资料

- [`base64` 0.23 文档](https://docs.rs/base64/0.23.1/base64/)、[`DecoderReader`](https://docs.rs/base64/0.23.1/base64/read/struct.DecoderReader.html)
- [`langtag` 文档](https://docs.rs/langtag/latest/langtag/)、[ICU4X Locale 解析说明](https://docs.rs/icu_locale/latest/icu_locale/struct.Locale.html)
- [`unicode-segmentation` 1.13.3](https://docs.rs/unicode-segmentation/1.13.3/unicode_segmentation/)、[`unicode-bidi` 0.3.18](https://docs.rs/unicode-bidi/0.3.18/unicode_bidi/)、[`icu_segmenter`](https://docs.rs/icu_segmenter/latest/icu_segmenter/)
- [`uniworld`](https://docs.rs/uniworld/latest/uniworld/)
- [Parley Engine 0.12](https://docs.rs/parley_engine/0.12.0/parley_engine/)、[Parley 文字排版栈](https://github.com/linebender/parley)、[HarfRust 官方仓库](https://github.com/harfbuzz/harfrust)、[fontique 0.12](https://docs.rs/fontique/0.12.0/fontique/)
- [`fontdb` 文档](https://docs.rs/fontdb/latest/fontdb/)
