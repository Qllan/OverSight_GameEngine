//! Values - operations on the JSON values the VM works with
//!
//! Integers stay integers while results are exact (`7 / 2` is `3.5`,
//! `6 / 2` is `3`). `+` joins text when either side is a string, and
//! concatenates arrays. Comparisons work on numbers or on strings.

use serde_json::{Number, Value as JsonValue};
use std::cmp::Ordering;

use crate::compiler::expr::BinaryOp;

/// Truthiness: null, false, 0, "", [] and {} are false
pub fn truthy(value: &JsonValue) -> bool {
    match value {
        JsonValue::Null => false,
        JsonValue::Bool(b) => *b,
        JsonValue::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        JsonValue::String(s) => !s.is_empty(),
        JsonValue::Array(a) => !a.is_empty(),
        JsonValue::Object(o) => !o.is_empty(),
    }
}

/// Equality that treats 1 and 1.0 as equal
pub fn equal(a: &JsonValue, b: &JsonValue) -> bool {
    match (a, b) {
        (JsonValue::Number(x), JsonValue::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// Text shown for a value in `{...}` placeholders: strings as-is, null as
/// nothing, everything else as JSON
pub fn display(value: &JsonValue) -> String {
    match value {
        JsonValue::String(s) => s.clone(),
        JsonValue::Null => String::new(),
        other => other.to_string(),
    }
}

fn type_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

fn float(value: f64) -> Result<JsonValue, String> {
    Number::from_f64(value)
        .map(JsonValue::Number)
        .ok_or_else(|| "result is not a finite number".to_string())
}

/// Apply a binary operator
pub fn binary(op: BinaryOp, a: &JsonValue, b: &JsonValue) -> Result<JsonValue, String> {
    use BinaryOp::*;
    match op {
        Eq => return Ok(JsonValue::Bool(equal(a, b))),
        Ne => return Ok(JsonValue::Bool(!equal(a, b))),
        Lt | Le | Gt | Ge => {
            let ordering = compare(a, b)?;
            let result = match op {
                Lt => ordering == Ordering::Less,
                Le => ordering != Ordering::Greater,
                Gt => ordering == Ordering::Greater,
                _ => ordering != Ordering::Less,
            };
            return Ok(JsonValue::Bool(result));
        }
        _ => {}
    }

    match (op, a, b) {
        (Add, JsonValue::String(_), _) | (Add, _, JsonValue::String(_)) => {
            Ok(JsonValue::String(display(a) + &display(b)))
        }
        (Add, JsonValue::Array(x), JsonValue::Array(y)) => {
            Ok(JsonValue::Array(x.iter().chain(y).cloned().collect()))
        }
        (_, JsonValue::Number(x), JsonValue::Number(y)) => arithmetic(op, x, y),
        _ => Err(format!(
            "cannot apply '{}' to {} and {}",
            op.symbol(),
            type_name(a),
            type_name(b)
        )),
    }
}

fn arithmetic(op: BinaryOp, x: &Number, y: &Number) -> Result<JsonValue, String> {
    use BinaryOp::*;
    if let (Some(a), Some(b)) = (x.as_i64(), y.as_i64()) {
        let exact = match op {
            Add => a.checked_add(b),
            Sub => a.checked_sub(b),
            Mul => a.checked_mul(b),
            Div if b == 0 => return Err("division by zero".to_string()),
            Div if a % b == 0 => a.checked_div(b),
            Div => None,
            Mod if b == 0 => return Err("modulo by zero".to_string()),
            // Result takes the sign of the divisor, as in Python
            Mod => a.checked_rem(b).map(|r| {
                if r != 0 && (r < 0) != (b < 0) {
                    r + b
                } else {
                    r
                }
            }),
            _ => None,
        };
        if let Some(value) = exact {
            return Ok(JsonValue::from(value));
        }
    }

    let (a, b) = (x.as_f64().unwrap_or(0.0), y.as_f64().unwrap_or(0.0));
    match op {
        Add => float(a + b),
        Sub => float(a - b),
        Mul => float(a * b),
        Div if b == 0.0 => Err("division by zero".to_string()),
        Div => float(a / b),
        Mod if b == 0.0 => Err("modulo by zero".to_string()),
        Mod => float(a - b * (a / b).floor()),
        _ => unreachable!("comparisons are handled by the caller"),
    }
}

fn compare(a: &JsonValue, b: &JsonValue) -> Result<Ordering, String> {
    match (a, b) {
        (JsonValue::Number(x), JsonValue::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .ok_or_else(|| "cannot compare numbers".to_string()),
        (JsonValue::String(x), JsonValue::String(y)) => Ok(x.cmp(y)),
        _ => Err(format!(
            "cannot compare {} and {}",
            type_name(a),
            type_name(b)
        )),
    }
}

/// Unary minus
pub fn negate(value: &JsonValue) -> Result<JsonValue, String> {
    match value {
        JsonValue::Number(n) => match n.as_i64() {
            Some(i) => i
                .checked_neg()
                .map(JsonValue::from)
                .ok_or_else(|| "integer overflow".to_string()),
            None => float(-n.as_f64().unwrap_or(0.0)),
        },
        other => Err(format!("cannot negate {}", type_name(other))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use BinaryOp::*;

    fn op(op: BinaryOp, a: JsonValue, b: JsonValue) -> JsonValue {
        binary(op, &a, &b).unwrap()
    }

    #[test]
    fn integer_arithmetic_stays_exact() {
        assert_eq!(op(Add, json!(2), json!(3)), json!(5));
        assert_eq!(op(Div, json!(6), json!(2)), json!(3));
        assert_eq!(op(Div, json!(7), json!(2)), json!(3.5));
        assert_eq!(op(Mod, json!(-7), json!(3)), json!(2));
        assert_eq!(op(Mul, json!(1.5), json!(2)), json!(3.0));
        assert_eq!(negate(&json!(4)).unwrap(), json!(-4));
    }

    #[test]
    fn errors() {
        assert!(binary(Div, &json!(1), &json!(0))
            .unwrap_err()
            .contains("zero"));
        assert!(binary(Sub, &json!("a"), &json!(1))
            .unwrap_err()
            .contains("string and number"));
        assert!(binary(Lt, &json!(1), &json!("2")).is_err());
        assert!(
            binary(Add, &json!(i64::MAX), &json!(1)).is_ok(),
            "overflow falls back to float"
        );
    }

    #[test]
    fn text_and_comparison() {
        assert_eq!(op(Add, json!("Score: "), json!(10)), json!("Score: 10"));
        assert_eq!(op(Add, json!([1]), json!([2])), json!([1, 2]));
        assert_eq!(op(Lt, json!("apple"), json!("banana")), json!(true));
        assert_eq!(op(Ge, json!(5), json!(5.0)), json!(true));
        assert_eq!(op(Eq, json!(1), json!(1.0)), json!(true));
        assert_eq!(display(&json!(null)), "");
        assert_eq!(display(&json!(true)), "true");
    }
}
