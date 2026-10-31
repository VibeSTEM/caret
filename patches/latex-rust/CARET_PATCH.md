# Caret patch record

This directory vendors the published `latex-rust` 3.0.1 crate as a Cargo path
patch. The source is the crates.io package whose registry checksum is
`2250132fbda62040ccba5ede9336add3257c76fe004f016d929926bf7530e9b6`. Keep the
upstream Apache-2.0/MIT licenses, `NOTICE`, and bundled STIX Two Math OFL files
with the copied source.

Caret-specific changes are limited to:

- `src/dim.rs`: checked conversion of finite dimensions to integer micro-units.
- `src/layout/engine.rs` and `src/layout/engine_walk.rs`: caller-shaped text,
  opaque external text boxes, and stable source-node provenance during layout.
  Provenance wrappers stay transparent to glyph/script/accent recognition;
  phantom layout preserves identities of visually suppressed child boxes.
- `src/layout/mod.rs` and `src/lib.rs`: public adapter API for shaping and
  provenance.
- `src/render/{egui/emit.rs,png/raster.rs,svg/render.rs}`: explicit errors for
  external text boxes, which those renderers cannot consume.

The source-span parser adapter is not implemented yet. Caret currently copies
the full formula range onto the syntax-tree root; nested `MathSyntaxNode` ranges
remain `None`. Do not treat nested source positions as available until parser
token and AST spans, including preprocessing offset mapping, are added and
covered by tests.

When updating this patch, compare it against the exact upstream 3.0.1 source,
preserve the original license and notices, and update this record with each
modified file and its reason.
