// JSON value helpers that replicate zod `Schema.Finite` / `Schema.Int` behavior
// and JavaScript `JSON.stringify` number rendering for golden-test parity.

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serializer};

/// Deserializes a required `Finite` number (`zod` `Schema.Finite`) as `f64`.
pub fn de_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = f64::deserialize(deserializer)?;
    if !value.is_finite() {
        return Err(de::Error::custom("Expected a finite number"));
    }
    Ok(value)
}

/// Deserializes an optional `Finite` number (`zod` `Schema.Finite`) as `f64`.
pub fn de_f64_opt<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<f64>::deserialize(deserializer)?;
    if let Some(value) = value {
        if !value.is_finite() {
            return Err(de::Error::custom("Expected a finite number"));
        }
    }
    Ok(value)
}

/// `Schema.Int.check(Schema.isBetween({ minimum: 1, maximum: 65535 }))` for an
/// optional `u16` field.
pub fn de_port_opt<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<u16>::deserialize(deserializer)?;
    if let Some(value) = value {
        if value == 0 {
            return Err(de::Error::custom("Expected a port between 1 and 65535"));
        }
    }
    Ok(value)
}

/// Serializes an `f64` the way JavaScript `JSON.stringify` does: integral
/// values within `[-1e21, 1e21)` render without a decimal point (`5` not
/// `5.0`), everything else uses the shortest round-tripping form.
pub fn serialize_js_number<S>(value: &f64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e21 {
        serializer.serialize_i64(*value as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

/// `serialize_js_number` for `Option<f64>` fields.
pub fn serialize_js_number_opt<S>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(value) => serialize_js_number(value, serializer),
        None => serializer.serialize_none(),
    }
}

/// Positive integer (`zod`: `Schema.Int.check(Schema.isGreaterThan(0))`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PositiveInt(pub u64);

impl PositiveInt {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl serde::Serialize for PositiveInt {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for PositiveInt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = deserializer.deserialize_any(U64Visitor)?;
        if value == 0 {
            return Err(de::Error::custom(
                "Expected a positive integer (greater than 0)",
            ));
        }
        Ok(PositiveInt(value))
    }
}

/// Non-negative integer (`zod`: `Schema.Int.check(Schema.isGreaterThanOrEqualTo(0))`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NonNegativeInt(pub u64);

impl NonNegativeInt {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl serde::Serialize for NonNegativeInt {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for NonNegativeInt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(U64Visitor).map(NonNegativeInt)
    }
}

struct U64Visitor;

impl<'de> Visitor<'de> for U64Visitor {
    type Value = u64;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an integer")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(value)
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        u64::try_from(value).map_err(|_| de::Error::custom("Expected a non-negative integer"))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        if value.fract() == 0.0 && value >= 0.0 && value <= u64::MAX as f64 {
            Ok(value as u64)
        } else {
            Err(de::Error::custom("Expected an integer"))
        }
    }
}
