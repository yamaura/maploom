use crate::{Layer, LineCap, LineJoin, LineLayer, Number, PdfRenderError, Rgb, Visibility};
use maploom_core::Filter;
use maploom_filter_fast_mvt::FastMvtFeature;
use pdf_writer::{
    Content,
    types::{LineCapStyle, LineJoinStyle},
};

pub(super) struct CompiledLayer {
    pub(super) source: Option<String>,
    pub(super) layer_name: String,
    filter: Option<Filter>,
    pub(super) style: CompiledPaint,
}

impl CompiledLayer {
    pub(super) fn matches(&self, feature: &fast_mvt::MvtFeature) -> bool {
        self.filter
            .as_ref()
            .is_none_or(|filter| filter.matches(&FastMvtFeature(feature)))
    }
}

pub(super) enum CompiledPaint {
    Fill(FillStyle),
    Line(LineStyle),
}

impl CompiledPaint {
    pub(super) fn opacity(&self) -> f32 {
        match self {
            Self::Fill(style) => style.opacity,
            Self::Line(style) => style.opacity,
        }
    }
}

pub(super) struct FillStyle {
    pub(super) color: Rgb,
    pub(super) opacity: f32,
}

pub(super) struct LineStyle {
    pub(super) color: Rgb,
    pub(super) opacity: f32,
    pub(super) width: f32,
    pub(super) cap: LineCapStyle,
    pub(super) join: LineJoinStyle,
    pub(super) dasharray: Option<Vec<f32>>,
}

pub(super) fn compile_layers(
    layers: &[Layer],
    zoom: f64,
) -> Result<Vec<CompiledLayer>, PdfRenderError> {
    let mut compiled = Vec::new();
    for (index, layer) in layers.iter().enumerate() {
        let invalid = |message: String| PdfRenderError::InvalidLayer { index, message };
        if layer.id().is_empty() || layer.source_layer().is_empty() || layer.source() == Some("") {
            return Err(invalid(
                "id, source-layer and any supplied source must be nonempty".into(),
            ));
        }
        let (min, max) = layer.zoom_range();
        if [min, max]
            .into_iter()
            .flatten()
            .any(|n| !n.is_finite() || n < 0.0)
            || min.zip(max).is_some_and(|(min, max)| min >= max)
        {
            return Err(invalid(
                "zoom range must be finite, nonnegative and increasing".into(),
            ));
        }
        if layer.visibility() == Visibility::None
            || min.is_some_and(|min| zoom < min)
            || max.is_some_and(|max| zoom >= max)
        {
            continue;
        }
        let filter = layer.filter().cloned();
        let style = match layer {
            Layer::Fill(layer) => CompiledPaint::Fill(FillStyle {
                color: layer.paint.color,
                opacity: opacity(&layer.paint.opacity, zoom).map_err(invalid)?,
            }),
            Layer::Line(layer) => CompiledPaint::Line(line_style(layer, zoom).map_err(invalid)?),
        };
        compiled.push(CompiledLayer {
            source: layer.source().map(str::to_owned),
            layer_name: layer.source_layer().to_owned(),
            filter,
            style,
        });
    }
    Ok(compiled)
}

fn opacity(value: &Number, zoom: f64) -> Result<f32, String> {
    let value = value.evaluate(zoom).map_err(str::to_owned)?;
    if !(0.0..=1.0).contains(&value) {
        return Err("opacity must be between zero and one".into());
    }
    Ok(value as f32)
}

pub(super) fn line_style(layer: &LineLayer, zoom: f64) -> Result<LineStyle, String> {
    let width = layer.paint.width.evaluate(zoom).map_err(str::to_owned)? as f32;
    if !width.is_finite() || width < 0.0 {
        return Err("line-width must be finite and nonnegative".into());
    }
    let dasharray = layer.paint.dasharray.as_ref().map(|values| {
        if values.is_empty() || !values.iter().all(|n| n.is_finite() && *n >= 0.0) || !values.iter().any(|n| *n > 0.0) {
            return Err("dash lengths must be finite and nonnegative, with at least one positive length".to_owned());
        }
        let scaled: Vec<_> = values.iter().map(|n| n * width).collect();
        if !scaled.iter().all(|n| n.is_finite()) { return Err("dash length overflow".into()); }
        Ok(scaled)
    }).transpose()?;
    Ok(LineStyle {
        color: layer.paint.color,
        opacity: opacity(&layer.paint.opacity, zoom)?,
        width,
        cap: match layer.layout.cap {
            LineCap::Butt => LineCapStyle::ButtCap,
            LineCap::Round => LineCapStyle::RoundCap,
            LineCap::Square => LineCapStyle::ProjectingSquareCap,
        },
        join: match layer.layout.join {
            LineJoin::Miter => LineJoinStyle::MiterJoin,
            LineJoin::Round => LineJoinStyle::RoundJoin,
            LineJoin::Bevel => LineJoinStyle::BevelJoin,
        },
        dasharray,
    })
}

pub(super) fn set_fill_rgb(content: &mut Content, color: Rgb) {
    content.set_fill_rgb(
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
    );
}

pub(super) fn set_stroke_rgb(content: &mut Content, color: Rgb) {
    content.set_stroke_rgb(
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
    );
}
