//! Hitting vs pitching. Fielding is intentionally out of milestone 1.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::{ObjectBuilder, Schema, SchemaType, Type};
use utoipa::{PartialSchema, ToSchema};

use crate::error::CoreError;

/// Which stat block a row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatGroup {
    /// Batting, including pitcher batting lines in the National League.
    Hitting,
    /// Pitching.
    Pitching,
}

impl StatGroup {
    /// Wire value used in query strings and JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            StatGroup::Hitting => "hitting",
            StatGroup::Pitching => "pitching",
        }
    }

    /// Parse `hitting` or `pitching`.
    pub fn parse(raw: &str) -> Result<Self, CoreError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "hitting" | "batting" | "hit" => Ok(StatGroup::Hitting),
            "pitching" | "pitch" => Ok(StatGroup::Pitching),
            other => Err(CoreError::Message(format!(
                "group must be hitting or pitching, got '{other}'"
            ))),
        }
    }
}

impl fmt::Display for StatGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for StatGroup {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for StatGroup {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl PartialSchema for StatGroup {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(SchemaType::new(Type::String))
            .enum_values(Some(["hitting", "pitching"]))
            .build()
            .into()
    }
}

impl ToSchema for StatGroup {
    fn name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("StatGroup")
    }
}

impl<'de> Deserialize<'de> for StatGroup {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}
