//! Backend-independent Maploom expression parsing and evaluation.

use std::{cmp::Ordering, collections::BTreeMap};

mod serde;

#[derive(Clone, Debug, PartialEq)]
/// A parsed feature expression shared by filters and text fields.
pub struct Expression {
    expression: Expr,
}

impl Expression {
    pub fn literal(value: impl Into<Value>) -> Self {
        Self {
            expression: Expr::Literal(value.into()),
        }
    }

    pub fn property(key: impl Into<String>) -> Self {
        Self {
            expression: Expr::Get(key.into()),
        }
    }

    pub fn to_text(self) -> Self {
        Self {
            expression: Expr::ToString(Box::new(self.expression)),
        }
    }

    pub fn concat(values: impl IntoIterator<Item = Self>) -> Self {
        Self {
            expression: Expr::Concat(values.into_iter().map(|v| v.expression).collect()),
        }
    }

    pub fn evaluate<F: Feature + ?Sized>(&self, feature: &F) -> Result<Value, EvalError> {
        self.expression.eval(feature)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryType {
    Point,
    LineString,
    Polygon,
    Unknown,
}

impl GeometryType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Point => "Point",
            Self::LineString => "LineString",
            Self::Polygon => "Polygon",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FeatureValue<'a> {
    Null,
    Bool(bool),
    Number(f64),
    String(&'a str),
}

impl FeatureValue<'_> {
    fn into_value(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(value),
            Self::Number(value) => Value::Number(value),
            Self::String(value) => Value::String(value.to_owned()),
        }
    }
}

/// Read-only feature access for custom data sources and MVT features.
pub trait Feature {
    fn property(&self, key: &str) -> Option<FeatureValue<'_>>;

    fn has_property(&self, key: &str) -> bool {
        self.property(key).is_some()
    }

    fn id(&self) -> Option<FeatureValue<'_>>;

    fn geometry_type(&self) -> GeometryType;
}

impl<T: Feature + ?Sized> Feature for &T {
    fn property(&self, key: &str) -> Option<FeatureValue<'_>> {
        (*self).property(key)
    }

    fn has_property(&self, key: &str) -> bool {
        (*self).has_property(key)
    }

    fn id(&self) -> Option<FeatureValue<'_>> {
        (*self).id()
    }

    fn geometry_type(&self) -> GeometryType {
        (*self).geometry_type()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct EvalError {
    message: String,
}

impl EvalError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Expr {
    Literal(Value),
    Get(String),
    Has(String),
    Id,
    GeometryType,
    Eq(Box<Expr>, Box<Expr>),
    Ne(Box<Expr>, Box<Expr>),
    Ordered(OrderedOp, Box<Expr>, Box<Expr>),
    In(Box<Expr>, Box<Expr>),
    All(Vec<Expr>),
    Any(Vec<Expr>),
    Not(Box<Expr>),
    Match {
        input: Box<Expr>,
        branches: Vec<(Vec<Value>, Expr)>,
        fallback: Box<Expr>,
    },
    Coalesce(Vec<Expr>),
    ToBoolean(Box<Expr>),
    ToString(Box<Expr>),
    Concat(Vec<Expr>),
    Case {
        branches: Vec<(Expr, Expr)>,
        fallback: Box<Expr>,
    },
    ToNumber(Vec<Expr>),
    TypeAssert(Type, Vec<Expr>),
    Math(MathOp, Vec<Expr>),
}

impl Expr {
    fn eval<F: Feature + ?Sized>(&self, feature: &F) -> Result<Value, EvalError> {
        match self {
            Self::Literal(value) => Ok(value.clone()),
            Self::Get(key) => Ok(feature
                .property(key)
                .map(FeatureValue::into_value)
                .unwrap_or(Value::Null)),
            Self::Has(key) => Ok(Value::Bool(feature.has_property(key))),
            Self::Id => Ok(feature
                .id()
                .map(FeatureValue::into_value)
                .unwrap_or(Value::Null)),
            Self::GeometryType => Ok(Value::String(feature.geometry_type().as_str().to_owned())),
            Self::Eq(left, right) => Ok(Value::Bool(left.eval(feature)? == right.eval(feature)?)),
            Self::Ne(left, right) => Ok(Value::Bool(left.eval(feature)? != right.eval(feature)?)),
            Self::Ordered(op, left, right) => {
                let left = left.eval(feature)?;
                let right = right.eval(feature)?;
                let ordering = left.partial_cmp(&right).ok_or_else(|| {
                    EvalError::new(format!(
                        "{} requires two strings or two numbers, got {} and {}",
                        op.as_str(),
                        left.type_name(),
                        right.type_name()
                    ))
                })?;
                Ok(Value::Bool(op.matches(ordering)))
            }
            Self::In(needle, haystack) => {
                let needle = needle.eval(feature)?;
                let haystack = haystack.eval(feature)?;
                Ok(Value::Bool(value_in(&needle, &haystack)?))
            }
            Self::All(expressions) => {
                for expression in expressions {
                    if !expression.eval(feature)?.expect_bool()? {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
            Self::Any(expressions) => {
                for expression in expressions {
                    if expression.eval(feature)?.expect_bool()? {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            Self::Not(expression) => Ok(Value::Bool(!expression.eval(feature)?.expect_bool()?)),
            Self::Match {
                input,
                branches,
                fallback,
            } => {
                let input = input.eval(feature)?;
                for (labels, output) in branches {
                    if labels.contains(&input) {
                        return output.eval(feature);
                    }
                }
                fallback.eval(feature)
            }
            Self::Coalesce(expressions) => {
                for expression in expressions {
                    let value = expression.eval(feature)?;
                    if value != Value::Null {
                        return Ok(value);
                    }
                }
                Ok(Value::Null)
            }
            Self::ToString(expression) => Ok(Value::String(expression.eval(feature)?.text()?)),
            Self::Concat(expressions) => {
                let mut text = String::new();
                for expression in expressions {
                    text.push_str(&expression.eval(feature)?.text()?);
                }
                Ok(Value::String(text))
            }
            Self::Case { branches, fallback } => {
                for (condition, output) in branches {
                    if condition.eval(feature)?.expect_bool()? {
                        return output.eval(feature);
                    }
                }
                fallback.eval(feature)
            }
            Self::ToBoolean(expression) => Ok(Value::Bool(expression.eval(feature)?.to_boolean())),
            Self::ToNumber(expressions) => {
                for expression in expressions {
                    if let Some(number) = expression.eval(feature)?.to_number() {
                        return Ok(Value::Number(number));
                    }
                }
                Err(EvalError::new("to-number could not convert any input"))
            }
            Self::TypeAssert(expected, expressions) => {
                for expression in expressions {
                    let value = expression.eval(feature)?;
                    if expected.matches(&value) {
                        return Ok(value);
                    }
                }
                Err(EvalError::new(format!(
                    "{} assertion failed",
                    expected.as_str()
                )))
            }
            Self::Math(op, expressions) => eval_math(*op, expressions, feature),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OrderedOp {
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
}

impl OrderedOp {
    fn as_str(self) -> &'static str {
        match self {
            Self::GreaterThan => ">",
            Self::GreaterThanOrEqual => ">=",
            Self::LessThan => "<",
            Self::LessThanOrEqual => "<=",
        }
    }

    fn matches(self, ordering: Ordering) -> bool {
        match self {
            Self::GreaterThan => ordering == Ordering::Greater,
            Self::GreaterThanOrEqual => matches!(ordering, Ordering::Greater | Ordering::Equal),
            Self::LessThan => ordering == Ordering::Less,
            Self::LessThanOrEqual => matches!(ordering, Ordering::Less | Ordering::Equal),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Type {
    Boolean,
    Number,
    String,
}

impl Type {
    fn as_str(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::String => "string",
        }
    }

    fn matches(self, value: &Value) -> bool {
        matches!(
            (self, value),
            (Self::Boolean, Value::Bool(_))
                | (Self::Number, Value::Number(_))
                | (Self::String, Value::String(_))
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MathOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}
impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}
impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl Value {
    fn text(self) -> Result<String, EvalError> {
        match self {
            Self::Null => Ok(String::new()),
            Self::String(value) => Ok(value),
            Self::Bool(value) => Ok(value.to_string()),
            Self::Number(value) if value.is_finite() => Ok(value.to_string()),
            _ => Err(EvalError::new("to-string requires a finite scalar value")),
        }
    }

    pub(crate) fn expect_bool(self) -> Result<bool, EvalError> {
        match self {
            Self::Bool(value) => Ok(value),
            other => Err(EvalError::new(format!(
                "expected boolean, got {}",
                other.type_name()
            ))),
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }

    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => left.partial_cmp(right),
            (Self::String(left), Self::String(right)) => Some(left.cmp(right)),
            _ => None,
        }
    }

    fn to_boolean(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(value) => *value,
            Self::Number(value) => *value != 0.0 && !value.is_nan(),
            Self::String(value) => !value.is_empty(),
            Self::Array(_) | Self::Object(_) => true,
        }
    }

    fn to_number(&self) -> Option<f64> {
        match self {
            Self::Null => Some(0.0),
            Self::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
            Self::Number(value) => Some(*value),
            Self::String(value) => value.parse().ok(),
            Self::Array(_) | Self::Object(_) => None,
        }
    }
}

fn value_in(needle: &Value, haystack: &Value) -> Result<bool, EvalError> {
    match haystack {
        Value::Array(values) => Ok(values.iter().any(|value| value == needle)),
        Value::String(haystack) => match needle {
            Value::String(needle) => Ok(haystack.contains(needle)),
            _ => Err(EvalError::new("string haystack requires a string needle")),
        },
        _ => Err(EvalError::new(format!(
            "in requires an array or string haystack, got {}",
            haystack.type_name()
        ))),
    }
}

fn eval_math<F: Feature + ?Sized>(
    op: MathOp,
    expressions: &[Expr],
    feature: &F,
) -> Result<Value, EvalError> {
    let mut numbers = expressions
        .iter()
        .map(|expression| {
            let value = expression.eval(feature)?;
            match value {
                Value::Number(value) => Ok(value),
                other => Err(EvalError::new(format!(
                    "math expression expected number, got {}",
                    other.type_name()
                ))),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let first = numbers.remove(0);
    let value = match op {
        MathOp::Add => numbers.into_iter().fold(first, |left, right| left + right),
        MathOp::Subtract if numbers.is_empty() => -first,
        MathOp::Subtract => numbers.into_iter().fold(first, |left, right| left - right),
        MathOp::Multiply => numbers.into_iter().fold(first, |left, right| left * right),
        MathOp::Divide => numbers.into_iter().fold(first, |left, right| left / right),
        MathOp::Remainder => {
            if numbers.len() != 1 {
                return Err(EvalError::new("% requires exactly two arguments"));
            }
            first % numbers[0]
        }
    };
    Ok(Value::Number(value))
}
