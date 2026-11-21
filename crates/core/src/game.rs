//! Games, box scores, and play-by-play rows.
//!
//! Counting stats are stored. Batting average, ERA, and WHIP on a box line are
//! recomputed from that game's counts in the query layer, with the same
//! functions as a season line. Pitch type and velocity stay optional: several
//! minor-league feeds send the pitch sequence and the umpire's call without them.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::CoreError;
use crate::level::Level;

/// One game on a schedule date.
///
/// `schedule_date` is the date that was requested. `official_date` is the
/// Stats API `officialDate`. They differ when a game was postponed and the
/// API rewrote `officialDate` to the makeup date while still returning the
/// game on the original date. Observed on 2025-06-21 for High-A game 789134.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScheduledGame {
    /// Stats API `gamePk`.
    pub game_pk: u64,
    /// Season year from the schedule.
    pub season: u16,
    /// Competitive level.
    pub level: Level,
    /// Date the schedule was requested for, `YYYY-MM-DD`.
    pub schedule_date: String,
    /// Stats API `officialDate`. May be a makeup date.
    pub official_date: String,
    /// Game type, usually `R`.
    pub game_type: String,
    /// `status.detailedState` (`Final`, `Postponed`, ...).
    pub detailed_state: String,
    /// `status.abstractGameState`. Not a reliable "this game was played" flag.
    pub abstract_state: String,
    /// `status.codedGameState` (`F` final, `D` postponed, ...).
    pub coded_state: String,
    /// Away team id.
    pub away_team_id: u32,
    /// Away team name.
    pub away_team_name: String,
    /// Away runs. Absent when the game was not played.
    pub away_score: Option<u32>,
    /// Home team id.
    pub home_team_id: u32,
    /// Home team name.
    pub home_team_name: String,
    /// Home runs. Absent when the game was not played.
    pub home_score: Option<u32>,
    /// Venue name.
    pub venue_name: Option<String>,
    /// `N` single game, `Y` doubleheader, `S` split.
    pub double_header: String,
    /// Game number on the date. `1` unless this is the second game of a doubleheader.
    pub game_number: u32,
}

impl ScheduledGame {
    /// Whether a box score and play-by-play should be fetched and cached.
    ///
    /// A postponed game can still say `abstractGameState = Final`. The coded
    /// state `F` or detailed state `Final` is the signal that a line exists.
    /// Game 789134 (2025-06-21 High-A) is `abstractGameState` Final, coded `D`,
    /// detailed `Postponed`, with no score.
    pub fn boxscore_ready(&self) -> bool {
        self.coded_state == "F" || self.detailed_state == "Final"
    }
}

/// One player's batting line in a game box score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BoxBattingLine {
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// `gamePk`.
    pub game_pk: u64,
    /// Schedule date the game was stored under.
    pub schedule_date: String,
    /// Stats API `officialDate`.
    pub official_date: String,
    /// Team id.
    pub team_id: u32,
    /// Team name.
    pub team_name: String,
    /// Whether this club was home.
    pub is_home: bool,
    /// Player id.
    pub player_id: u32,
    /// Player name.
    pub player_name: String,
    /// Position abbreviation.
    pub position: Option<String>,
    /// Stats API batting-order code (`100` is the first slot, `200` the second).
    pub batting_order: Option<u32>,
    /// At-bats.
    pub at_bats: u32,
    /// Plate appearances.
    pub plate_appearances: u32,
    /// Runs.
    pub runs: u32,
    /// Hits.
    pub hits: u32,
    /// Doubles.
    pub doubles: u32,
    /// Triples.
    pub triples: u32,
    /// Home runs.
    pub home_runs: u32,
    /// RBI.
    pub rbi: u32,
    /// Walks.
    pub walks: u32,
    /// Strikeouts.
    pub strikeouts: u32,
    /// Hit by pitch.
    pub hit_by_pitch: u32,
    /// Sacrifice flies.
    pub sac_flies: u32,
    /// Total bases.
    pub total_bases: u32,
    /// Stolen bases.
    pub stolen_bases: u32,
    /// API summary, when the box score sent one.
    pub summary: Option<String>,
    /// Recomputed batting average for this game.
    pub avg: Option<f64>,
}

/// One pitcher's line in a game box score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BoxPitchingLine {
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// `gamePk`.
    pub game_pk: u64,
    /// Schedule date the game was stored under.
    pub schedule_date: String,
    /// Stats API `officialDate`.
    pub official_date: String,
    /// Team id.
    pub team_id: u32,
    /// Team name.
    pub team_name: String,
    /// Whether this club was home.
    pub is_home: bool,
    /// Player id.
    pub player_id: u32,
    /// Player name.
    pub player_name: String,
    /// Position abbreviation.
    pub position: Option<String>,
    /// Outs recorded.
    pub outs: u32,
    /// Baseball innings notation.
    pub innings_pitched: String,
    /// Hits allowed.
    pub hits: u32,
    /// Runs allowed.
    pub runs: u32,
    /// Earned runs.
    pub earned_runs: u32,
    /// Home runs allowed.
    pub home_runs: u32,
    /// Walks.
    pub walks: u32,
    /// Strikeouts.
    pub strikeouts: u32,
    /// Hit batsmen.
    pub hit_batters: u32,
    /// Batters faced.
    pub batters_faced: u32,
    /// Pitches thrown.
    pub number_of_pitches: u32,
    /// Win flag for this game (`1` or `0`).
    pub wins: u32,
    /// Loss flag for this game.
    pub losses: u32,
    /// Save flag for this game.
    pub saves: u32,
    /// API summary, when present.
    pub summary: Option<String>,
    /// Recomputed ERA for this outing.
    pub era: Option<f64>,
    /// Recomputed WHIP for this outing.
    pub whip: Option<f64>,
}

/// One plate appearance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlateAppearance {
    /// `gamePk`.
    pub game_pk: u64,
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Schedule date.
    pub schedule_date: String,
    /// Official date.
    pub official_date: String,
    /// Zero-based index of the plate appearance in the game.
    pub at_bat_index: u32,
    /// Inning number.
    pub inning: u32,
    /// `top` or `bottom`.
    pub half_inning: String,
    /// Batter id.
    pub batter_id: u32,
    /// Batter name.
    pub batter_name: String,
    /// Pitcher id.
    pub pitcher_id: u32,
    /// Pitcher name.
    pub pitcher_name: String,
    /// Batter hand (`L`, `R`, `S`).
    pub bat_side: Option<String>,
    /// Pitcher hand.
    pub pitch_hand: Option<String>,
    /// Result label (`Single`, `Strikeout`, ...).
    pub event: Option<String>,
    /// Result code (`single`, `strikeout`, `field_out`, ...).
    pub event_type: Option<String>,
    /// Sentence the API wrote for the plate appearance.
    pub description: Option<String>,
    /// RBI on the play.
    pub rbi: u32,
    /// Whether the play scored a run.
    pub is_scoring_play: bool,
    /// Away score after the play.
    pub away_score: u32,
    /// Home score after the play.
    pub home_score: u32,
}

/// One pitch inside a plate appearance.
///
/// `pitch_type_code` and `start_speed` are `None` when the feed did not send
/// them. That is common at Double-A, High-A, and Rookie, and it also happens
/// at Single-A. The call (`B`, `C`, `S`, `F`, `X`, ...) is usually still there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Pitch {
    /// `gamePk`.
    pub game_pk: u64,
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Schedule date.
    pub schedule_date: String,
    /// Official date.
    pub official_date: String,
    /// Plate appearance index.
    pub at_bat_index: u32,
    /// Pitch number inside the plate appearance, starting at 1.
    pub pitch_number: u32,
    /// Umpire call code.
    pub call_code: Option<String>,
    /// Umpire call description.
    pub call_description: Option<String>,
    /// Pitch-type code (`FF`, `SI`, ...), when the feed sent one.
    pub pitch_type_code: Option<String>,
    /// Pitch-type name, when the feed sent one.
    pub pitch_type: Option<String>,
    /// Release speed in mph, when the feed sent one.
    pub start_speed: Option<f64>,
    /// The call was a ball.
    pub is_ball: bool,
    /// The call was a strike.
    pub is_strike: bool,
    /// The ball was put in play.
    pub is_in_play: bool,
}

/// How much pitch-level detail a game's feed actually contained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PitchTracking {
    /// Plate appearances stored.
    pub plate_appearances: u32,
    /// Pitch events stored.
    pub pitches: u32,
    /// Pitches that included `startSpeed`.
    pub pitches_with_velocity: u32,
    /// Pitches that included a pitch type.
    pub pitches_with_pitch_type: u32,
}

impl PitchTracking {
    /// Summarize plays and pitches already normalized. Nothing is inferred.
    pub fn from_rows(plays: &[PlateAppearance], pitches: &[Pitch]) -> Self {
        let pitches_with_velocity = pitches
            .iter()
            .filter(|pitch| pitch.start_speed.is_some())
            .count();
        let pitches_with_pitch_type = pitches
            .iter()
            .filter(|pitch| pitch.pitch_type_code.is_some())
            .count();
        Self {
            plate_appearances: u32::try_from(plays.len()).unwrap_or(u32::MAX),
            pitches: u32::try_from(pitches.len()).unwrap_or(u32::MAX),
            pitches_with_velocity: u32::try_from(pitches_with_velocity).unwrap_or(u32::MAX),
            pitches_with_pitch_type: u32::try_from(pitches_with_pitch_type).unwrap_or(u32::MAX),
        }
    }
}

/// A sentence a response can show next to [`PitchTracking`].
///
/// The wording follows the counts for this game. It does not assume a level
/// never has pitch tracking: Single-A games on the same date disagree.
pub fn pitch_tracking_note(tracking: &PitchTracking) -> &'static str {
    if tracking.plate_appearances == 0 {
        "The play-by-play feed had no plate appearances."
    } else if tracking.pitches == 0 {
        "Plate appearances are present. The feed had no pitch events."
    } else if tracking.pitches_with_velocity == 0 && tracking.pitches_with_pitch_type == 0 {
        "Pitch sequence and umpire calls are present. The Stats API did not send pitch type or velocity for these pitches."
    } else if tracking.pitches_with_velocity == tracking.pitches
        && tracking.pitches_with_pitch_type == tracking.pitches
    {
        "Pitch type and velocity were present on every pitch in this feed."
    } else {
        "Pitch type and velocity were present on only some pitches in this feed."
    }
}

/// `YYYY-MM-DD` with a real month and day. Used before a date becomes a path.
pub fn parse_iso_date(raw: &str) -> Result<String, CoreError> {
    let bytes = raw.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(CoreError::Message(format!(
            "date must be YYYY-MM-DD, got '{raw}'"
        )));
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return Err(CoreError::Message(format!(
            "date must be YYYY-MM-DD, got '{raw}'"
        )));
    }
    let month: u32 = raw[5..7].parse().unwrap_or(0);
    let day: u32 = raw[8..10].parse().unwrap_or(0);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(CoreError::Message(format!(
            "date '{raw}' is not a calendar day"
        )));
    }
    Ok(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postponed_abstract_final_is_not_boxscore_ready() {
        let mut game = sample();
        game.abstract_state = "Final".into();
        game.coded_state = "D".into();
        game.detailed_state = "Postponed".into();
        assert!(!game.boxscore_ready());
        game.coded_state = "F".into();
        game.detailed_state = "Final".into();
        assert!(game.boxscore_ready());
    }

    #[test]
    fn pitch_note_follows_the_counts() {
        let plays = vec![sample_pa()];
        let tracked = PitchTracking::from_rows(&plays, &[sample_pitch(Some(94.0), Some("FF"))]);
        assert!(pitch_tracking_note(&tracked).contains("every pitch"));
        let bare = PitchTracking::from_rows(&plays, &[sample_pitch(None, None)]);
        assert!(pitch_tracking_note(&bare).contains("did not send pitch type"));
        let empty = PitchTracking::from_rows(&[], &[]);
        assert!(pitch_tracking_note(&empty).contains("no plate appearances"));
    }

    #[test]
    fn iso_dates_reject_path_payloads() {
        assert_eq!(parse_iso_date("2025-06-21").unwrap(), "2025-06-21");
        assert!(parse_iso_date("2025-06-21;drop").is_err());
        assert!(parse_iso_date("2025-13-01").is_err());
        assert!(parse_iso_date("../2025-06-21").is_err());
    }

    fn sample() -> ScheduledGame {
        ScheduledGame {
            game_pk: 1,
            season: 2025,
            level: Level::HighA,
            schedule_date: "2025-06-21".into(),
            official_date: "2025-06-21".into(),
            game_type: "R".into(),
            detailed_state: "Final".into(),
            abstract_state: "Final".into(),
            coded_state: "F".into(),
            away_team_id: 1,
            away_team_name: "Away".into(),
            away_score: Some(1),
            home_team_id: 2,
            home_team_name: "Home".into(),
            home_score: Some(2),
            venue_name: None,
            double_header: "N".into(),
            game_number: 1,
        }
    }

    fn sample_pa() -> PlateAppearance {
        PlateAppearance {
            game_pk: 1,
            season: 2025,
            level: Level::Mlb,
            schedule_date: "2025-06-21".into(),
            official_date: "2025-06-21".into(),
            at_bat_index: 0,
            inning: 1,
            half_inning: "top".into(),
            batter_id: 1,
            batter_name: "Batter".into(),
            pitcher_id: 2,
            pitcher_name: "Pitcher".into(),
            bat_side: None,
            pitch_hand: None,
            event: None,
            event_type: None,
            description: None,
            rbi: 0,
            is_scoring_play: false,
            away_score: 0,
            home_score: 0,
        }
    }

    fn sample_pitch(speed: Option<f64>, kind: Option<&str>) -> Pitch {
        Pitch {
            game_pk: 1,
            season: 2025,
            level: Level::Mlb,
            schedule_date: "2025-06-21".into(),
            official_date: "2025-06-21".into(),
            at_bat_index: 0,
            pitch_number: 1,
            call_code: Some("C".into()),
            call_description: None,
            pitch_type_code: kind.map(str::to_string),
            pitch_type: None,
            start_speed: speed,
            is_ball: false,
            is_strike: true,
            is_in_play: false,
        }
    }
}
