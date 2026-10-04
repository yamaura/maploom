use crate::{Layer, LineCap, LineJoin, LineLayer, Number, PdfRenderError, Rgb, Visibility};
use maploom_core::Filter;
use maploom_filter_fast_mvt::FastMvtFeature;
#[cfg(feature = "fonts")]
use maploom_pdf_fonts::TextStyle;
use pdf_writer::{
    Content,
    types::{LineCapStyle, LineJoinStyle},
};

pub(super) struct CompiledLayer {
    #[cfg(feature = "fonts")]
    pub(super) id: String,
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
    #[cfg(feature = "fonts")]
    Text(TextStyle),
}

impl CompiledPaint {
    pub(super) fn opacity(&self) -> f32 {
        match self {
            Self::Fill(style) => style.opacity,
            Self::Line(style) => style.opacity,
            #[cfg(feature = "fonts")]
            Self::Text(style) => style.opacity,
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

/// Validate all layers and paint stops, including hidden layers and inactive zooms.
/// Requires no tiles or PDF document.
pub fn validate_layers(layers: &[Layer]) -> Result<(), PdfRenderError> {
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
        let check_opacity = |number: &Number| {
            let values = number_values(number).map_err(invalid)?;
            if values.iter().any(|value| !(0.0..=1.0).contains(value)) {
                return Err(invalid(
                    "opacity must be between zero and one at every zoom stop".into(),
                ));
            }
            Ok(())
        };
        match layer {
            Layer::Fill(layer) => check_opacity(&layer.paint.opacity)?,
            Layer::Line(layer) => {
                check_opacity(&layer.paint.opacity)?;
                let values = number_values(&layer.paint.width).map_err(invalid)?;
                if values
                    .iter()
                    .any(|value| *value < 0.0 || !(*value as f32).is_finite())
                {
                    return Err(invalid(
                        "line-width must be finite and nonnegative at every zoom stop".into(),
                    ));
                }
                // Dash scaling must be valid at the widest zoom stop.
                let mut widest = layer.clone();
                widest.paint.width = values.into_iter().fold(0.0, f64::max).into();
                line_style(&widest, 0.0).map_err(invalid)?;
            }
            Layer::Text(layer) => {
                check_opacity(&layer.paint.opacity)?;
                for number in [&layer.layout.size, &layer.paint.halo_width] {
                    if number_values(number)
                        .map_err(invalid)?
                        .iter()
                        .any(|value| *value < 0.0 || !(*value as f32).is_finite())
                    {
                        return Err(invalid("text-size and text-halo-width must be finite and nonnegative at every stop".into()));
                    }
                }
                if layer.layout.fonts.is_empty()
                    || layer.layout.fonts.iter().any(|name| name.trim().is_empty())
                {
                    return Err(invalid(
                        "text-font must name at least one explicit font family".into(),
                    ));
                }
                if !layer.layout.offset.iter().all(|n| n.is_finite())
                    || !layer.layout.line_height.is_finite()
                    || layer.layout.line_height <= 0.0
                    || !layer.layout.padding.is_finite()
                    || layer.layout.padding < 0.0
                {
                    return Err(invalid(
                        "text offsets must be finite; line height positive; padding nonnegative"
                            .into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn number_values(number: &Number) -> Result<Vec<f64>, String> {
    number.evaluate(0.0).map_err(str::to_owned)?;
    Ok(match number {
        Number::Constant(value) => vec![*value],
        Number::Linear { stops } => stops.iter().map(|(_, value)| *value).collect(),
        Number::Step { default, stops } => std::iter::once(*default)
            .chain(stops.iter().map(|(_, value)| *value))
            .collect(),
    })
}

pub(super) fn compile_layers(
    layers: &[Layer],
    zoom: f64,
) -> Result<Vec<CompiledLayer>, PdfRenderError> {
    validate_layers(layers)?;
    let mut compiled = Vec::new();
    for (index, layer) in layers.iter().enumerate() {
        let invalid = |message: String| PdfRenderError::InvalidLayer { index, message };
        let (min, max) = layer.zoom_range();
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
            #[cfg(feature = "fonts")]
            Layer::Text(layer) => CompiledPaint::Text(TextStyle {
                layout: layer.layout.clone(),
                size: layer
                    .layout
                    .size
                    .evaluate(zoom)
                    .map_err(|e| invalid(e.into()))? as f32,
                color: layer.paint.color,
                opacity: opacity(&layer.paint.opacity, zoom).map_err(invalid)?,
                halo_color: layer.paint.halo_color,
                halo_width: layer
                    .paint
                    .halo_width
                    .evaluate(zoom)
                    .map_err(|e| invalid(e.into()))? as f32,
            }),
            #[cfg(not(feature = "fonts"))]
            Layer::Text(_) => {
                return Err(invalid("text rendering requires the fonts feature".into()));
            }
        };
        compiled.push(CompiledLayer {
            #[cfg(feature = "fonts")]
            id: layer.id().to_owned(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LineLayer, Number, TextField, TextLayer, Visibility};
    use serde_json::json;

    #[test]
    fn typed_layers_preserve_zoom_boundaries_and_visibility() {
        let mut line = LineLayer::new("roads", "roads");
        line.minzoom = Some(4.0);
        line.maxzoom = Some(10.0);
        let layers = [Layer::Line(line.clone())];
        assert_eq!(compile_layers(&layers, 3.0).unwrap().len(), 0);
        assert_eq!(compile_layers(&layers, 4.0).unwrap().len(), 1);
        assert_eq!(compile_layers(&layers, 9.0).unwrap().len(), 1);
        assert_eq!(compile_layers(&layers, 10.0).unwrap().len(), 0);
        line.layout.visibility = Visibility::None;
        assert!(
            compile_layers(&[Layer::Line(line.clone())], 5.0)
                .unwrap()
                .is_empty()
        );
        line.minzoom = Some(f64::NAN);
        assert!(matches!(
            compile_layers(&[Layer::Line(line)], 5.0),
            Err(PdfRenderError::InvalidLayer { index: 0, .. })
        ));
    }

    #[test]
    fn dashes_scale_by_width_and_unsupported_layers_are_errors() {
        let mut line = LineLayer::new("roads", "roads");
        line.paint.width = 3.0.into();
        line.paint.dasharray = Some(vec![2.0, 1.0]);
        assert_eq!(
            line_style(&line, 0.0).unwrap().dasharray,
            Some(vec![6.0, 3.0])
        );
        assert!(
            serde_json::from_value::<Layer>(
                json!({"id":"places", "type":"symbol", "source-layer":"places"})
            )
            .is_err()
        );
    }

    fn label_layer() -> TextLayer {
        let mut layer = TextLayer::new("labels", "places", TextField::property("name"));
        layer.layout.fonts = vec!["Latin".into(), "Japanese".into(), "Hebrew".into()];
        layer
    }

    #[test]
    fn validates_text_layout_and_every_hidden_zoom_stop() {
        let mut layer = label_layer();
        layer.layout.visibility = Visibility::None;
        layer.layout.size = Number::Linear {
            stops: vec![(0.0, 10.0), (20.0, -1.0)],
        };
        assert!(validate_layers(&[Layer::Text(layer)]).is_err());
        let mut layer = label_layer();
        layer.layout.offset = [f32::INFINITY, 0.0];
        assert!(validate_layers(&[Layer::Text(layer)]).is_err());
        #[cfg(feature = "fonts")]
        {
            let mut layer = label_layer();
            layer.layout.size = Number::Linear {
                stops: vec![(0.0, 10.0), (10.0, 20.0)],
            };
            let compiled = compile_layers(&[Layer::Text(layer)], 5.0).unwrap();
            let CompiledPaint::Text(style) = &compiled[0].style else {
                panic!()
            };
            assert_eq!(style.size, 15.0);
        }
    }
}
