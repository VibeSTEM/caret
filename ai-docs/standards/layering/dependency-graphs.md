# 依赖图

更新日期：2026-10-30。本文集中展示 Rust crate 与 native 目标的允许依赖方向；层级规则和边界要求见[分层索引](../layering.md)。

```mermaid
flowchart TB
  subgraph L7["L7 组合根"]
    launcher["launcher"]
    cli["caret-cli"]
  end
  subgraph L6["L6 边界"]
    ffi["caret-ffi"]
  end
  subgraph L5["L5 应用"]
    app["caret-app"]
  end
  subgraph L4["L4 会话"]
    engine["caret-engine"]
  end
  subgraph L3["L3 派生"]
    layout["caret-layout"]
    structure["caret-structure"]
  end
  subgraph L2["L2 叶子"]
    doc["caret-doc"]
    dslc["caret-dslc"]
  end
  subgraph L1["L1 基础"]
    foundation["caret-foundation"]
    build["caret-build"]
    dslcore["caret-dsl-core"]
    math["caret-math"]
  end

  launcher --> ffi
  launcher --> build
  launcher --> dslcore
  cli --> app
  cli --> foundation
  launcher ~~~ cli
  ffi --> app
  ffi --> foundation
  app --> engine
  app --> dslcore
  app --> doc
  app --> foundation
  engine --> doc
  engine --> layout
  engine --> structure
  layout --> doc
  structure --> doc
  dslc --> dslcore
  layout ~~~ structure
  doc ~~~ dslc
  foundation ~~~ build
  build ~~~ dslcore
  dslcore ~~~ math
```

```mermaid
flowchart TB
  subgraph N4["N4 入口"]
    entry["native 入口"]
  end
  subgraph N3["N3 界面"]
    qml["QML"]
  end
  subgraph N2["N2 界面能力"]
    vm["ViewModel"]
    canvas["画布"]
  end
  subgraph N1["N1 适配"]
    adapter["caret::adapters"]
  end
  staticlib["caret-ffi staticlib"]

  entry --> qml
  entry --> vm
  entry --> canvas
  entry --> adapter
  entry --> staticlib
  qml --> vm
  qml --> canvas
  vm --> adapter
  vm --> staticlib
  canvas --> adapter
  vm ~~~ canvas
```

`caret-foundation`、`caret-build`、`caret-dsl-core` 没有指向其他 workspace crate 的边。`caret-doc`、布局、结构索引和会话在需要时可以指向 `caret-foundation`，图里不再重复这几条边。画布和 ViewModel 互不依赖，由 native 入口把它们接在一起。逐帧绘制、命中和输入留在画布里；跨到 Rust 的是命令和页面快照，不是每个指针事件。
