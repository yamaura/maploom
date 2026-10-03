# maploom-core

The library depends only on `serde`; `serde_json` is used in examples and tests.

Typed `Layer::Fill(FillLayer)` and `Layer::Line(LineLayer)` values for map
drawing. These are drawing layers, independent of PDF optional content groups.
A slice of layers is drawn in order.

```rust
use maploom_core::{Layer, LineLayer, LineCap, Rgb};

let mut roads = LineLayer::new("road-stroke", "roads");
roads.source = Some("basemap".into());
roads.paint.color = Rgb::new(51, 102, 153);
roads.paint.width = 2.0.into();
roads.layout.cap = LineCap::Round;
let layers = vec![Layer::Line(roads)];

// Serialize the drawing rules, or pass the typed slice to a renderer.
let json = serde_json::to_value(&layers)?;
assert_eq!(json[0]["id"], "road-stroke");
assert_eq!(json[0]["type"], "line");
assert_eq!(json[0]["source-layer"], "roads");
assert_eq!(json[0]["paint"]["line-color"], "#336699");
# Ok::<(), serde_json::Error>(())
```

Maploom layer JSON can be read with Serde:

```rust
use maploom_core::Layer;
let layer: Layer = serde_json::from_str(r##"{
    "id":"land-fill", "type":"fill", "source":"basemap", "source-layer":"land",
    "paint":{"fill-color":"#d8ead3", "fill-opacity":0.75}
}"##)?;
# Ok::<(), serde_json::Error>(())
```

Fill/line paint and layout have separate types, so incompatible paint cannot be
assigned in Rust. Unknown fields, mismatched paint, unsupported layer types,
unsupported expressions and invalid color strings fail during deserialization;
they are not silently dropped. Layer JSON describes drawing rules rather than
a complete map document. Other layer types and unlisted properties are
currently unsupported.

```compile_fail
use maploom_core::{FillLayer, LinePaint};
let mut land = FillLayer::new("land", "land");
land.paint = LinePaint::default(); // fill layers require FillPaint
```

`FillPaint` supports RGB color and opacity. `LinePaint` adds width and dash lengths
in multiples of width; `LineLayout` supplies typed cap/join settings. Both layouts
support visibility. Paint numbers use `Number::Constant`, `Number::Linear` or
`Number::Step` with explicit zoom stops; they serialize as Maploom expressions.
`Rgb` accepts hex RGB strings and can be constructed without parsing. Filters
use the validated `Filter` type and serialize as Maploom JSON expressions.
Malformed or unsupported filter syntax fails during deserialization.
Strings in expressions are literals; property access uses `["get", "key"]`.

`id` identifies the drawing layer; `source_layer` identifies the MVT layer. Both
are explicit, with no generated IDs or layer-name rewriting. `source = None`
allows source-agnostic rendering. A named source restricts drawing to features
from that source. PDF rendering validates zoom bounds, nonempty identifiers,
opacity, width and dash lengths, including values constructed directly in Rust.

The old flat `RenderRule` format and `compile_render_rules` API have been removed.
Use `type`, `source-layer`, `paint` and `layout` in JSON, or the native Rust types.


Filter parsing and evaluation are backend-independent. `Feature`, `FeatureValue`
and `GeometryType` describe read-only feature access without MVT or PDF dependencies.
Use `maploom-filter-fast-mvt::FastMvtFeature` to evaluate fast-mvt features.

```rust
use maploom_core::{Filter, LineLayer};
use serde_json::json;

let mut roads = LineLayer::new("roads", "roads");
roads.filter = Some(serde_json::from_str::<Filter>(r#"["==", ["get", "kind"], "road"]"#)?);
let json = serde_json::to_value(&roads)?;
assert_eq!(json["filter"], json!(["==", ["get", "kind"], "road"]));
# Ok::<(), Box<dyn std::error::Error>>(())
```
