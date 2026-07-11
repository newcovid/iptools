# Web terminal font

`maple-mono-cn-iptools.woff2` is a glyph subset of Maple Mono NF CN Regular
7.900. The locked source SHA-256 is
`8b4f149beaead3eac78ffb84adf31513ebf06fe6dbc6e181ccd9debad3c70f1a`.
Maple Mono is maintained at <https://github.com/subframe7536/maple-font> and
is distributed under the SIL Open Font License 1.1; see `OFL.txt`.

The subset contains printable Latin-1, arrows, box drawing and block symbols,
plus all glyphs found in the shared core, UI, demo scenarios and Web shell. It
is generated from `MapleMono-NF-CN-Regular.ttf` with FontTools 4.61.0:

```text
python -m pip install "fonttools[woff]==4.61.0"
python scripts/subset-web-font.py --source /path/to/MapleMono-NF-CN-Regular.ttf
python scripts/subset-web-font.py --verify-only
```
The full upstream font is intentionally not bundled because it is over 20 MiB.

When UI copy changes, regenerate the subset from the current source strings
and verify that `document.fonts.check('16px "Maple Mono CN iptools"')` remains
true in the browser test.
