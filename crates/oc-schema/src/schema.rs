//! From reference/packages/schema/src/schema.ts

use serde::{de::Deserializer, ser::Serializer, Deserialize, Serialize};

/// `Schema.Int.check(Schema.isGreaterThan(0))`.
pub type PositiveInt = u64;

/// `Schema.Int.check(Schema.isGreaterThanOrEqualTo(0))`.
pub type NonNegativeInt = u64;

/// `Schema.String.pipe(Schema.brand("RelativePath"))`.
pub type RelativePath = String;

/// `Schema.String.pipe(Schema.brand("AbsolutePath"))`.
pub type AbsolutePath = String;

/// `DateTimeUtcFromMillis` — epoch milliseconds. Serializes as an integer.
pub type DateTimeUtc = i64;

/// `Schema.Json` — any JSON-serializable value.
pub type Json = serde_json::Value;

/// A non-empty JSON object used for event payloads whose zod schema is `{}`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Empty {}

/// `Schema.Finite` — a finite float. Serialization mirrors ECMAScript
/// `Number.prototype.toString`, so integral values omit the `.0` suffix and the
/// exponent thresholds match JavaScript (decimal below 1e21 / above 1e-6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finite(pub f64);

impl Finite {
    pub fn new(value: f64) -> Self {
        Finite(value)
    }

    pub fn value(&self) -> f64 {
        self.0
    }
}

impl From<f64> for Finite {
    fn from(value: f64) -> Self {
        Finite(value)
    }
}

impl From<Finite> for f64 {
    fn from(value: Finite) -> Self {
        value.0
    }
}

impl std::ops::Deref for Finite {
    type Target = f64;
    fn deref(&self) -> &f64 {
        &self.0
    }
}

impl Serialize for Finite {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let raw = serde_json::value::RawValue::from_string(format_js_number(self.0))
            .map_err(serde::ser::Error::custom)?;
        raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Finite {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        value
            .as_f64()
            .map(Finite)
            .ok_or_else(|| serde::de::Error::custom("expected a finite number"))
    }
}

/// Format an f64 exactly like ECMAScript `Number.prototype.toString` (the JSON
/// serializer used by the reference). Uses ryu's shortest round-trip digits and
/// then applies the ES decimal/exponential thresholds: decimal when
/// `1e-6 <= |v| < 1e21`, exponential otherwise.
pub fn format_js_number(v: f64) -> String {
    if v.is_nan() || v.is_infinite() {
        return "null".to_string();
    }
    if v == 0.0 {
        return "0".to_string();
    }
    let sign = if v < 0.0 { "-" } else { "" };
    let mut buf = ryu::Buffer::new();
    let s = buf.format(v.abs());
    let s = if s.contains('e') || s.contains('E') {
        s.to_string()
    } else if let Some(stripped) = s.strip_suffix(".0") {
        stripped.to_string()
    } else {
        s.to_string()
    };
    let (mant, exp) = match s.find('e').or_else(|| s.find('E')) {
        Some(i) => (&s[..i], s[i + 1..].parse::<i64>().unwrap()),
        None => (s.as_str(), 0),
    };
    let dot = mant
        .find('.')
        .map(|i| i as i64)
        .unwrap_or(mant.len() as i64);
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = dot + exp;
    let n = digits.len() as i64;
    if k >= 22 || k <= -6 {
        let e = k - 1;
        let mantissa = if n > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits
        };
        format!(
            "{}{}e{}{}",
            sign,
            mantissa,
            if e >= 0 { "+" } else { "" },
            e
        )
    } else if k >= n {
        format!("{}{}{}", sign, digits, "0".repeat((k - n) as usize))
    } else if k > 0 {
        let (a, b) = digits.split_at(k as usize);
        format!("{}{}.{}", sign, a, b)
    } else if k == 0 {
        format!("{}0.{}", sign, digits)
    } else {
        format!("{}0.{}{}", sign, "0".repeat((-k) as usize), digits)
    }
}

/// The `optional(...)` helper from schema.ts. Property is omitted from the
/// serialized object when absent, never encoded as `null`.
#[macro_export]
macro_rules! optional {
    ($field:ident : $ty:ty) => {
        #[serde(skip_serializing_if = "Option::is_none", default)]
        pub $field: Option<$ty>
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_serialization_matches_ecmascript() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "0"),
            (5.0, "5"),
            (0.5, "0.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-7, "1e-7"),
            (1e-6, "0.000001"),
            (0.0001, "0.0001"),
            (9007199254740991.0, "9007199254740991"),
            (1.2345678901234567e19, "12345678901234567000"),
            (100.1, "100.1"),
            (1.5, "1.5"),
            (-5.5, "-5.5"),
            (123.456e89, "1.23456e+91"),
            (1e308, "1e+308"),
            (5e-324, "5e-324"),
            (1.234567890123456e20, "123456789012345600000"),
            (0.0000012345678901234567, "0.0000012345678901234567"),
        ];
        for (value, expected) in cases {
            assert_eq!(&format_js_number(*value), expected, "value {value}");
        }
    }

    #[test]
    fn finite_roundtrips() {
        let value = Finite(1234.0);
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "1234");
        let back: Finite = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value);
        let value = Finite(0.5);
        assert_eq!(serde_json::to_string(&value).unwrap(), "0.5");
    }
}
