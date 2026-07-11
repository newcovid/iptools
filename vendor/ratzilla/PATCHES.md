# iptools patches on Ratzilla 0.3.1

This directory starts from the crates.io `ratzilla` 0.3.1 source. iptools keeps
the version pin and carries only the following browser rendering fixes until
an upstream release contains equivalent behavior:

1. `CanvasBackendOptions::font` lets the application select its bundled
   terminal font. Non-ASCII clipping uses Unicode display width, so a CJK glyph
   receives a two-cell clip rectangle instead of losing its right half.
2. DOM drawing uses checked cell lookup during resize. A frame rendered with
   the previous dimensions may contain positions outside the newly rebuilt DOM
   grid; those transient cells are skipped and the next autoresized frame
   repaints the complete grid.

`cargo test --manifest-path vendor/ratzilla/Cargo.toml --lib` covers the pure
width and resize-index decisions. Browser behavior is additionally covered by
`crates/iptools-web/tests/e2e.mjs`.
