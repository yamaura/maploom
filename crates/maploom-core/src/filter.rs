//! Boolean predicates over the shared feature expression language.
use crate::Expression;
pub use crate::expression::{EvalError, Feature, FeatureValue, GeometryType};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Filter(Expression);

impl Filter {
    pub fn new(expression: Expression) -> Self {
        Self(expression)
    }
    /// Evaluate this predicate against a backend-independent feature.
    ///
    /// ```
    /// use maploom_core::{Feature, FeatureValue, Filter, GeometryType};
    /// use serde_json::json;
    ///
    /// struct Road;
    /// impl Feature for Road {
    ///     fn property(&self, key: &str) -> Option<FeatureValue<'_>> {
    ///         (key == "kind").then_some(FeatureValue::String("road"))
    ///     }
    ///     fn id(&self) -> Option<FeatureValue<'_>> {
    ///         None
    ///     }
    ///     fn geometry_type(&self) -> GeometryType {
    ///         GeometryType::LineString
    ///     }
    /// }
    /// let filter: Filter = serde_json::from_value(json!([
    ///     "all",
    ///     ["==", ["get", "kind"], "road"],
    ///     ["==", ["geometry-type"], "LineString"]
    /// ]))
    /// .unwrap();
    /// assert!(filter.evaluate(&Road).unwrap());
    /// ```
    pub fn evaluate<F: Feature + ?Sized>(&self, feature: &F) -> Result<bool, EvalError> {
        self.0.evaluate(feature)?.expect_bool()
    }
    pub fn matches<F: Feature + ?Sized>(&self, feature: &F) -> bool {
        self.evaluate(feature).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filters_round_trip_without_changing_expression_semantics() {
        for value in [
            json!(["==", ["get", "kind"], "road"]),
            json!(["!", ["in", ["geometry-type"], ["literal", ["Polygon"]]]]),
            json!(["!", ["any", ["has", "hidden"]]]),
            json!(["==", ["literal", "road"], "road"]),
            json!(["in", ["get", "kind"], ["literal", ["road", "rail"]]]),
            json!(["match", ["get", "kind"], ["road", "rail"], true, false]),
            json!(["!", ["has", "hidden"]]),
            json!(["==", ["coalesce", ["get", "kind"], "road"], "road"]),
            json!([">", ["to-number", ["get", "rank"], 0], 2]),
            json!(["==", ["%", ["get", "code"], 10000], 101]),
        ] {
            let filter = serde_json::from_value::<Filter>(value.clone()).unwrap();
            let serialized = serde_json::to_value(&filter).unwrap();
            let restored: Filter = serde_json::from_value(serialized).unwrap();
            assert_eq!(restored, filter, "changed {value}");
        }
    }

    #[test]
    fn core_expressions_use_literal_strings_and_require_modern_operators() {
        let filter = serde_json::from_value::<Filter>(json!(["==", "road", "road"])).unwrap();
        let json = serde_json::to_value(&filter).unwrap();
        assert_eq!(json, json!(["==", ["literal", "road"], "road"]));
        for unsupported in [
            json!(["!has", "kind"]),
            json!(["none", true]),
            json!(["!in", "kind", "road"]),
        ] {
            assert!(serde_json::from_value::<Filter>(unsupported).is_err());
        }
    }
}
