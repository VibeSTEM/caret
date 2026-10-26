# 排版主题配置

本目录保存纳入版本控制的排版预设。每个预设包含稳定的 `profileId` 和 `formatVersion`。内置 `default.json` 为新建 `.cdoc` 文档提供正文默认字号与段落配置；新文档会把生效值写入 `globalSettings`。

正文默认字号以整数百分之一 pt 存在 `textStyle.fontSizeCentipoints`，当前默认是 1200（12 pt）。段落行距和间距存在 `paragraphLayout`。读取缺少相应全局设置的旧文档时，Rust 从内置预设补默认值；保存时将生效值写入 `.cdoc`，之后修改预设不会改变已保存文档的外观。期刊和出版物排版预设可以放在同一目录，并映射到相同的文档模型。
