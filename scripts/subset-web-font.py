#!/usr/bin/env python3
"""Build and verify the reproducible Maple Mono CN subset used by the Web demo."""

from __future__ import annotations

import argparse
import hashlib
import os
import unicodedata
from pathlib import Path

import fontTools
from fontTools import subset
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "crates/iptools-web/assets/fonts/maple-mono-cn-iptools.woff2"
SOURCE_SHA256 = "8b4f149beaead3eac78ffb84adf31513ebf06fe6dbc6e181ccd9debad3c70f1a"
SOURCE_VERSION = "Version 7.900"
FONTTOOLS_VERSION = "4.61.0"
TEXT_ROOTS = (
    ROOT / "crates/iptools-core/src",
    ROOT / "crates/iptools-ui/src",
    ROOT / "crates/iptools-demo",
    ROOT / "crates/iptools-web/src",
    ROOT / "crates/iptools-web/index.html",
    ROOT / "crates/iptools-web/manifest.webmanifest",
    ROOT / "assets/locales",
)
TEXT_SUFFIXES = {".rs", ".json", ".html", ".webmanifest"}
COMMON_ARROWS = {
    0x2190, 0x2191, 0x2192, 0x2193, 0x2194, 0x2195, 0x2196, 0x2197,
    0x2198, 0x2199, 0x219D, 0x219E, 0x21A0, 0x21A2, 0x21A3, 0x21A4,
    0x21A5, 0x21A6, 0x21A7, 0x21A9, 0x21AA, 0x21AD, 0x21B0, 0x21B1,
    0x21B2, 0x21B3, 0x21BE, 0x21C4, 0x21C6, 0x21C9, 0x21D0, 0x21D1,
    0x21D2, 0x21D3, 0x21D4, 0x21DA, 0x21DB, 0x21DE, 0x21DF, 0x21E4,
    0x21E5, 0x21E6, 0x21E7, 0x21E8, 0x21E9, 0x21EA,
}


def source_files() -> list[Path]:
    files: list[Path] = []
    for root in TEXT_ROOTS:
        if root.is_file():
            files.append(root)
        elif root.is_dir():
            files.extend(
                path
                for path in root.rglob("*")
                if path.is_file()
                and path.suffix in TEXT_SUFFIXES
                and "node_modules" not in path.parts
                and "dist" not in path.parts
            )
    return sorted(set(files))


def required_codepoints() -> set[int]:
    characters = set()
    for path in source_files():
        characters.update(path.read_text(encoding="utf-8"))

    characters.update(chr(codepoint) for codepoint in range(0x20, 0x7F))
    characters.update(chr(codepoint) for codepoint in range(0xA0, 0x100))
    characters.update(chr(codepoint) for codepoint in COMMON_ARROWS)
    characters.update(chr(codepoint) for codepoint in range(0x2500, 0x25A0))

    return {
        ord(character)
        for character in characters
        if character == " " or not unicodedata.category(character).startswith("C")
    }


def cmap(font_path: Path) -> set[int]:
    with TTFont(font_path, lazy=True) as font:
        return set(font.getBestCmap() or {})


def verify(font_path: Path, required: set[int]) -> None:
    missing = sorted(required - cmap(font_path))
    if missing:
        preview = ", ".join(f"U+{value:04X}" for value in missing[:20])
        raise SystemExit(f"{font_path} is missing {len(missing)} required glyphs: {preview}")
    print(f"font coverage passed: {font_path} contains {len(required)} required codepoints")


def build(source: Path, output: Path, required: set[int]) -> None:
    if fontTools.__version__ != FONTTOOLS_VERSION:
        raise SystemExit(
            f"fonttools {FONTTOOLS_VERSION} is required, found {fontTools.__version__}"
        )
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    if digest != SOURCE_SHA256:
        raise SystemExit(f"unexpected Maple Mono source SHA-256: {digest}")

    font = TTFont(source, recalcTimestamp=False)
    version = font["name"].getName(5, 3, 1)
    if version is None or str(version) != SOURCE_VERSION:
        raise SystemExit(f"unexpected Maple Mono source version: {version}")

    missing = sorted(required - set(font.getBestCmap() or {}))
    if missing:
        raise SystemExit(f"source font is missing {len(missing)} required codepoints")

    options = subset.Options()
    options.flavor = "woff2"
    options.layout_features = ["*"]
    options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
    options.name_languages = [0x409]
    options.recalc_timestamp = False
    options.canonical_order = True
    options.notdef_glyph = True
    options.notdef_outline = True
    options.recommended_glyphs = True

    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=required)
    subsetter.subset(font)
    output.parent.mkdir(parents=True, exist_ok=True)
    font.save(output, reorderTables=True)
    verify(output, required)
    print(f"generated {output} ({output.stat().st_size} bytes)")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--source",
        type=Path,
        default=Path(
            os.environ.get(
                "MAPLE_MONO_SOURCE",
                r"C:\Windows\Fonts\MapleMono-NF-CN-Regular.ttf",
            )
        ),
    )
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()

    required = required_codepoints()
    if args.verify_only:
        verify(args.output, required)
    else:
        build(args.source, args.output, required)


if __name__ == "__main__":
    main()
