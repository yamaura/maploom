# maploom-pdf

Draw vector maps into a caller-owned `pdf-writer` document. `write_map` adds a
self-contained Form XObject to a `pdf_writer::Chunk` (including `Pdf` through
Deref). It creates **no pages or document structure**. The caller chooses object
IDs, page size, resource names, placement and when to finish the PDF.

```rust
use maploom_pdf::{LngLatBounds, MapViewport, TileSet, write_map};
use pdf_writer::{Content, Name, Pdf, Rect, Ref};

let mut pdf = Pdf::new();
let map = write_map(
    &mut pdf,
    Ref::new(10), // an unused reference allocated by the application
    MapViewport::new(400.0, 300.0, LngLatBounds::world(), 0.0),
    &[], // &[Layer]: Maploom drawing layers
    &TileSet::default(), // empty here; use TileSet::decode for MVT inputs
)?;

// Normal pdf-writer document setup, owned by the application.
pdf.catalog(Ref::new(1)).pages(Ref::new(2));
pdf.pages(Ref::new(2)).kids([Ref::new(3)]).count(1);
{
    let mut page = pdf.page(Ref::new(3));
    page.parent(Ref::new(2))
        .media_box(Rect::new(0.0, 0.0, 595.0, 842.0))
        .contents(Ref::new(4));
    page.resources().x_objects().pair(Name(b"Map"), map.id());
}

let mut content = Content::new();
// Bottom-left and top-right corners in the current PDF user space.
map.draw(&mut content, Name(b"Map"), Rect::new(40.0, 200.0, 440.0, 500.0))?;
// The same map can be reused elsewhere, or on another page after registering it.
map.draw(&mut content, Name(b"Map"), Rect::new(450.0, 40.0, 550.0, 115.0))?;
let content = content.finish();
pdf.stream(Ref::new(4), &content);
pdf.merge_limits(content.limits());
let bytes = pdf.finish();
assert!(bytes.starts_with(b"%PDF-"));
# Ok::<(), maploom_pdf::PdfRenderError>(())
```

`MapViewport` describes the map's local dimensions and geographic view, not a
PDF page. `MapXObject::draw` fits this rectangle into the target area with uniform
scaling and centers it. Existing transforms and clipping apply. Widths and
symbols scale with the form; for exact physical line widths, use a viewport the
same size as the destination. Use `Content::transform` and `Content::x_object`
directly when you need a different placement policy.

The form's BBox clips drawing to its local bounds. Each input tile is also clipped
to its XYZ footprint, preventing buffered geometry from overlapping adjacent
tiles and darkening translucent fills or strokes. Supply neighboring tiles at
the same zoom; clipping does not deduplicate overlapping parent/child tiles.
Opacity resources live inside
the form and consume no additional indirect references. `draw` saves/restores
graphics state, so colors, line widths and transforms do not leak into surrounding
content. Invoke it between paths/text objects, outside `BT`/`ET`. Resource names
are scoped to each containing page/form and must be registered by the caller.
The returned handle belongs to the document receiving the chunk; references must
be updated by the caller if the chunk is subsequently renumbered.

The drawing argument is `&[Layer]`. Construct typed `FillLayer` / `LineLayer` / `TextLayer`
values (re-exported here with `FillPaint`, `LinePaint`, `Number`, `Rgb`, etc.) or
read Maploom layer JSON with Serde. `Layer` describes drawing style;
PDF visibility groups are independently controlled by the application.

No browser, tile server, archive format or application data is required.

## Toggleable PDF layers

Optional Content Groups (OCGs) are created by the application using `pdf-writer`.
To toggle parts of the map independently, call `write_map` for each drawing layer
or contiguous group of drawing layers. All calls can share a single decoded `TileSet` and
the same `MapViewport` and placement rectangle. Wrap each `draw` invocation in
an OCG marked-content block:

```rust
# use maploom_pdf::{LngLatBounds, MapViewport, TileSet, write_map};
# use pdf_writer::{Content, Name, Chunk, Rect, Ref};
# let mut chunk = Chunk::new();
# let map = write_map(&mut chunk, Ref::new(1), MapViewport::new(400.0, 300.0, LngLatBounds::world(), 0.0), &[], &TileSet::default())?;
# let mut content = Content::new();
// Register RoadsLayer in page Resources/Properties and Roads in Resources/XObject.
content.begin_marked_content_with_properties(Name(b"OC"))
    .properties_named(Name(b"RoadsLayer"));
map.draw(&mut content, Name(b"Roads"), Rect::new(40.0, 200.0, 440.0, 500.0))?;
content.end_marked_content();
# Ok::<(), maploom_pdf::PdfRenderError>(())
```

The caller also registers OCGs and their default visibility/order in the catalog's
`OCProperties`. Preserve drawing order: nonadjacent drawing layers can share the same OCG
without being moved next to each other. A form containing several drawing layers toggles
as a whole when wrapped this way. See `examples/layers.rs` for complete OCG setup.

Decode inputs once with `TileSet::decode` and reuse them across `write_map` calls.
Use `MvtTileInput::new(z, x, y, bytes)` for unnamed tiles, or `.with_source("basemap")`
to match a drawing layer's source. A drawing layer without a source matches its MVT source-layer in every
input tile; a named source matches only tiles carrying that exact source name.
`source_layer` means the MVT layer name, without rewriting it.

Supported: polygon fills (including holes), line and polygon outlines, constant
hex RGB colors (`#rgb`, `#rrggbb`), opacity, line width/cap/join/dashes, filters,
zoom limits, visibility and numeric `step`/linear `interpolate` on zoom.
Widths are PDF points (1/72 inch); dash lengths are multiples of line width.
Bounds use Web Mercator, preserve aspect ratio and are centered in the map viewport.
Antimeridian-crossing bounds are unsupported; split them before rendering.

Icons, line-following labels, automatic text wrapping, rotated text,
circles, rasters, backgrounds, terrain, patterns, data-driven paint expressions
and other layer types are unsupported. Unsupported layer types and properties are rejected when deserializing `Layer`.
Invalid numeric paint values are rejected by the renderer. Input tiles should cover the
requested bounds. The renderer does not fetch, merge or deduplicate tiles.

Generate a document combining page text, a main map and a second placement of the same map:

```sh
cargo run -p maploom-pdf --example render -- map.pdf
```

## Validating styles before applying them

`validate_layers(&layers)` checks IDs, zoom ranges, and all numeric paint values
(including every linear/step stop and hidden layers) without tiles or a PDF
chunk. Use it when accepting an edited style. `write_map` performs the same
validation before writing anything. Invalid values in currently invisible layers
are rejected too, so changing zoom or visibility cannot activate an invalid style.

## Unicode point labels

`Layer::Text(TextLayer)` draws selectable PDF text at Point/MultiPoint features.
Font support is provided by `maploom-pdf-fonts` through the `fonts` feature,
enabled by default. `FontCatalog` and `FontError` are re-exported here.
To render fill/line geometry without font dependencies:

```toml
maploom-pdf = { version = "0.2.0", default-features = false }
```

With `fonts` disabled, visible text layers return an error before modifying the
PDF. `PdfMapWriter::new` takes only the chunk and reference cursor.
The examples use PDF-standard Helvetica and require no font files.

The caller supplies font bytes and exact family names. There is no filesystem,
network or system-font lookup in this crate. Use a document-scoped `PdfMapWriter`
to allocate forms and share each embedded font across all maps in that writer:

```rust
# #[cfg(feature = "fonts")]
# {
use maploom_pdf::{FontCatalog, Layer, LngLatBounds, MapViewport, PdfMapWriter,
                  TextField, TextLayer, TileSet};
use pdf_writer::{Chunk, Ref};

let mut fonts = FontCatalog::new();
// Register before creating the writer:
// fonts.register("Noto Sans", include_bytes!("NotoSans-Regular.ttf"))?;
let mut labels = TextLayer::new("airports", "airports", TextField::property("iata_code"));
labels.layout.fonts = vec!["Noto Sans".into()];
labels.layout.size = 12.0.into();
labels.paint.halo_width = 1.0.into();
let mut chunk = Chunk::new();
let mut next = Ref::new(10); // all references >= 10 must be available
let mut writer = PdfMapWriter::new(&mut chunk, &mut next, &fonts);
let map = writer.write_map(
    MapViewport::new(400.0, 300.0, LngLatBounds::world(), 0.0),
    &[Layer::Text(labels)], &TileSet::default(),
)?;
assert_eq!(map.id(), Ref::new(10)); // no visible labels, so no font needed here
# }
# Ok::<(), maploom_pdf::PdfRenderError>(())
```

Use one writer per document to share embedded fonts across maps. `writer.finish()`
or dropping the writer finalizes font resources; continue allocating with the
same reference cursor afterward. Forms keep their own font resources.
`write_map` is a convenience for geometry without a font catalog; visible text
through it reports a missing-font error.

The font catalog accepts standalone, static TrueType fonts with `glyf` outlines.
Extract collections and instantiate variable fonts before registration. CFF
outlines are unsupported. The library embeds complete fonts, without subsetting
or compression, and shares those bytes per writer; provide pre-subset fonts when
PDF size matters. Family fallback is ordered and keeps each grapheme together.
Missing families, unsupported glyphs and text-expression errors are explicit
errors. All shaping/validation completes before the chunk or cursor changes.

Text is shaped with Rustybuzz, split into script runs, and reordered using the
Unicode bidi algorithm. Embedded Type0 fonts map each shaped cluster, including ligatures, back to
its Unicode sequence. Distinct text sequences sharing an outline get distinct
CIDs; this also works in readers that ignore `ActualText`. Marked
`ActualText` retains each label's logical text through ligatures and bidi in PDF
readers that support it. Text remains text rather than outlines or images.
Support depends on the supplied font's glyph coverage and shaping tables.

Text sizes, halo widths, and opacity support the same zoom `Number` values as
geometry. Sizes and padding are PDF points; offsets and line height are ems.
Nine anchors, RGB colors, explicit newlines, filters, source selection, zoom
ranges and visibility are supported. There is no automatic wrapping. Labels
are shaped and placed independently; this is not a paragraph layout engine.

Collision placement is deterministic: earlier layers and earlier input features
win. `text-allow-overlap` skips collision rejection, while
`text-ignore-placement` prevents a label from blocking later ones. The collision
space spans the entire map viewport and resets per `write_map` call. Splitting
labels across separate forms does not share placement state.

For buffered MVT points, only anchors in `[0, extent)` belong to that tile; the
label itself can cross tile boundaries and is clipped only by the form BBox.
Supply neighboring tiles at a single zoom and avoid duplicate tile inputs.
MapLibre and PDF can choose different labels because placement algorithms and
font metrics differ. MapLibre imports require explicit `text-max-width: 0`.

Tests use `font-test-data` as a development dependency; no font binaries are bundled.
