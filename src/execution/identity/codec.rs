use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// The restricted `execution-json-v1` data model. Objects use ASCII keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalValue {
    /// Explicit absence.
    Null,
    /// A boolean.
    Bool(bool),
    /// A nonnegative integer, with no alternate numeric spelling.
    Integer(u64),
    /// Exact Unicode text, without normalization.
    String(String),
    /// An ordered sequence.
    Array(Vec<Self>),
    /// A unique-key object, encoded in unsigned ASCII order.
    Object(BTreeMap<String, Self>),
}

/// Invalid restricted data, closed record shape, or canonical spelling.
/// Error text never includes rejected input or private launch data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("Invalid execution-json-v1 value or record.")]
pub struct CanonicalError;

impl CanonicalValue {
    /// Parse restricted JSON, rejecting duplicate fields before constructing a map.
    /// This permits formatting whitespace; use `decode` for persisted artifacts.
    pub fn parse(bytes: &[u8]) -> Result<Self, CanonicalError> {
        serde_json::from_slice(bytes).map_err(|_| CanonicalError)
    }

    /// Decode only the unique canonical byte spelling, including all escapes.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let value = Self::parse(bytes)?;
        if value.encode()? != bytes {
            return Err(CanonicalError);
        }
        Ok(value)
    }

    /// Project an already constructed JSON value; duplicates must be rejected upstream.
    pub fn from_json(value: serde_json::Value) -> Result<Self, CanonicalError> {
        Self::parse(&serde_json::to_vec(&value).map_err(|_| CanonicalError)?)
    }

    /// Encode without relying on a general JSON serializer's ordering or escaping.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        let mut output = String::new();
        self.write(&mut output, 0)?;
        Ok(output.into_bytes())
    }

    /// Require exactly the listed keys. Consumers apply this at every record depth.
    pub fn fields(&self, names: &[&str]) -> Result<&BTreeMap<String, Self>, CanonicalError> {
        let Self::Object(fields) = self else {
            return Err(CanonicalError);
        };
        if fields.len() != names.len() || names.iter().any(|name| !fields.contains_key(*name)) {
            return Err(CanonicalError);
        }
        Ok(fields)
    }

    fn write(&self, output: &mut String, depth: usize) -> Result<(), CanonicalError> {
        if depth > 128 {
            return Err(CanonicalError);
        }
        match self {
            Self::Null => output.push_str("null"),
            Self::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Self::Integer(value) => output.push_str(&value.to_string()),
            Self::String(value) => write_string(value, output),
            Self::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    value.write(output, depth + 1)?;
                }
                output.push(']');
            }
            Self::Object(values) => {
                output.push('{');
                for (index, (key, value)) in values.iter().enumerate() {
                    if !key.is_ascii() {
                        return Err(CanonicalError);
                    }
                    if index != 0 {
                        output.push(',');
                    }
                    write_string(key, output);
                    output.push(':');
                    value.write(output, depth + 1)?;
                }
                output.push('}');
            }
        }
        Ok(())
    }
}

fn write_string(value: &str, output: &mut String) {
    const HEX: &[u8] = b"0123456789abcdef";
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\0'..='\u{1f}' => {
                let byte = character as usize;
                output.push_str("\\u00");
                output.push(HEX[byte >> 4] as char);
                output.push(HEX[byte & 15] as char);
            }
            other => output.push(other),
        }
    }
    output.push('"');
}

impl<'de> Deserialize<'de> for CanonicalValue {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Restricted;
        impl<'de> Visitor<'de> for Restricted {
            type Value = CanonicalValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an execution-json-v1 value")
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(CanonicalValue::Null)
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(CanonicalValue::Bool(v))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(CanonicalValue::Integer(v))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(CanonicalValue::String(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(CanonicalValue::String(v))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? {
                    values.push(value);
                }
                Ok(CanonicalValue::Array(values))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !key.is_ascii() || values.contains_key(&key) {
                        return Err(de::Error::custom(CanonicalError));
                    }
                    values.insert(key, map.next_value()?);
                }
                Ok(CanonicalValue::Object(values))
            }
        }
        deserializer.deserialize_any(Restricted)
    }
}

/// SHA-256 of the exact bytes, with the portable algorithm prefix.
pub fn content_digest(bytes: &[u8]) -> String {
    let fingerprint = crate::provenance::fingerprint_bytes(bytes);
    format!("{}:{}", fingerprint.algorithm, fingerprint.value)
}

/// Domain-separated SHA-256 with one literal NUL before the canonical value.
pub fn domain_digest(domain: &str, value: &CanonicalValue) -> Result<String, CanonicalError> {
    if domain.contains('\0') {
        return Err(CanonicalError);
    }
    let mut hash = Sha256::new();
    hash.update(domain.as_bytes());
    hash.update([0]);
    hash.update(value.encode()?);
    Ok(digest_string(&hash.finalize()))
}

fn digest_string(bytes: &[u8]) -> String {
    let mut result = String::from("sha256:");
    for byte in bytes {
        use std::fmt::Write;
        write!(result, "{byte:02x}").expect("writing a String cannot fail");
    }
    result
}
