# maploom-pdf-fonts

Font registration, Unicode text shaping and PDF font resources for Maploom.
`maploom-pdf` uses this crate through its `fonts` feature, enabled by default.
This crate depends on `maploom-core` and `pdf-writer`, independently of the map
renderer and MVT decoding.

`FontCatalog` borrows caller-supplied standalone, static TrueType font bytes.
`layout` applies grapheme-aware font fallback and connects `unicode-bidi` and
`rustybuzz` to PDF coordinates and Unicode clusters. `draw` writes label text and
halo outlines. `FontResources` preflights and allocates glyph encodings, shares
embedded fonts across forms, and writes the final resources into a caller-owned
PDF chunk. Call `finish` after preparing all forms and before finishing the PDF.

No fonts are bundled. Tests use `font-test-data` as a development dependency.
