# Maploom

Typed map drawing rules, feature expressions and vector PDF rendering.

| Crate | Role |
| --- | --- |
| [maploom-core](crates/maploom-core) | Typed layers and feature expressions |
| [maploom-filter-fast-mvt](crates/maploom-filter-fast-mvt) | fast-mvt feature adapters |
| [maploom-pdf](crates/maploom-pdf) | Vector maps as reusable PDF forms |
| [maploom-pdf-fonts](crates/maploom-pdf-fonts) | Font registration, text shaping and PDF embedding |
| [maploom-maplibre](crates/maploom-maplibre) | MapLibre layer and filter JSON conversion |

PDF text uses the default-enabled `fonts` feature and caller-supplied font data.
No fonts are bundled. The examples use PDF-standard Helvetica.

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p maploom-pdf --example render -- map.pdf
cargo run -p maploom-pdf --example layers -- layers.pdf
```
