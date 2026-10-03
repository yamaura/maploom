# maploom-filter-fast-mvt

Borrowed fast-mvt adapters for `maploom-core` filters. Parsing and evaluation
live in core; this crate supplies feature property, ID and geometry access.

```rust
use maploom_core::Filter;
use maploom_filter_fast_mvt::FastMvtFeature;
use fast_mvt::{MvtFeature, MvtGeometry, MvtPoint, MvtValue};
use serde_json::json;

let mut feature = MvtFeature::new(MvtGeometry::Point(MvtPoint::new(1, 2)));
feature.add_tag("kind", MvtValue::from("station"));
let filter = serde_json::from_value::<Filter>(json!(["==", ["get", "kind"], "station"]))?;
assert!(filter.evaluate(&FastMvtFeature(&feature))?);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Wrap an owned `MvtFeature` by reference, or a borrowed `MvtFeatureRef` by value.
No property collections or geometry are copied. IDs and numeric properties
are represented as f64, so integers above 2^53 may lose precision.
