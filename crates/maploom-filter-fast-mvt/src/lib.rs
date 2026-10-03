#![doc = include_str!("../README.md")]

use maploom_core::{Feature, FeatureValue, GeometryType};

#[cfg(test)]
mod tests;

/// A borrowed adapter for owned or borrowed fast-mvt features.
#[derive(Clone, Copy, Debug)]
pub struct FastMvtFeature<T>(pub T);
use fast_mvt::{MvtFeature, MvtFeatureRef, MvtValue, MvtValueRef};
use geo_types::Geometry;

impl Feature for FastMvtFeature<&MvtFeature> {
    fn property(&self, key: &str) -> Option<FeatureValue<'_>> {
        self.0
            .properties
            .iter()
            .find(|(property_key, _)| property_key == key)
            .and_then(|(_, value)| mvt_value(value))
    }

    fn has_property(&self, key: &str) -> bool {
        self.0
            .properties
            .iter()
            .any(|(property_key, _)| property_key == key)
    }

    fn id(&self) -> Option<FeatureValue<'_>> {
        self.0.id.map(|id| FeatureValue::Number(id as f64))
    }

    fn geometry_type(&self) -> GeometryType {
        geometry_type(&self.0.geometry)
    }
}

impl Feature for FastMvtFeature<MvtFeatureRef<'_>> {
    fn property(&self, key: &str) -> Option<FeatureValue<'_>> {
        self.0.properties().find_map(|property| {
            let (property_key, value) = property.ok()?;
            (property_key == key)
                .then(|| mvt_value_ref(value))
                .flatten()
        })
    }

    fn has_property(&self, key: &str) -> bool {
        self.0
            .properties()
            .any(|property| property.is_ok_and(|(property_key, _)| property_key == key))
    }

    fn id(&self) -> Option<FeatureValue<'_>> {
        MvtFeatureRef::id(self.0).map(|id| FeatureValue::Number(id as f64))
    }

    fn geometry_type(&self) -> GeometryType {
        match self.0.geom_type_value() {
            Some(1) => GeometryType::Point,
            Some(2) => GeometryType::LineString,
            Some(3) => GeometryType::Polygon,
            _ => GeometryType::Unknown,
        }
    }
}

fn geometry_type(geometry: &fast_mvt::MvtGeometry) -> GeometryType {
    match geometry {
        Geometry::Point(_) | Geometry::MultiPoint(_) => GeometryType::Point,
        Geometry::LineString(_) | Geometry::MultiLineString(_) => GeometryType::LineString,
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) => GeometryType::Polygon,
        _ => GeometryType::Unknown,
    }
}

fn mvt_value(value: &MvtValue) -> Option<FeatureValue<'_>> {
    match value {
        MvtValue::String(value) => Some(FeatureValue::String(value)),
        MvtValue::Float(value) => Some(FeatureValue::Number(f64::from(*value))),
        MvtValue::Double(value) => Some(FeatureValue::Number(*value)),
        MvtValue::Int(value) | MvtValue::SInt(value) => Some(FeatureValue::Number(*value as f64)),
        MvtValue::UInt(value) => Some(FeatureValue::Number(*value as f64)),
        MvtValue::Bool(value) => Some(FeatureValue::Bool(*value)),
        MvtValue::Null => Some(FeatureValue::Null),
    }
}

fn mvt_value_ref(value: MvtValueRef<'_>) -> Option<FeatureValue<'_>> {
    match value {
        MvtValueRef::String(value) => Some(FeatureValue::String(value)),
        MvtValueRef::Float(value) => Some(FeatureValue::Number(f64::from(value))),
        MvtValueRef::Double(value) => Some(FeatureValue::Number(value)),
        MvtValueRef::Int(value) | MvtValueRef::SInt(value) => {
            Some(FeatureValue::Number(value as f64))
        }
        MvtValueRef::UInt(value) => Some(FeatureValue::Number(value as f64)),
        MvtValueRef::Bool(value) => Some(FeatureValue::Bool(value)),
        MvtValueRef::Null => Some(FeatureValue::Null),
    }
}
