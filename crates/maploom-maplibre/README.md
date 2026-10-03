# maploom-maplibre

MapLibre JSON adapters for Maploom. This crate imports and exports the supported
fill/line layer subset and translates legacy property filters into Maploom
expressions. Maploom expression syntax is inspired by MapLibre, while its
evaluation behavior is defined independently. This adapter handles conversion
between the supported formats. It depends on `maploom-core` and `serde_json`, with no MapLibre
runtime, MVT or PDF dependency.

```rust
use maploom_maplibre::{from_layer, to_layer};
use serde_json::json;

let layer = from_layer(&json!({
    "id": "roads", "type": "line", "source": "basemap", "source-layer": "roads",
    "filter": ["in", "kind", "road"],
    "paint": {"line-color": "#336699", "line-width": 2}
}))?;
let json = to_layer(&layer)?;
assert_eq!(json["filter"], json!(["in", ["get", "kind"], ["literal", ["road"]]]));
# Ok::<(), maploom_maplibre::Error>(())
```

- `from_filter` detects legacy filters or modern expression filters. Legacy
  comparisons treat their string key as a property; `in` and `!in` use exact
  membership, including one candidate or an empty candidate list. `$id`, `$type`,
  `has`, `!has`, `all`, `any` and `none` are translated explicitly.
- `from_expression` imports modern expression syntax explicitly. Strings are
  literals: `["in", "ro", "road"]` tests substring membership here, whereas
  `from_filter` reads the ambiguous form as legacy property membership.
- `to_filter` exports modern expression syntax. The original legacy spelling is
  not preserved. `literal` payloads and `match` labels are never traversed as code.
- `from_layer` / `to_layer` convert individual layers; `from_layers` / `to_layers`
  preserve array order and identify failing layer indices. Vector layers require
  nonempty `id`, `source` and `source-layer`; source-agnostic PDF layers need a
  source before export. Paint and layout use the subset documented in core.

Supported expressions include property/ID/geometry access, comparisons,
membership, boolean operations, `match`, `coalesce`, type assertions, conversions
and arithmetic. Constant numeric paint and linear/step zoom paint are supported.
Unknown fields, unsupported operators (including `within`, collators and
object-valued `get`/`has`), unsupported layer types and mixed legacy/expression
filters produce errors rather than being dropped. A complete style document,
including its sources, is outside this crate's scope.

This is a JSON conversion adapter for a supported subset, not a complete
MapLibre expression engine or Style Spec validator. Evaluation uses Maploom's
independent semantics; feature-dependent type errors are reported at evaluation.
Numeric properties and IDs use f64, so integers above 2^53 may lose precision.
Full expression compatibility and renderer parity are not guaranteed.

References: [MapLibre layer format](https://maplibre.org/maplibre-style-spec/layers/)
and [filter syntax classification](https://github.com/maplibre/maplibre-style-spec/blob/main/src/feature_filter/index.ts).
