# Maploom

Typed map drawing rules, feature expressions and vector PDF rendering.

| Crate | Role |
| --- | --- |
| [maploom-core](crates/maploom-core) | Typed layers, paint, layout, zoom values and backend-independent filters |
| [maploom-filter-fast-mvt](crates/maploom-filter-fast-mvt) | fast-mvt feature adapters for filter evaluation |
| [maploom-pdf](crates/maploom-pdf) | Render MVT polygons and lines into reusable PDF Form XObjects |
| [Compatibility adapter](crates/maploom-maplibre) | Import/export external layer and filter JSON |

Core depends only on `serde` for its library API. PDF rendering uses
core and the fast-mvt adapter. Supported drawing rules, expressions and format
conversions are documented in the crate READMEs.

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p maploom-pdf --example render -- map.pdf
cargo run -p maploom-pdf --example layers -- layers.pdf
```

Licensed under the [MIT License](LICENSE). Copyright (c) 2024 Yuki Yamaura.
