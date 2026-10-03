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

The form's BBox clips drawing to its local bounds. Opacity resources live inside
the form and consume no additional indirect references. `draw` saves/restores
graphics state, so colors, line widths and transforms do not leak into surrounding
content. Invoke it between paths/text objects, outside `BT`/`ET`. Resource names
are scoped to each containing page/form and must be registered by the caller.
The returned handle belongs to the document receiving the chunk; references must
be updated by the caller if the chunk is subsequently renumbered.

The drawing argument is `&[Layer]`. Construct typed `FillLayer` / `LineLayer`
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

Text, symbols,
circles, rasters, backgrounds, terrain, patterns, data-driven paint expressions
and other layer types are unsupported. Unsupported layer types and properties are rejected when deserializing `Layer`.
Invalid numeric paint values are rejected by the renderer. Input tiles should cover the
requested bounds. The renderer does not fetch, merge or deduplicate tiles.

Generate a document combining text, a main map and a second placement of the same map:

```sh
cargo run -p maploom-pdf --example render -- map.pdf
```
