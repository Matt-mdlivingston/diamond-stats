//! Serde shapes for MLB Stats API JSON.
//!
//! These types mirror the wire format, including stringly-typed rates and the
//! collapsed multi-team season row. They are not the types the query API returns.
//! Unknown fields are ignored so a new API field does not break ingest.

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};

use crate::rates::parse_api_decimal;

/// A counting stat. Missing and JSON null become zero. Strings of digits are accepted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Count(pub u32);

impl<'de> Deserialize<'de> for Count {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = u32;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an integer count, a numeric string, or null")
            }

            fn visit_unit<E: de::Error>(self) -> Result<u32, E> {
                Ok(0)
            }

            fn visit_none<E: de::Error>(self) -> Result<u32, E> {
                Ok(0)
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<u32, E> {
                u32::try_from(value).map_err(E::custom)
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<u32, E> {
                u32::try_from(value).map_err(E::custom)
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<u32, E> {
                if !value.is_finite() || value < 0.0 {
                    return Err(E::custom("invalid count"));
                }
                let rounded = value.round();
                if (rounded - value).abs() > 1e-6 {
                    return Err(E::custom("fractional count"));
                }
                u32::try_from(rounded as u64).map_err(E::custom)
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<u32, E> {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Ok(0);
                }
                if let Ok(parsed) = trimmed.parse::<u32>() {
                    return Ok(parsed);
                }
                if let Ok(parsed) = trimmed.parse::<f64>() {
                    return self.visit_f64(parsed);
                }
                Err(E::custom(format!("invalid count '{value}'")))
            }
        }
        deserializer.deserialize_any(V).map(Count)
    }
}

/// A rate that may be a number, a numeric string, or an undefined sentinel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct ApiDecimal(pub Option<f64>);

impl<'de> Deserialize<'de> for ApiDecimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Option<f64>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a rate, or an undefined sentinel such as '.---'")
            }

            fn visit_unit<E: de::Error>(self) -> Result<Option<f64>, E> {
                Ok(None)
            }

            fn visit_none<E: de::Error>(self) -> Result<Option<f64>, E> {
                Ok(None)
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Option<f64>, E> {
                Ok(value.is_finite().then_some(value))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Option<f64>, E> {
                Ok(Some(value as f64))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Option<f64>, E> {
                Ok(Some(value as f64))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Option<f64>, E> {
                Ok(parse_api_decimal(value))
            }
        }
        deserializer.deserialize_any(V).map(ApiDecimal)
    }
}

fn de_opt_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    struct V;
    impl Visitor<'_> for V {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a string, number, or null")
        }

        fn visit_unit<E: de::Error>(self) -> Result<Option<String>, E> {
            Ok(None)
        }

        fn visit_none<E: de::Error>(self) -> Result<Option<String>, E> {
            Ok(None)
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Option<String>, E> {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<Option<String>, E> {
            if !value.is_finite() {
                return Ok(None);
            }
            let tenths = (value * 10.0).round() as i64;
            let whole = tenths.div_euclid(10);
            let tenth = tenths.rem_euclid(10);
            Ok(Some(format!("{whole}.{tenth}")))
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Option<String>, E> {
            Ok(Some(format!("{value}.0")))
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<Option<String>, E> {
            Ok(Some(format!("{value}.0")))
        }
    }
    deserializer.deserialize_any(V)
}

/// The stat object inside a split. Hitting and pitching share one struct;
/// fields that do not apply default to zero or `None`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RawStat {
    /// Games played.
    pub games_played: Count,
    /// Games started.
    pub games_started: Count,
    /// At-bats.
    pub at_bats: Count,
    /// Plate appearances.
    pub plate_appearances: Count,
    /// Runs.
    pub runs: Count,
    /// Hits.
    pub hits: Count,
    /// Doubles.
    pub doubles: Count,
    /// Triples.
    pub triples: Count,
    /// Home runs.
    pub home_runs: Count,
    /// Runs batted in.
    pub rbi: Count,
    /// Walks.
    pub base_on_balls: Count,
    /// Intentional walks, already included in walks.
    pub intentional_walks: Count,
    /// Strikeouts.
    pub strike_outs: Count,
    /// Hit by pitch.
    pub hit_by_pitch: Count,
    /// Stolen bases.
    pub stolen_bases: Count,
    /// Caught stealing.
    pub caught_stealing: Count,
    /// Sacrifice flies.
    pub sac_flies: Count,
    /// Sacrifice bunts.
    pub sac_bunts: Count,
    /// Total bases.
    pub total_bases: Count,
    /// Grounded into double plays.
    pub ground_into_double_play: Count,
    /// Batting average, or opponent average on a pitching line.
    pub avg: ApiDecimal,
    /// On-base percentage.
    pub obp: ApiDecimal,
    /// Slugging.
    pub slg: ApiDecimal,
    /// OPS.
    pub ops: ApiDecimal,
    /// Stolen-base percentage. Often the sentinel `.---`.
    pub stolen_base_percentage: ApiDecimal,
    /// Wins.
    pub wins: Count,
    /// Losses.
    pub losses: Count,
    /// Saves.
    pub saves: Count,
    /// Save opportunities.
    pub save_opportunities: Count,
    /// Holds.
    pub holds: Count,
    /// Blown saves.
    pub blown_saves: Count,
    /// Earned runs.
    pub earned_runs: Count,
    /// Batters faced.
    pub batters_faced: Count,
    /// Outs recorded.
    pub outs: Count,
    /// Games pitched.
    pub games_pitched: Count,
    /// Complete games.
    pub complete_games: Count,
    /// Shutouts.
    pub shutouts: Count,
    /// Strikes.
    pub strikes: Count,
    /// Hit batsmen.
    pub hit_batsmen: Count,
    /// Balks.
    pub balks: Count,
    /// Wild pitches.
    pub wild_pitches: Count,
    /// Pitches.
    pub number_of_pitches: Count,
    /// ERA. On a game log this is season-to-date, not the single game.
    pub era: ApiDecimal,
    /// WHIP. On a game log this is season-to-date, not the single game.
    pub whip: ApiDecimal,
    /// Innings pitched in baseball notation.
    #[serde(default, deserialize_with = "de_opt_string")]
    pub innings_pitched: Option<String>,
    /// Game-log summary line.
    pub summary: Option<String>,
}

/// `id` + `name` reference used for teams and opponents.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RefId {
    /// Numeric id.
    pub id: u32,
    /// Display name.
    pub name: Option<String>,
}

/// Player reference embedded on a stat split.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRef {
    /// MLB person id.
    pub id: u32,
    /// Full name. Game logs sometimes omit the split first/last fields.
    #[serde(default)]
    pub full_name: Option<String>,
    /// First name.
    #[serde(default)]
    pub first_name: Option<String>,
    /// Last name.
    #[serde(default)]
    pub last_name: Option<String>,
}

impl PlayerRef {
    /// Best display name available on this reference.
    pub fn display_name(&self) -> String {
        if let Some(name) = self.full_name.as_ref().filter(|name| !name.is_empty()) {
            return name.clone();
        }
        match (&self.first_name, &self.last_name) {
            (Some(first), Some(last)) => format!("{first} {last}"),
            (None, Some(last)) => last.clone(),
            (Some(first), None) => first.clone(),
            (None, None) => format!("player {}", self.id),
        }
    }
}

/// Position object. The JSON field `type` is renamed because it is a keyword.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PositionRef {
    /// Position code.
    pub code: Option<String>,
    /// Position name.
    pub name: Option<String>,
    /// Abbreviation (`CF`, `P`, `1B`).
    pub abbreviation: Option<String>,
    /// Position family. The wire field is `type`.
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// Game pointer on a game log split.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRef {
    /// Stats API game primary key.
    pub game_pk: u64,
    /// `day` or `night`.
    pub day_night: Option<String>,
    /// Game number of a doubleheader.
    pub game_number: Option<u32>,
}

/// One split inside a stats response.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StatSplit {
    /// Season, as a string (`"2025"`).
    pub season: Option<String>,
    /// Counting and rate stats.
    #[serde(default)]
    pub stat: RawStat,
    /// Team. Absent on the people-endpoint combined season row.
    pub team: Option<RefId>,
    /// Player.
    pub player: Option<PlayerRef>,
    /// Opponent, on game logs.
    pub opponent: Option<RefId>,
    /// Sport.
    pub sport: Option<SportRef>,
    /// Primary position.
    pub position: Option<PositionRef>,
    /// Positions played in a game.
    #[serde(default)]
    pub positions_played: Vec<PositionRef>,
    /// How many teams the counting stats cover.
    ///
    /// On the sport-wide season endpoint this is set and the row is a season
    /// total labeled with a single team. On per-team stints it is omitted.
    pub num_teams: Option<u32>,
    /// Game date, `YYYY-MM-DD`.
    pub date: Option<String>,
    /// Home team flag.
    pub is_home: Option<bool>,
    /// Team win flag.
    pub is_win: Option<bool>,
    /// Game identity.
    pub game: Option<GameRef>,
    /// Game type, usually `R` for the regular season.
    pub game_type: Option<String>,
}

/// One stat group inside a response (`stats[0]`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsBlock {
    /// Rows available for this query. Used to know when pagination is finished.
    #[serde(default)]
    pub total_splits: Option<u32>,
    /// The page of rows. This is the only array ingest keeps.
    #[serde(default)]
    pub splits: Vec<StatSplit>,
    /// Players tied with the first row of this page. They also appear on the previous page.
    #[serde(default)]
    pub splits_tied_with_offset: Vec<StatSplit>,
    /// Players tied with the last row of this page. They also appear on the next page.
    #[serde(default)]
    pub splits_tied_with_limit: Vec<StatSplit>,
}

/// Top-level `/stats` or `/people/{id}/stats` document.
#[derive(Debug, Clone, Deserialize)]
pub struct StatsEnvelope {
    /// MLBAM copyright notice. Retained so we do not pretend the payload is ours.
    #[serde(default)]
    pub copyright: String,
    /// Stat groups. A single-group request has one element.
    #[serde(default)]
    pub stats: Vec<StatsBlock>,
}

/// Named league, division, or similar reference.
#[derive(Debug, Clone, Deserialize)]
pub struct NamedRef {
    /// Id, when present.
    pub id: Option<u32>,
    /// Name.
    pub name: Option<String>,
}

/// Sport reference on a team or a split.
#[derive(Debug, Clone, Deserialize)]
pub struct SportRef {
    /// `sportId`.
    pub id: Option<u32>,
    /// Name.
    pub name: Option<String>,
    /// Abbreviation.
    pub abbreviation: Option<String>,
}

/// A team object from `GET /api/v1/teams`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTeam {
    /// Team id.
    pub id: u32,
    /// Full name.
    pub name: String,
    /// Abbreviation.
    #[serde(default)]
    pub abbreviation: Option<String>,
    /// Nickname.
    #[serde(default)]
    pub team_name: Option<String>,
    /// Location.
    #[serde(default)]
    pub location_name: Option<String>,
    /// Season. The teams endpoint sends this as a number.
    #[serde(default)]
    pub season: Option<u16>,
    /// League.
    #[serde(default)]
    pub league: Option<NamedRef>,
    /// Division.
    #[serde(default)]
    pub division: Option<NamedRef>,
    /// Sport.
    #[serde(default)]
    pub sport: Option<SportRef>,
    /// Parent organization id for an affiliate.
    #[serde(default)]
    pub parent_org_id: Option<u32>,
    /// Parent organization name.
    #[serde(default)]
    pub parent_org_name: Option<String>,
    /// Whether the club is active.
    #[serde(default)]
    pub active: Option<bool>,
}

/// `GET /api/v1/teams` document.
#[derive(Debug, Clone, Deserialize)]
pub struct TeamsEnvelope {
    /// MLBAM copyright notice.
    #[serde(default)]
    pub copyright: String,
    /// Clubs.
    #[serde(default)]
    pub teams: Vec<ApiTeam>,
}

/// One sport from `GET /api/v1/sports`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSport {
    /// `sportId`.
    pub id: u32,
    /// Short code (`mlb`, `aax`, `min`, ...).
    pub code: String,
    /// Name.
    pub name: String,
    /// Abbreviation.
    pub abbreviation: String,
    /// Whether the sport is active.
    pub active_status: bool,
}

/// `GET /api/v1/sports` document.
#[derive(Debug, Clone, Deserialize)]
pub struct SportsEnvelope {
    /// MLBAM copyright notice.
    #[serde(default)]
    pub copyright: String,
    /// Sports.
    #[serde(default)]
    pub sports: Vec<ApiSport>,
}
