# Web terminal font

`maple-mono-cn-iptools.woff2` is a glyph subset of Maple Mono NF CN Regular.
Maple Mono is maintained at <https://github.com/subframe7536/maple-font> and
is distributed under the SIL Open Font License 1.1; see `OFL.txt`.

The subset contains printable Latin-1, arrows, box drawing and block symbols,
plus all glyphs found in the shared core, UI, demo scenarios and Web shell. It
is generated from `MapleMono-NF-CN-Regular.ttf` with FontTools `pyftsubset`.
The full upstream font is intentionally not bundled because it is over 20 MiB.

When UI copy changes, regenerate the subset from the current source strings
and verify that `document.fonts.check('16px "Maple Mono CN iptools"')` remains
true in the browser test.
