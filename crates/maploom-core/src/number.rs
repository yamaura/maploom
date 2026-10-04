use serde::de::{Error, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

use crate::serde_support::required;

/// A numeric paint value with Maploom JSON zoom expressions.
/// Stops must have finite values and strictly increasing zooms. Linear requires
/// at least two stops; Step requires at least one. Invalid native values are
/// rejected by `evaluate` and serialization, and invalid JSON by deserialization.
#[derive(Clone, Debug, PartialEq)]
pub enum Number {
    Constant(f64),
    Linear {
        stops: Vec<(f64, f64)>,
    },
    Step {
        default: f64,
        stops: Vec<(f64, f64)>,
    },
}

impl From<f64> for Number {
    fn from(value: f64) -> Self {
        Self::Constant(value)
    }
}

impl Number {
    fn validate(&self) -> Result<(), &'static str> {
        let (stops, minimum) = match self {
            Self::Constant(n) => {
                return if n.is_finite() {
                    Ok(())
                } else {
                    Err("number must be finite")
                };
            }
            Self::Linear { stops } => (stops, 2),
            Self::Step { default, stops } => {
                if !default.is_finite() {
                    return Err("step default must be finite");
                }
                (stops, 1)
            }
        };
        if stops.len() < minimum {
            return Err("not enough zoom stops");
        }
        if !stops
            .iter()
            .all(|(zoom, n)| zoom.is_finite() && n.is_finite())
        {
            return Err("zoom stops and values must be finite");
        }
        if stops.windows(2).any(|w| w[0].0 >= w[1].0) {
            return Err("zoom stops must be strictly increasing");
        }
        Ok(())
    }

    pub fn evaluate(&self, zoom: f64) -> Result<f64, &'static str> {
        self.validate()?;
        if !zoom.is_finite() {
            return Err("zoom must be finite");
        }
        let value = match self {
            Self::Constant(n) => *n,
            Self::Linear { stops } => {
                let mut output = stops[0].1;
                for pair in stops.windows(2) {
                    let [(left_zoom, left), (right_zoom, right)] = [pair[0], pair[1]];
                    if zoom <= left_zoom {
                        break;
                    }
                    if zoom >= right_zoom {
                        output = right;
                        continue;
                    }
                    let t = (zoom - left_zoom) / (right_zoom - left_zoom);
                    output = left * (1.0 - t) + right * t;
                    break;
                }
                output
            }
            Self::Step { default, stops } => stops
                .iter()
                .take_while(|(stop, _)| zoom >= *stop)
                .last()
                .map_or(*default, |(_, n)| *n),
        };
        if value.is_finite() {
            Ok(value)
        } else {
            Err("numeric expression overflow")
        }
    }
}

struct NumberVisitor;

impl<'de> Visitor<'de> for NumberVisitor {
    type Value = Number;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a number or a supported zoom expression")
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Number, E> {
        self.visit_f64(value as f64)
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Number, E> {
        self.visit_f64(value as f64)
    }

    fn visit_f64<E: Error>(self, value: f64) -> Result<Number, E> {
        let number = Number::Constant(value);
        number.validate().map_err(E::custom)?;
        Ok(number)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Number, A::Error> {
        let operator: String = required(&mut seq)?;
        let default = match operator.as_str() {
            "interpolate" => {
                let _: (Linear,) = required(&mut seq)?;
                let _: (Zoom,) = required(&mut seq)?;
                None
            }
            "step" => {
                let _: (Zoom,) = required(&mut seq)?;
                Some(required(&mut seq)?)
            }
            _ => {
                return Err(A::Error::custom(format!(
                    "unsupported numeric operator {operator:?}"
                )));
            }
        };
        let mut stops = Vec::new();
        while let Some(zoom) = seq.next_element()? {
            stops.push((zoom, required(&mut seq)?));
        }
        let number = match default {
            Some(default) => Number::Step { default, stops },
            None => Number::Linear { stops },
        };
        number.validate().map_err(A::Error::custom)?;
        Ok(number)
    }
}

#[derive(Deserialize)]
enum Zoom {
    #[serde(rename = "zoom")]
    Value,
}

#[derive(Deserialize)]
enum Linear {
    #[serde(rename = "linear")]
    Value,
}

impl<'de> Deserialize<'de> for Number {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NumberVisitor)
    }
}

impl Serialize for Number {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        let (operator, stops) = match self {
            Self::Constant(value) => return serializer.serialize_f64(*value),
            Self::Linear { stops } => ("interpolate", stops),
            Self::Step { stops, .. } => ("step", stops),
        };
        let mut seq = serializer.serialize_seq(Some(3 + 2 * stops.len()))?;
        seq.serialize_element(operator)?;
        match self {
            Self::Linear { .. } => {
                seq.serialize_element(&("linear",))?;
                seq.serialize_element(&("zoom",))?;
            }
            Self::Step { default, .. } => {
                seq.serialize_element(&("zoom",))?;
                seq.serialize_element(default)?;
            }
            Self::Constant(_) => unreachable!(),
        }
        for (zoom, value) in stops {
            seq.serialize_element(zoom)?;
            seq.serialize_element(value)?;
        }
        seq.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn evaluates_and_round_trips_zoom_expressions() {
        let linear = Number::Linear {
            stops: vec![(4.0, 1.0), (10.0, 4.0)],
        };
        assert_eq!(linear.evaluate(3.0), Ok(1.0));
        assert_eq!(linear.evaluate(7.0), Ok(2.5));
        assert_eq!(linear.evaluate(11.0), Ok(4.0));
        assert_eq!(
            serde_json::to_value(&linear).unwrap(),
            json!(["interpolate", ["linear"], ["zoom"], 4.0, 1.0, 10.0, 4.0])
        );
        let step = Number::Step {
            default: 1.0,
            stops: vec![(4.0, 2.0), (10.0, 3.0)],
        };
        assert_eq!(step.evaluate(3.0), Ok(1.0));
        assert_eq!(step.evaluate(4.0), Ok(2.0));
        assert_eq!(step.evaluate(10.0), Ok(3.0));
        for value in [linear, step] {
            assert_eq!(
                serde_json::from_value::<Number>(serde_json::to_value(&value).unwrap()).unwrap(),
                value
            );
        }
    }

    #[test]
    fn invalid_native_numbers_are_rejected() {
        for value in [
            Number::Constant(f64::NAN),
            Number::Linear { stops: vec![] },
            Number::Linear {
                stops: vec![(4.0, 1.0), (4.0, 2.0)],
            },
            Number::Step {
                default: 1.0,
                stops: vec![(8.0, 1.0), (4.0, 2.0)],
            },
        ] {
            assert!(value.evaluate(5.0).is_err());
            assert!(serde_json::to_value(&value).is_err());
        }
    }

    #[test]
    fn unsupported_or_malformed_numeric_expressions_fail_at_deserialization() {
        for value in [
            json!(["interpolate", ["exponential", 2], ["zoom"], 0, 1, 10, 4]),
            json!(["step", ["zoom"], 1, 2]),
            json!(["step", ["zoom"], 1, 8, 1, 4, 2]),
            json!(["get", "width"]),
        ] {
            assert!(serde_json::from_value::<Number>(value).is_err());
        }
    }

    #[test]
    fn malformed_numeric_expression_streams_are_rejected() {
        for source in [
            r#"["step", ["zoom", 1], 1, 2, 3]"#,
            r#"["step", ["zoom"], 1, 2]"#,
            r#"["interpolate", ["linear", 1], ["zoom"], 0, 1, 2, 3]"#,
            r#"["interpolate", ["linear"], ["get", "zoom"], 0, 1, 2, 3]"#,
            r#"["interpolate", ["linear"], ["zoom"], 0, 1, 2]"#,
        ] {
            assert!(
                serde_json::from_str::<Number>(source).is_err(),
                "accepted {source}"
            );
        }
    }
}
