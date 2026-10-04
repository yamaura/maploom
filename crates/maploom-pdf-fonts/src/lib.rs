#![doc = include_str!("../README.md")]
mod fonts;
mod resources;
#[cfg(test)]
mod test_fonts;
mod text;

pub use fonts::{FontCatalog, FontError};
use maploom_core::{Rgb, TextLayout};
pub use resources::{FontForm, FontResources};
pub use text::{Label, draw, layout};

/// Evaluated text style supplied by the renderer, in PDF points.
pub struct TextStyle {
    pub layout: TextLayout,
    pub size: f32,
    pub color: Rgb,
    pub opacity: f32,
    pub halo_color: Rgb,
    pub halo_width: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum TextError {
    #[error("{0}")]
    Font(#[source] FontError),
    #[error("text layout coordinate overflow")]
    CoordinateOverflow,
}
