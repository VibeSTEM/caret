# TeXmacs：结构环境与可定制定义

只研究公开文档中描述的模型和行为。不复制 TeXmacs 源码、样式文件、文档表达、测试或 Scheme 宏实现，也不加入依赖。

## 观察

- TeXmacs 把文档表示为有标签的树；节点语义和排版受当前 typesetting environment 影响。该环境可包含宏和环境变量，变量可在局部作用域改变。[内部架构](https://www.texmacs.org/tmdoc/devel/source/architecture.en.html)、[样式文件和宏](https://www.texmacs.org/tmweb/manual/webman-style.en.html)
- 样式包可以定义可复用宏、环境变量和定制钩子。用户可添加引文、定理等环境，并集中修改其渲染，不必逐处手工格式化。[结构化文本与环境](https://www.texmacs.org/tmdoc/main/text/man-structure.en.html)、[定理环境定制](https://www.texmacs.org/tmdoc/devel/style/customize/customize-theorems.en.html)
- 高层结构标签不只是外观宏。官方定制指南指出，`section` 还涉及子计数器重置、目录登记等副作用，因此提供专用渲染和回调钩子，而不建议用户重写整个标签。[通用定制原则](https://www.texmacs.org/tmdoc/devel/style/customize/customize-general.en.html)、[章节标签定制](https://www.texmacs.org/tmdoc/devel/style/customize/customize-sections.en.html)
- 文档树和宏展开参与排版后，屏幕物理位置到树路径光标的映射会变复杂，特别是遇到宏展开、换行和分页时。[内部架构](https://www.texmacs.org/tmdoc/devel/source/architecture.en.html)
- TeXmacs 的 D.R.D. 记录标签参数数量、子项可访问性等信息，用于编辑器默认结构操作和遍历；自定义标签可以推导这些属性，也能显式覆盖。[文档格式与 D.R.D.](https://www.texmacs.org/tmweb/manual/webman-format.en.html)

## 对 Caret 的启发

1. 分开**结构实例**与**可重用定义**。文档保留 `Section`、`Showcase` 等语义节点；定义负责标题/正文的样式组合和可定制呈现。章节计数、目录语义、子项归属不能只靠一个外观宏推导。
2. 支持组合和覆盖。Caret 首版采用受限声明式模板：只引用已登记的语义能力、格式环境和样式值，不执行用户代码、不在每次布局时展开成新的文档树。这样保留自由组合，同时让节点 ID、光标路径、撤销、缓存和未知能力处理保持可预测。
3. 结构类型仍有内置语义契约。用户可以基于内置类型创建/覆盖模板；创建新编号协议、目录副作用或新的子节点规则属于新增结构能力，需版本化，不能由未知宏静默决定。
4. 样式定义和实例设置应有明确的覆盖关系，并按度量/绘制/结构影响更新缓存代际。不能因为只改字色就复制内容树或重做无关布局。

## 范围限制

这是一份公开设计资料对照，不是 TeXmacs 源码评审，也没有验证其当前实现中的每种扩展边界。Caret 只采用上述模型经验，不承诺 TeXmacs 格式或宏语言兼容。
