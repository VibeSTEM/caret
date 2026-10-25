# Rust 文档与排版任务的库审查

审查日期：2026-10-25。范围为当前 021 实现及 022–027、009 的 Rust 工作。标准算法优先复用维护中的实现；依赖是否采用仍须满足 Caret 的分层、Unicode 版本锁定、性能和跨平台验收。

## 结论

| 能力 | 建议 | 依据与边界 |
| --- | --- | --- |
| `.cdoc` 媒体 Base64 | 已采用 `base64` 0.23.1 | 021 原来手写字符集、填充及规范尾随位验证。标准 `STANDARD` decoder 已执行严格填充与尾随位检查；`DecoderReader` 流式读入 sink 可复用验证而不分配完整解码副本。 |
| BCP 47 语法 | 后续评估 `langtag`，替换自维护语法状态机 | crate 声明实现 RFC 5646，`LangTag::new` 借用输入，覆盖 normal/private-use/grandfathered 类型；只用于语法解析，不规范化或改写保存的原始标签。加入前应以仓库固定的 BCP 47/RFC 用例集验证非法重复 variant/extension、private use 和 grandfathered 标签。ICU4X `Locale` 不适合直接代替：其文档说明当前拒绝超过三个字符的主语言标签，而 BCP 47 语法允许更长语言子标签。 |
| 扩展字素簇 | 022 采用 `unicode-segmentation` 候选，先锁定数据版本 | 当前 1.13.3 提供 UAX #29 并带 Unicode 17.0 数据；跨 Run 与行内环境的逻辑段落拼接/回映仍是 Caret 的领域适配，不应自己重写分段算法。 |
| 双向算法 | 024 评估 `unicode-bidi` 或统一 Unicode 栈 | `unicode-bidi` 提供 UAX #9，但当前 0.3.18 数据为 Unicode 16.0，与 Unicode 17 grapheme 数据不一致；不能未经 corpus 和版本决策就直接与上项混用。 |
| 行断 | 024 选定和 bidi/grapheme 同版的实现，不写自制规则 | ICU4X segmenter 对 UAX #14 有 15.1 稳定构造和 17.0 构造；17.0 构造当前标为 unstable。较新的 `uniworld` 声称将 bidi、UAX #14、UAX #29 统一到 Unicode 17，但版本为 0.2，采用前需检查 API 稳定性、数据许可、性能以及三套 UAX conformance corpus。 |
| OpenType shaping | 024 优先原型验证官方 HarfRust，不从零编写 shaper | HarfRust 是 HarfBuzz 组织维护的 Rust 移植，使用 Fontations `read-fonts`，文档报告接近 HarfBuzz 的 shaping 行为；仍需覆盖 Caret 所需的 cluster、字体实例和系统字体输入。旧 `rustybuzz` 仓库已归档并指向 HarfRust，不选旧包。 |
| 字体发现/匹配 | `fontdb` 作为目录/匹配候选，不直接承诺完整系统集成 | 它支持加载目录并查询字体，但文档列出非目标包括完整 fallback，字体类型限 TrueType，系统加载是扫描预定义目录而非调用 OS 字体 API。需测试 macOS/Windows/Linux 的用户字体目录、TTC/OTF 和平台 fallback；Qt/原生 API 可能更适合作为字体枚举适配层。 |
| 节点 ID | 暂不引入 UUID | 021 ID 只要求文档内唯一，当前进程 ID、纳秒时间和原子序列满足生成需求；随机 UUID 不会替代事务里的 ID 保留/拆分/合并规则。若未来需要跨文档合并或外部引用，再评估 UUID。 |
| 命令语言 | 当前无需增加解析器 | 009 是固定别名和候选匹配，不是用户可扩展语法；手写小状态逻辑更合适。Caret 的 DSL 已经使用 pest，若命令后来形成正式语法可复用该生态再评估。 |

## 执行记录

- 021 的 Base64 手写校验已在完成后的依赖审查中替换为严格流式解码；锁文件和 workspace manifest 一并更新。
- BCP 47 替换与 Unicode/排版依赖的选择需要对应固定语料和平台验证，不能为消除手写代码而引入版本不一致或未经验证的新栈。实施入口分别记在 021 与 024。

## 参考资料

- [`base64` 0.23 文档](https://docs.rs/base64/0.23.1/base64/)、[`DecoderReader`](https://docs.rs/base64/0.23.1/base64/read/struct.DecoderReader.html)
- [`langtag` 文档](https://docs.rs/langtag/latest/langtag/)、[ICU4X Locale 解析说明](https://docs.rs/icu_locale/latest/icu_locale/struct.Locale.html)
- [`unicode-segmentation` 1.13.3](https://docs.rs/unicode-segmentation/1.13.3/unicode_segmentation/)、[`unicode-bidi` 0.3.18](https://docs.rs/unicode-bidi/0.3.18/unicode_bidi/)、[`icu_segmenter`](https://docs.rs/icu_segmenter/latest/icu_segmenter/)
- [`uniworld`](https://docs.rs/uniworld/latest/uniworld/)
- [HarfRust 官方仓库](https://github.com/harfbuzz/harfrust)、[`fontdb` 文档](https://docs.rs/fontdb/latest/fontdb/)
