# 024 — 字体回退与文字塑形

- 状态：planned
- 依赖：022、023
- 优先级：P2
- 创建 / 更新：2026-10-25 / 2026-10-25

## 目标与背景

实现真实 shaping、字体覆盖/回退和双向文字映射，生成供行布局和命中使用的 glyph/cluster 数据。

## 必读

- [文字塑形](../architecture/editor/text-shaping.md)、[格式语义](../architecture/editor/text-formatting.md)、[增量排版](../architecture/editor/incremental-layout.md)
- [分层](../standards/layering.md)、[测试](../standards/testing.md)

## 范围与非目标

实现脚本/方向分段、字体回退、字重/字形解析、shaping、Unicode 换行机会和双向映射。遍历段落内的 Run/行内 EnvironmentNode，解析嵌套作用域后的有效样式再塑形；请求字体与实际替代字体分开呈现。分发字体时检查许可证。

不包含未获许可字体、Word 分页一致承诺和完整 OpenType 控件。

## 前置条件与待决策

022 的文本位置、023 的版心几何接口稳定。设计选定实际使用的 shaping API。

## 实施步骤

1. 评估锁定 Qt/font API 与平台可用能力。
2. 实现 shaping 输入、cluster 映射和回退。
3. 建立中英混排、组合字符、emoji、阿拉伯 RTL 和缺失字体样例。
4. 独立评审并复验。

## 预计改动

caret-layout 字体/shaping 模块、Qt 字体适配器、缓存键和跨平台测试。

## 验收标准

- [ ] 文本、选区、光标与 glyph cluster 映射稳定。
- [ ] 中英、RTL、组合字符和 emoji 无丢字、重字或无效位置。
- [ ] 缺失字体保留请求名并报告替代，不静默改文档。
- [ ] 未授权字体不进入安装包；字体实例变化会失效对应缓存。
- [ ] 独立评审与平台验证通过。

## 验证计划与结果

| 日期 | 环境 / 检查 | 预期 | 实际结果 |
| --- | --- | --- | --- |
| — | shaping/cluster fixtures | 位置映射和换行输入可复现 | 未执行 |
| — | macOS/Linux/Windows 字体 smoke | 请求字体和 fallback 行为一致 | 未执行 |

## 风险与回退

字体版本变化会改变度量与分页。固定测试字体 fixture，不承诺跨平台默认字体分页一致。

## 决策与工作记录

- 2026-10-25：从原 008 整体布局拆出文字塑形。

## 完成摘要

未完成。
