//! Serde shapes for schedule, box score, and play-by-play documents.
//!
//! Unknown fields are ignored. Counting stats reuse [`crate::api_model::RawStat`]
//! because the box score uses the same camelCase names as a season line,
//! including baseball innings notation in `inningsPitched`.

use std::collections::HashMap;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};

use crate::api_model::{PlayerRef, PositionRef, RawStat, SportRef};

fn de_opt_u32<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    struct V;
    impl Visitor<'_> for V {
        type Value = Option<u32>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an integer, a numeric string, or null")
        }

        fn visit_unit<E: de::Error>(self) -> Result<Option<u32>, E> {
            Ok(None)
        }

        fn visit_none<E: de::Error>(self) -> Result<Option<u32>, E> {
            Ok(None)
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<Option<u32>, E> {
            u32::try_from(value).map(Some).map_err(E::custom)
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Option<u32>, E> {
            u32::try_from(value).map(Some).map_err(E::custom)
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Option<u32>, E> {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed.parse::<u32>().map(Some).map_err(E::custom)
        }
    }
    deserializer.deserialize_any(V)
}

fn de_opt_f64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    struct V;
    impl Visitor<'_> for V {
        type Value = Option<f64>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a number or null")
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
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed.parse::<f64>().map(Some).map_err(E::custom)
        }
    }
    deserializer.deserialize_any(V)
}

/// `GET /api/v1/schedule` document.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleEnvelope {
    /// MLBAM copyright notice.
    #[serde(default)]
    pub copyright: String,
    /// Games the API said were in the response. Not used as a row count.
    #[serde(default)]
    pub total_games: Option<u32>,
    /// One entry per date. A single-date query has one.
    #[serde(default)]
    pub dates: Vec<ScheduleDate>,
}

/// Games grouped under one schedule date.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleDate {
    /// `YYYY-MM-DD` the group is labeled with.
    #[serde(default)]
    pub date: Option<String>,
    /// Games.
    #[serde(default)]
    pub games: Vec<ScheduleGame>,
}

/// One game inside a schedule response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleGame {
    /// `gamePk`.
    pub game_pk: u64,
    /// Season, as a string.
    #[serde(default)]
    pub season: Option<String>,
    /// Official date. A postponement can move this to the makeup date.
    #[serde(default)]
    pub official_date: Option<String>,
    /// `R` for the regular season.
    #[serde(default)]
    pub game_type: Option<String>,
    /// `N`, `Y`, or `S`.
    #[serde(default)]
    pub double_header: Option<String>,
    /// Game number.
    #[serde(default)]
    pub game_number: Option<u32>,
    /// Status block.
    #[serde(default)]
    pub status: Option<ScheduleStatus>,
    /// Clubs and scores.
    pub teams: ScheduleTeams,
    /// Venue.
    #[serde(default)]
    pub venue: Option<ScheduleVenue>,
}

/// Status fields. `abstractGameState` alone does not mean the game was played.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleStatus {
    /// Coarse state. Postponed games observed as `Final` here.
    #[serde(default)]
    pub abstract_game_state: Option<String>,
    /// Detailed state (`Final`, `Postponed`, ...).
    #[serde(default)]
    pub detailed_state: Option<String>,
    /// Short code (`F`, `D`, ...).
    #[serde(default)]
    pub coded_game_state: Option<String>,
}

/// Away and home clubs.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleTeams {
    /// Away club.
    pub away: ScheduleSide,
    /// Home club.
    pub home: ScheduleSide,
}

/// One side of a scheduled game. The team object has id and name, not `sport`.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleSide {
    /// Club.
    pub team: ScheduleTeam,
    /// Runs. Missing or null when the game was not played.
    #[serde(default)]
    pub score: Option<u32>,
}

/// Team reference on a schedule game.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleTeam {
    /// Team id.
    pub id: u32,
    /// Team name.
    #[serde(default)]
    pub name: Option<String>,
}

/// Venue reference.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleVenue {
    /// Venue name.
    #[serde(default)]
    pub name: Option<String>,
}

/// `GET /api/v1/game/{gamePk}/boxscore`.
#[derive(Debug, Clone, Deserialize)]
pub struct BoxscoreEnvelope {
    /// MLBAM copyright notice.
    #[serde(default)]
    pub copyright: String,
    /// Away and home.
    pub teams: BoxscoreTeams,
}

/// Both clubs in a box score.
#[derive(Debug, Clone, Deserialize)]
pub struct BoxscoreTeams {
    /// Away club.
    pub away: BoxscoreTeam,
    /// Home club.
    pub home: BoxscoreTeam,
}

/// One club's box. `players` is keyed `ID{personId}`.
#[derive(Debug, Clone, Deserialize)]
pub struct BoxscoreTeam {
    /// Club. `sport.id` is present here even though the schedule team omits it.
    pub team: BoxTeamRef,
    /// Players who appeared, plus others the API included.
    #[serde(default)]
    pub players: HashMap<String, BoxscorePlayer>,
    /// Person ids who batted, in order.
    #[serde(default)]
    pub batters: Vec<u32>,
    /// Person ids who pitched, in order.
    #[serde(default)]
    pub pitchers: Vec<u32>,
}

/// The subset of a box-score team we read.
#[derive(Debug, Clone, Deserialize)]
pub struct BoxTeamRef {
    /// Team id.
    pub id: u32,
    /// Team name.
    pub name: String,
    /// Sport. Used to check the game's level.
    #[serde(default)]
    pub sport: Option<SportRef>,
}

/// One player entry in the box score.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxscorePlayer {
    /// Person.
    pub person: PlayerRef,
    /// Position.
    #[serde(default)]
    pub position: Option<PositionRef>,
    /// Batting-order code. A string such as `"100"` on the wire.
    #[serde(default, deserialize_with = "de_opt_u32")]
    pub batting_order: Option<u32>,
    /// Game batting and pitching lines. Season-to-date lives elsewhere and is ignored.
    #[serde(default)]
    pub stats: BoxscoreStats,
}

/// Stat blocks on a box-score player.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BoxscoreStats {
    /// Game batting line. Empty object for a pitcher who did not bat.
    #[serde(default)]
    pub batting: RawStat,
    /// Game pitching line. Empty object for a batter who did not pitch.
    #[serde(default)]
    pub pitching: RawStat,
}

/// `GET /api/v1/game/{gamePk}/playByPlay`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayByPlayEnvelope {
    /// MLBAM copyright notice.
    #[serde(default)]
    pub copyright: String,
    /// Plate appearances. Empty when the feed has no play-by-play.
    #[serde(default)]
    pub all_plays: Vec<ApiPlay>,
}

/// One plate appearance in the play-by-play feed.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiPlay {
    /// Result of the plate appearance.
    #[serde(default)]
    pub result: PlayResult,
    /// Inning and index.
    #[serde(default)]
    pub about: PlayAbout,
    /// Batter and pitcher.
    #[serde(default)]
    pub matchup: PlayMatchup,
    /// Pitches and other events (mound visits, steals) inside the plate appearance.
    #[serde(default)]
    pub play_events: Vec<PlayEvent>,
}

/// Result text and score after the play.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayResult {
    /// Display event (`Single`, `Strikeout`).
    #[serde(default)]
    pub event: Option<String>,
    /// Event code (`single`, `strikeout`).
    #[serde(default)]
    pub event_type: Option<String>,
    /// Prose description.
    #[serde(default)]
    pub description: Option<String>,
    /// RBI.
    #[serde(default)]
    pub rbi: Option<u32>,
    /// Away score after the play.
    #[serde(default)]
    pub away_score: Option<u32>,
    /// Home score after the play.
    #[serde(default)]
    pub home_score: Option<u32>,
}

/// Where the plate appearance sat in the game.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayAbout {
    /// Zero-based index.
    #[serde(default)]
    pub at_bat_index: u32,
    /// `top` or `bottom`.
    #[serde(default)]
    pub half_inning: Option<String>,
    /// Inning.
    #[serde(default)]
    pub inning: u32,
    /// False on a plate appearance that had not finished when the feed was captured.
    #[serde(default)]
    pub is_complete: Option<bool>,
    /// Scoring play.
    #[serde(default)]
    pub is_scoring_play: Option<bool>,
    /// Top of the inning. Fallback when `halfInning` is absent.
    #[serde(default)]
    pub is_top_inning: Option<bool>,
}

/// Batter, pitcher, and handedness.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayMatchup {
    /// Batter.
    #[serde(default)]
    pub batter: Option<PlayerRef>,
    /// Pitcher.
    #[serde(default)]
    pub pitcher: Option<PlayerRef>,
    /// Batter hand.
    #[serde(default)]
    pub bat_side: Option<HandCode>,
    /// Pitcher hand.
    #[serde(default)]
    pub pitch_hand: Option<HandCode>,
}

/// A code/description pair (`L` / `R`, or a pitch type).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HandCode {
    /// Short code.
    #[serde(default)]
    pub code: Option<String>,
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
}

/// One event inside a plate appearance. Only `isPitch` rows become pitches.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayEvent {
    /// True for a pitch.
    #[serde(default)]
    pub is_pitch: bool,
    /// Wire `type`, usually `pitch` or `action`.
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    /// Pitch number within the plate appearance.
    #[serde(default)]
    pub pitch_number: Option<u32>,
    /// Call and, when tracked, pitch type.
    #[serde(default)]
    pub details: Option<EventDetails>,
    /// Tracking measurements. Velocity is often absent below Triple-A.
    #[serde(default)]
    pub pitch_data: Option<PitchData>,
}

/// Call and pitch type on one pitch.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDetails {
    /// Umpire call.
    #[serde(default)]
    pub call: Option<HandCode>,
    /// Pitch type. Missing on many minor-league pitches.
    #[serde(rename = "type", default)]
    pub pitch_type: Option<HandCode>,
    /// Short description of the call.
    #[serde(default)]
    pub description: Option<String>,
    /// Ball.
    #[serde(default)]
    pub is_ball: Option<bool>,
    /// Strike.
    #[serde(default)]
    pub is_strike: Option<bool>,
    /// In play.
    #[serde(default)]
    pub is_in_play: Option<bool>,
}

/// The pitch-data fields we keep. Coordinates and break charts are not stored.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PitchData {
    /// Release speed, mph.
    #[serde(default, deserialize_with = "de_opt_f64")]
    pub start_speed: Option<f64>,
}
