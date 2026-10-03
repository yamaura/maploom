use super::{Expr, Filter, MathOp, OrderedOp, Type, Value};
use crate::serde_support::{arguments, finish, required};
use serde::de::{Error, IntoDeserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

impl Serialize for Filter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.expression.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Filter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Expr::deserialize(deserializer).map(|expression| Self { expression })
    }
}

macro_rules! scalar_visits {
    ($wrap:expr) => {
        fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
            Ok(($wrap)(Value::Null))
        }
        fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
            self.visit_unit()
        }
        fn visit_bool<E: Error>(self, value: bool) -> Result<Self::Value, E> {
            Ok(($wrap)(Value::Bool(value)))
        }
        fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> {
            self.visit_f64(value as f64)
        }
        fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> {
            self.visit_f64(value as f64)
        }
        fn visit_f64<E: Error>(self, value: f64) -> Result<Self::Value, E> {
            if !value.is_finite() {
                return Err(E::custom("expression numbers must be finite"));
            }
            Ok(($wrap)(Value::Number(value)))
        }
        fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
            self.visit_string(value.to_owned())
        }
        fn visit_string<E: Error>(self, value: String) -> Result<Self::Value, E> {
            Ok(($wrap)(Value::String(value)))
        }
    };
}

struct ExprVisitor;

impl<'de> Visitor<'de> for ExprVisitor {
    type Value = Expr;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a literal or an expression beginning with an operator")
    }

    scalar_visits!(Expr::Literal);

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Expr, A::Error> {
        ValueVisitor.visit_map(map).map(Expr::Literal)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Expr, A::Error> {
        let operator: String = required(&mut seq)?;
        let expression = match operator.as_str() {
            "literal" => Expr::Literal(required(&mut seq)?),
            "get" => Expr::Get(required(&mut seq)?),
            "has" => Expr::Has(required(&mut seq)?),
            "id" => Expr::Id,
            "geometry-type" => Expr::GeometryType,
            "==" => Expr::Eq(required(&mut seq)?, required(&mut seq)?),
            "!=" => Expr::Ne(required(&mut seq)?, required(&mut seq)?),
            ">" | ">=" | "<" | "<=" => {
                let op = match operator.as_str() {
                    ">" => OrderedOp::GreaterThan,
                    ">=" => OrderedOp::GreaterThanOrEqual,
                    "<" => OrderedOp::LessThan,
                    _ => OrderedOp::LessThanOrEqual,
                };
                Expr::Ordered(op, required(&mut seq)?, required(&mut seq)?)
            }
            "in" => Expr::In(required(&mut seq)?, required(&mut seq)?),
            "!" => Expr::Not(required(&mut seq)?),
            "to-boolean" => Expr::ToBoolean(required(&mut seq)?),
            "all" => return arguments(&mut seq, 0).map(Expr::All),
            "any" => return arguments(&mut seq, 0).map(Expr::Any),
            "coalesce" => return arguments(&mut seq, 0).map(Expr::Coalesce),
            "to-number" => return arguments(&mut seq, 1).map(Expr::ToNumber),
            "boolean" | "number" | "string" => {
                let kind = match operator.as_str() {
                    "boolean" => Type::Boolean,
                    "number" => Type::Number,
                    _ => Type::String,
                };
                return arguments(&mut seq, 0).map(|args| Expr::TypeAssert(kind, args));
            }
            "+" | "-" | "*" | "/" | "%" => {
                let op = match operator.as_str() {
                    "+" => MathOp::Add,
                    "-" => MathOp::Subtract,
                    "*" => MathOp::Multiply,
                    "/" => MathOp::Divide,
                    _ => MathOp::Remainder,
                };
                let minimum = if matches!(op, MathOp::Subtract) { 1 } else { 2 };
                return arguments(&mut seq, minimum).map(|args| Expr::Math(op, args));
            }
            "match" => return read_match(&mut seq),
            _ => {
                return Err(A::Error::custom(format!(
                    "unsupported Maploom filter operator {operator:?}"
                )));
            }
        };
        finish(&mut seq)?;
        Ok(expression)
    }
}

impl<'de> Deserialize<'de> for Expr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ExprVisitor)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MatchArgument {
    Labels(Vec<Scalar>),
    Expression(Expr),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Scalar {
    Null(()),
    Bool(bool),
    Number(f64),
    String(String),
}

impl Scalar {
    fn into_value(self) -> Value {
        match self {
            Self::Null(()) => Value::Null,
            Self::Bool(value) => Value::Bool(value),
            Self::Number(value) => Value::Number(value),
            Self::String(value) => Value::String(value),
        }
    }
}

impl<'de> IntoDeserializer<'de> for Scalar {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de> Deserializer<'de> for Scalar {
    type Error = serde::de::value::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::Null(()) => visitor.visit_unit(),
            Self::Bool(value) => visitor.visit_bool(value),
            Self::Number(value) => visitor.visit_f64(value),
            Self::String(value) => visitor.visit_string(value),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

impl MatchArgument {
    fn into_labels<E: Error>(self) -> Result<Vec<Value>, E> {
        match self {
            Self::Labels(labels) => Ok(labels.into_iter().map(Scalar::into_value).collect()),
            Self::Expression(Expr::Literal(value)) => Ok(vec![value]),
            _ => Err(E::custom("match labels must be literal values")),
        }
    }

    fn into_expression<E: Error>(self) -> Result<Expr, E> {
        match self {
            Self::Expression(expression) => Ok(expression),
            Self::Labels(labels) => {
                Expr::deserialize(serde::de::value::SeqDeserializer::new(labels.into_iter()))
                    .map_err(E::custom)
            }
        }
    }
}

fn read_match<'de, A: SeqAccess<'de>>(seq: &mut A) -> Result<Expr, A::Error> {
    let input = required(seq)?;
    let mut candidate: MatchArgument = required(seq)?;
    let mut branches = Vec::new();
    while let Some(output) = seq.next_element()? {
        branches.push((candidate.into_labels()?, output));
        candidate = required(seq)?;
    }
    if branches.is_empty() {
        return Err(A::Error::custom(
            "match requires at least one branch and a fallback",
        ));
    }
    Ok(Expr::Match {
        input,
        branches,
        fallback: Box::new(candidate.into_expression()?),
    })
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a literal value")
    }

    scalar_visits!(|value| value);

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        arguments(&mut seq, 0).map(Value::Array)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut values = std::collections::BTreeMap::new();
        while let Some((key, value)) = map.next_entry()? {
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Number(value) => serializer.serialize_f64(*value),
            Self::String(value) => serializer.serialize_str(value),
            Self::Array(values) => values.serialize(serializer),
            Self::Object(values) => values.serialize(serializer),
        }
    }
}

struct Operand<'a>(&'a Expr);

impl Serialize for Operand<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Expr::Literal(Value::String(value)) => ("literal", value).serialize(serializer),
            expression => expression.serialize(serializer),
        }
    }
}

impl Serialize for Expr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let operator = match self {
            Self::Literal(value) => match value {
                Value::Array(_) | Value::Object(_) => "literal",
                _ => return value.serialize(serializer),
            },
            Self::Get(_) => "get",
            Self::Has(_) => "has",
            Self::Id => "id",
            Self::GeometryType => "geometry-type",
            Self::Eq(..) => "==",
            Self::Ne(..) => "!=",
            Self::Ordered(op, ..) => op.as_str(),
            Self::In(..) => "in",
            Self::All(_) => "all",
            Self::Any(_) => "any",
            Self::Not(_) => "!",
            Self::Match { .. } => "match",
            Self::Coalesce(_) => "coalesce",
            Self::ToBoolean(_) => "to-boolean",
            Self::ToNumber(_) => "to-number",
            Self::TypeAssert(kind, _) => kind.as_str(),
            Self::Math(op, _) => match op {
                MathOp::Add => "+",
                MathOp::Subtract => "-",
                MathOp::Multiply => "*",
                MathOp::Divide => "/",
                MathOp::Remainder => "%",
            },
        };
        let mut seq = serializer.serialize_seq(None)?;
        seq.serialize_element(operator)?;
        match self {
            Self::Literal(value) => seq.serialize_element(value)?,
            Self::Get(key) | Self::Has(key) => seq.serialize_element(key)?,
            Self::Id | Self::GeometryType => {}
            Self::Eq(left, right)
            | Self::Ne(left, right)
            | Self::Ordered(_, left, right)
            | Self::In(left, right) => {
                seq.serialize_element(&Operand(left))?;
                seq.serialize_element(right)?;
            }
            Self::Not(expr) | Self::ToBoolean(expr) => seq.serialize_element(expr)?,
            Self::All(exprs)
            | Self::Any(exprs)
            | Self::Coalesce(exprs)
            | Self::ToNumber(exprs)
            | Self::TypeAssert(_, exprs)
            | Self::Math(_, exprs) => {
                for expr in exprs {
                    seq.serialize_element(expr)?;
                }
            }
            Self::Match {
                input,
                branches,
                fallback,
            } => {
                seq.serialize_element(input)?;
                for (labels, output) in branches {
                    if labels.len() == 1 {
                        seq.serialize_element(&labels[0])?;
                    } else {
                        seq.serialize_element(labels)?;
                    }
                    seq.serialize_element(output)?;
                }
                seq.serialize_element(fallback)?;
            }
        }
        seq.end()
    }
}
