#![doc = include_str!("../README.md")]

use maploom_core::{Filter, Layer};
use serde_json::{Value, json};
use std::{error::Error as StdError, fmt};

#[cfg(test)]
mod tests;

/// Unsupported or malformed compatibility input, including its layer index when applicable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    message: String,
}

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl StdError for Error {}

/// Import a MapLibre filter, detecting legacy property filters or expression syntax.
/// Null means an unrestricted filter. Mixed legacy/expression trees are rejected.
pub fn from_filter(value: &Value) -> Result<Filter, Error> {
    let expression = if value.is_null() {
        json!(true)
    } else if classify(value) == Syntax::Legacy {
        convert_legacy(value)?
    } else {
        reject_mixed(value)?;
        value.clone()
    };
    from_expression(&expression)
}

/// Import an expression filter explicitly, without legacy property-key detection.
/// For example, `in` with two strings means substring membership here.
pub fn from_expression(value: &Value) -> Result<Filter, Error> {
    validate_expression(value)?;
    serde_json::from_value(value.clone()).map_err(|error| Error::new(error.to_string()))
}

/// Export a filter in modern expression syntax, normalizing legacy input.
pub fn to_filter(filter: &Filter) -> Result<Value, Error> {
    let value = serde_json::to_value(filter).map_err(|error| Error::new(error.to_string()))?;
    validate_expression(&value)?;
    Ok(value)
}

/// Import a supported MapLibre vector fill/line layer.
pub fn from_layer(value: &Value) -> Result<Layer, Error> {
    let mut value = value.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| Error::new("layer must be an object"))?;
    if let Some(filter) = object.get_mut("filter")
        && !filter.is_null()
    {
        *filter = to_filter(&from_filter(filter)?)?;
    }
    let layer: Layer =
        serde_json::from_value(value).map_err(|error| Error::new(error.to_string()))?;
    validate_layer(&layer)?;
    Ok(layer)
}

/// Export a supported vector layer. A nonempty source is required by this adapter.
pub fn to_layer(layer: &Layer) -> Result<Value, Error> {
    validate_layer(layer)?;
    let mut value = serde_json::to_value(layer).map_err(|error| Error::new(error.to_string()))?;
    if let Some(filter) = layer.filter() {
        value["filter"] = to_filter(filter)?;
    }
    Ok(value)
}

/// Import a layer array in drawing order. Unsupported layers are errors, not skipped.
pub fn from_layers(value: &Value) -> Result<Vec<Layer>, Error> {
    let layers = value
        .as_array()
        .ok_or_else(|| Error::new("layers must be an array"))?;
    layers
        .iter()
        .enumerate()
        .map(|(index, layer)| {
            from_layer(layer).map_err(|error| Error::new(format!("layers[{index}]: {error}")))
        })
        .collect()
}

/// Export a layer array in drawing order.
pub fn to_layers(layers: &[Layer]) -> Result<Value, Error> {
    layers
        .iter()
        .enumerate()
        .map(|(index, layer)| {
            to_layer(layer).map_err(|error| Error::new(format!("layers[{index}]: {error}")))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

fn validate_layer(layer: &Layer) -> Result<(), Error> {
    if layer.id().is_empty()
        || layer.source_layer().is_empty()
        || layer.source().is_none_or(str::is_empty)
    {
        return Err(Error::new(
            "MapLibre vector layers require nonempty id, source and source-layer",
        ));
    }
    let (min, max) = layer.zoom_range();
    if [min, max]
        .into_iter()
        .flatten()
        .any(|zoom| !zoom.is_finite() || !(0.0..=24.0).contains(&zoom))
        || min.zip(max).is_some_and(|(min, max)| min >= max)
    {
        return Err(Error::new(
            "zoom bounds must be within 0..=24 and increasing",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Syntax {
    Legacy,
    Expression,
    Neutral,
}

fn classify(value: &Value) -> Syntax {
    if value.is_boolean() {
        return Syntax::Neutral;
    }
    let Some(items) = value.as_array() else {
        return Syntax::Legacy;
    };
    match items.first().and_then(Value::as_str) {
        Some("!in" | "!has" | "none") => Syntax::Legacy,
        Some("has") if items.len() == 2 => {
            if matches!(items[1].as_str(), Some("$id" | "$type")) {
                Syntax::Legacy
            } else {
                Syntax::Neutral
            }
        }
        Some("in") => {
            if items.len() >= 3 && (!items[1].is_string() || items[2].is_array()) {
                Syntax::Expression
            } else {
                Syntax::Legacy
            }
        }
        Some("==" | "!=" | ">" | ">=" | "<" | "<=") => {
            if items.len() == 3 && !items[1].is_array() && !items[2].is_array() {
                Syntax::Legacy
            } else {
                Syntax::Expression
            }
        }
        Some("all" | "any") => {
            let mut result = Syntax::Neutral;
            for child in &items[1..] {
                match classify(child) {
                    Syntax::Expression => return Syntax::Expression,
                    Syntax::Legacy => result = Syntax::Legacy,
                    Syntax::Neutral => {}
                }
            }
            result
        }
        _ => Syntax::Expression,
    }
}

fn reject_mixed(value: &Value) -> Result<(), Error> {
    let Some(items) = value.as_array() else {
        return Ok(());
    };
    if matches!(
        items.first().and_then(Value::as_str),
        Some("all" | "any" | "!")
    ) {
        for child in &items[1..] {
            if classify(child) == Syntax::Legacy {
                return Err(Error::new(
                    "mixed legacy and expression filter syntax is unsupported",
                ));
            }
            reject_mixed(child)?;
        }
    }
    Ok(())
}

fn property(key: &str) -> Value {
    match key {
        "$id" => json!(["id"]),
        "$type" => json!(["geometry-type"]),
        _ => json!(["get", key]),
    }
}

fn convert_legacy(value: &Value) -> Result<Value, Error> {
    if value.is_boolean() {
        return Ok(value.clone());
    }
    let items = value
        .as_array()
        .ok_or_else(|| Error::new("legacy filter must be an array"))?;
    let op = items
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("missing legacy filter operator"))?;
    let args = &items[1..];
    if matches!(op, "all" | "any" | "none") {
        let mut result = vec![json!(if op == "none" { "any" } else { op })];
        for child in args {
            result.push(convert_legacy(child)?);
        }
        let result = Value::Array(result);
        return Ok(if op == "none" {
            json!(["!", result])
        } else {
            result
        });
    }
    let key = args
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("legacy filter key must be a string"))?;
    match op {
        "==" | "!=" | ">" | ">=" | "<" | "<=" if args.len() == 2 && scalar(&args[1]) => {
            Ok(json!([op, property(key), args[1]]))
        }
        "in" | "!in" if args[1..].iter().all(scalar) => {
            let expression = json!(["in", property(key), ["literal", &args[1..]]]);
            Ok(if op == "!in" {
                json!(["!", expression])
            } else {
                expression
            })
        }
        "has" | "!has" if args.len() == 1 => {
            let expression = match key {
                "$id" => json!(["!=", ["id"], null]),
                "$type" => json!(true),
                _ => json!(["has", key]),
            };
            Ok(if op == "!has" {
                json!(["!", expression])
            } else {
                expression
            })
        }
        _ => Err(Error::new(format!(
            "unsupported or malformed legacy filter {op:?}"
        ))),
    }
}

fn scalar(value: &Value) -> bool {
    value.is_string() || value.is_number() || value.is_boolean()
}

// Literal payloads and match labels are data, not expressions.
fn validate_expression(value: &Value) -> Result<(), Error> {
    let Some(items) = value.as_array() else {
        return if value.is_object() {
            Err(Error::new("objects require literal"))
        } else {
            Ok(())
        };
    };
    let op = items
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("missing expression operator"))?;
    let args = &items[1..];
    let valid_count = match op {
        "literal" | "get" | "has" | "!" | "to-boolean" => args.len() == 1,
        "id" | "geometry-type" => args.is_empty(),
        "==" | "!=" | ">" | ">=" | "<" | "<=" | "in" | "/" | "%" => args.len() == 2,
        "-" => matches!(args.len(), 1 | 2),
        "+" | "*" => args.len() >= 2,
        "all" | "any" => true,
        "coalesce" | "to-number" | "boolean" | "number" | "string" => !args.is_empty(),
        "match" => args.len() >= 4 && args.len().is_multiple_of(2),
        _ => {
            return Err(Error::new(format!(
                "unsupported MapLibre expression operator {op:?}"
            )));
        }
    };
    if !valid_count {
        return Err(Error::new(format!("invalid argument count for {op:?}")));
    }
    match op {
        "literal" => {}
        "get" | "has" => {
            if !args[0].is_string() {
                return Err(Error::new(format!("{op} requires a string key")));
            }
        }
        "match" => {
            validate_expression(&args[0])?;
            for pair in args[1..args.len() - 1].as_chunks::<2>().0 {
                let labels = pair[0]
                    .as_array()
                    .map_or(std::slice::from_ref(&pair[0]), Vec::as_slice);
                if labels.is_empty()
                    || !labels
                        .iter()
                        .all(|value| value.is_string() || value.is_number())
                {
                    return Err(Error::new(
                        "match labels must be nonempty strings or numbers",
                    ));
                }
                validate_expression(&pair[1])?;
            }
            validate_expression(args.last().expect("checked count"))?;
        }
        _ => {
            for arg in args {
                validate_expression(arg)?;
            }
        }
    }
    Ok(())
}
