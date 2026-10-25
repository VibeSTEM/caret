# 005 — Rust 文档与排版任务的库审查

审查日期：2026-10-25。范围为当前 021 实现及 022–027、009 的 Rust 工作。标准算法优先复用维护中的实现；依赖是否采用仍须满足 Caret 的分层、Unicode 版本锁定、性能和跨平台验收。

## 结论

| 能力 | 建议 | 依据与边界 |
| --- | --- | --- |
| `.cdoc` 媒体 Base64 | 已采用 `base64` 0.23.1 | 021 原来手写字符集、填充及规范尾随位验证。标准 `STANDARD` decoder 已执行严格填充与尾随位检查；`DecoderReader` 流式读入 sink 可复用验证而不分配完整解码副本。 |
| BCP 47 语法 | 暂保留 021 的校验器，不采用 `langtag` 1.1.0 | `langtag` 的借用式 RFC 5646 解析接口原本很适合保留原始标签；但实际用 `en-1901-1901` 坏输入调用 `LangTag::new` 时 crate 内部 panic，而不是返回错误。面对不可信 `.cdoc` 输入不能采用此版本。待上游修复后再用固定 RFC/BCP 47 语料复测。ICU4X `Locale` 也不适合直接代替：其文档说明当前拒绝超过三个字符的主语言标签，而 BCP 47 语法允许更长语言子标签。 |
| 扩展字素簇 | 022 采用 `unicode-segmentation` 候选，先锁定数据版本 | 当前 1.13.3 提供 UAX #29 并带 Unicode 17.0 数据；跨 Run 与行内环境的逻辑段落拼接/回映仍是 Caret 的领域适配，不应自己重写分段算法。 |
| 文本分析、塑形与 glyph/cluster 映射 | 024 已采用 Parley Engine 0.12；其底层直接使用 HarfRust，不另写 shaping/bidi/cluster 管线 | Engine 暴露 paragraph analysis、font selection、shaped runs、clusters、atoms 和 grapheme 映射，适合 Caret 自己控制段落和页面布局。Caret 已通过 locked 本地构建及 Unicode 17 bidi-character 全量 corpus；任意断点后的 reshaping 尚未完成，026 只能在 Atom/ShapedCluster 安全边界断行。Caret 保留自有文档模型和 UTF-8 位置映射。 |
| OpenType shaping | HarfRust 由 Parley Engine 带入；只有 API 缺口经原型证明后才直接依赖 | HarfRust 是 HarfBuzz 组织维护的 Rust 移植，使用 Fontations `read-fonts`，官方发布匹配 HarfBuzz 15。它不集成平台字体加载、Graphite 或 ICU；由 Parley/fontique 提供字体目录与 fallback。旧 `rustybuzz` 仓库已归档并指向 HarfRust，不选旧包。 |
| 字体发现/匹配 | 024 已采用 Parley Engine 搭配 fontique 0.12 | fontique 提供系统字体枚举、family/face 查询、fallback 和 SourceCache 字体数据缓存，且有平台后端；SystemUi generic family 用作默认主族，Emoji cluster 优先 Emoji generic family。系统字体重复加载是 no-op，`Collection::clear` 不会清系统字体。Caret 显式刷新时原子替换 Collection/SourceCache，不在编辑时扫描；Windows 另扫系统和当前用户标准 Fonts 目录，以绕过 0.12 不刷 DirectWrite 缓存集合的限制。字体族别名按名称缓存，canonical ID 名称用于报告实际族。非标准注册路径及 Windows 删除项需后续平台 smoke 确认。 |
| Unicode grapheme/bidi/line-break | 024 已采用 Unicode 17 一致性数据源并跑完整 corpus | `unicode-segmentation` 1.13.3、Parley Engine 0.12、ICU4X 2.3.0 分别承担 grapheme、bidi/shaping、line-break。仓库内 GraphemeBreakTest、BidiCharacterTest、LineBreakTest 三份 Unicode 17 完整语料均通过；LineBreakTest 的 SA 类在生产路径按 UAX #14 LB1 映射为 CM/AL，复杂脚本按语言启用词典 tailoring。 |
| 节点 ID | 暂不引入 UUID | 021 ID 只要求文档内唯一，当前进程 ID、纳秒时间和原子序列满足生成需求；随机 UUID 不会替代事务里的 ID 保留/拆分/合并规则。若未来需要跨文档合并或外部引用，再评估 UUID。 |
| 命令语言 | 当前无需增加解析器 | 009 是固定别名和候选匹配，不是用户可扩展语法；手写小状态逻辑更合适。Caret 的 DSL 已经使用 pest，若命令后来形成正式语法可复用该生态再评估。 |

## 执行记录

- 021 的 Base64 手写校验已在完成后的依赖审查中替换为严格流式解码；锁文件和 workspace manifest 一并更新。
- 对 `langtag` 1.1.0 的坏输入测试触发库内 panic，故没有引入该依赖；恢复 021 原有验证器。Unicode/排版依赖的选择需要对应固定语料和平台验证，不能为消除手写代码而引入版本不一致或未经验证的新栈。实施入口分别记在 021 与 024。
- 2026-10-25：024 实现确认 Parley Engine 0.12 + fontique 0.12；锁定的 API、字体目录与 Unicode 17 conformance 在 macOS/CoreText 本机通过。Parley Engine 的公开说明明确提示任意断行后的 reshaping 尚未完成，行布局应限制到 Atom/ShapedCluster 安全边界。Linux/Windows 字体 smoke 留待平台 CI。

## 参考资料

- [`base64` 0.23 文档](https://docs.rs/base64/0.23.1/base64/)、[`DecoderReader`](https://docs.rs/base64/0.23.1/base64/read/struct.DecoderReader.html)
- [`langtag` 文档](https://docs.rs/langtag/latest/langtag/)、[ICU4X Locale 解析说明](https://docs.rs/icu_locale/latest/icu_locale/struct.Locale.html)
- [`unicode-segmentation` 1.13.3](https://docs.rs/unicode-segmentation/1.13.3/unicode_segmentation/)、[`unicode-bidi` 0.3.18](https://docs.rs/unicode-bidi/0.3.18/unicode_bidi/)、[`icu_segmenter`](https://docs.rs/icu_segmenter/latest/icu_segmenter/)
- [`uniworld`](https://docs.rs/uniworld/latest/uniworld/)
- [Parley Engine 0.12](https://docs.rs/parley_engine/0.12.0/parley_engine/)、[Parley 文字排版栈](https://github.com/linebender/parley)、[HarfRust 官方仓库](https://github.com/harfbuzz/harfrust)、[fontique 0.12](https://docs.rs/fontique/0.12.0/fontique/)
- [`fontdb` 文档](https://docs.rs/fontdb/latest/fontdb/)
