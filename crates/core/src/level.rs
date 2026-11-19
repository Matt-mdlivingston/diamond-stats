//! Competitive levels and the MLB Stats API `sportId` values they map to.
//!
//! Verified against `GET /api/v1/sports` on 2026-11-19. `sportId` 15 is absent.
//! `sportId` 21 ("Minor League Baseball") is active in `/sports` but a season
//! stats query for it returns `totalSplits: 0`, so it is not an "all minors" id.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::openapi::RefOr;
use utoipa::openapi::schema::{ObjectBuilder, Schema, SchemaType, Type};
use utoipa::{PartialSchema, ToSchema};

use crate::error::CoreError;

/// A level diamond-stats knows how to name and query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Level {
    /// Major League Baseball. `sportId` 1, API code `mlb`.
    Mlb,
    /// Triple-A. `sportId` 11, API code `aaa`.
    Aaa,
    /// Double-A. `sportId` 12, API code `aax`.
    Aa,
    /// High-A. `sportId` 13, API code `afa`.
    HighA,
    /// Single-A. `sportId` 14, API code `afx`.
    SingleA,
    /// Rookie. `sportId` 16, API code `rok`.
    Rookie,
}

/// Static description of a level, suitable for `GET /v1/levels`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelInfo {
    /// URL and storage slug (`mlb`, `aaa`, `high_a`, ...).
    pub slug: &'static str,
    /// Display name.
    pub name: &'static str,
    /// MLB Stats API `sportId`.
    pub sport_id: u32,
    /// `code` field on `GET /api/v1/sports`.
    pub api_code: &'static str,
    /// `abbreviation` field on `GET /api/v1/sports`.
    pub api_abbreviation: &'static str,
}

impl Level {
    /// Every level diamond-stats ingests. MLB through Rookie use one code path;
    /// `sportId` selects the level.
    pub const ALL: [Level; 6] = [
        Level::Mlb,
        Level::Aaa,
        Level::Aa,
        Level::HighA,
        Level::SingleA,
        Level::Rookie,
    ];

    /// Metadata for this level.
    pub fn info(self) -> LevelInfo {
        match self {
            Level::Mlb => LevelInfo {
                slug: "mlb",
                name: "Major League Baseball",
                sport_id: 1,
                api_code: "mlb",
                api_abbreviation: "MLB",
            },
            Level::Aaa => LevelInfo {
                slug: "aaa",
                name: "Triple-A",
                sport_id: 11,
                api_code: "aaa",
                api_abbreviation: "AAA",
            },
            Level::Aa => LevelInfo {
                slug: "aa",
                name: "Double-A",
                sport_id: 12,
                api_code: "aax",
                api_abbreviation: "AA",
            },
            Level::HighA => LevelInfo {
                slug: "high_a",
                name: "High-A",
                sport_id: 13,
                api_code: "afa",
                api_abbreviation: "A+",
            },
            Level::SingleA => LevelInfo {
                slug: "single_a",
                name: "Single-A",
                sport_id: 14,
                api_code: "afx",
                api_abbreviation: "A",
            },
            Level::Rookie => LevelInfo {
                slug: "rookie",
                name: "Rookie",
                sport_id: 16,
                api_code: "rok",
                api_abbreviation: "ROK",
            },
        }
    }

    /// Storage and URL slug.
    pub fn slug(self) -> &'static str {
        self.info().slug
    }

    /// MLB Stats API `sportId`.
    pub fn sport_id(self) -> u32 {
        self.info().sport_id
    }

    /// Map a verified `sportId` to a level.
    ///
    /// Returns `None` for inactive id 15, the empty "all minors" id 21, and
    /// every other sport the Stats API lists (winter leagues, NPB, college, ...).
    pub fn from_sport_id(sport_id: u32) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|level| level.sport_id() == sport_id)
    }

    /// Parse a CLI token or query parameter.
    pub fn parse(raw: &str) -> Result<Self, CoreError> {
        let token = raw.trim().to_ascii_lowercase().replace('-', "_");
        let level = match token.as_str() {
            "mlb" | "1" | "major" => Level::Mlb,
            "aaa" | "11" | "triple_a" | "triplea" => Level::Aaa,
            "aa" | "12" | "double_a" | "doublea" => Level::Aa,
            "high_a" | "13" | "a+" | "higha" => Level::HighA,
            "single_a" | "14" | "a" | "low_a" | "singlea" => Level::SingleA,
            "rookie" | "rok" | "rk" | "16" => Level::Rookie,
            "15" | "21" | "minors" | "milb" | "all_minors" => {
                return Err(CoreError::UnknownLevel(format!(
                    "{raw} (sportId 15 is inactive and sportId 21 does not return minor-league players)"
                )));
            }
            _ => return Err(CoreError::UnknownLevel(raw.to_string())),
        };
        Ok(level)
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

impl FromStr for Level {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for Level {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.slug())
    }
}

impl PartialSchema for Level {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(SchemaType::new(Type::String))
            .enum_values(Some(["mlb", "aaa", "aa", "high_a", "single_a", "rookie"]))
            .description(Some(
                "Competitive level slug. sportId 15 and 21 are intentionally absent.",
            ))
            .build()
            .into()
    }
}

impl ToSchema for Level {
    fn name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("Level")
    }
}

impl<'de> Deserialize<'de> for Level {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sport_ids_match_the_verified_map() {
        assert_eq!(Level::Mlb.sport_id(), 1);
        assert_eq!(Level::Aaa.sport_id(), 11);
        assert_eq!(Level::Aa.sport_id(), 12);
        assert_eq!(Level::HighA.sport_id(), 13);
        assert_eq!(Level::SingleA.sport_id(), 14);
        assert_eq!(Level::Rookie.sport_id(), 16);
        assert!(Level::from_sport_id(15).is_none());
        assert!(Level::from_sport_id(21).is_none());
    }

    #[test]
    fn aliases_parse_and_dead_ids_do_not() {
        assert_eq!(Level::parse("high-a").unwrap(), Level::HighA);
        assert_eq!(Level::parse("AAA").unwrap(), Level::Aaa);
        assert!(Level::parse("21").is_err());
        assert!(Level::parse("minors").is_err());
    }
}
