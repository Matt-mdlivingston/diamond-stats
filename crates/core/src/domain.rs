//! Normalized rows. Counting stats are stored; rate stats are recomputed.
//!
//! API-provided rates are kept alongside the recomputed ones so a caller can
//! see the source value. Leaderboards recompute from summed counts rather than
//! averaging the API strings.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::group::StatGroup;
use crate::level::Level;
use crate::rates::{self, Innings};

/// One team-season batting line. Multi-team seasons are one row per team.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BattingStint {
    /// Season year.
    pub season: u16,
    /// Competitive level.
    pub level: Level,
    /// MLB person id.
    pub player_id: u32,
    /// Display name from the Stats API.
    pub player_name: String,
    /// Team the counting stats belong to.
    pub team_id: u32,
    /// Team name.
    pub team_name: String,
    /// Position abbreviation when the API sent one (`CF`, `1B`, `P`).
    pub position: Option<String>,
    /// Games.
    pub games: u32,
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
    /// Runs batted in.
    pub rbi: u32,
    /// Walks, including intentional walks.
    pub walks: u32,
    /// Intentional walks. Already included in [`Self::walks`].
    pub intentional_walks: u32,
    /// Strikeouts.
    pub strikeouts: u32,
    /// Hit by pitch.
    pub hit_by_pitch: u32,
    /// Stolen bases.
    pub stolen_bases: u32,
    /// Caught stealing.
    pub caught_stealing: u32,
    /// Sacrifice flies. Part of the OBP denominator.
    pub sac_flies: u32,
    /// Sacrifice bunts. Not part of the OBP denominator.
    pub sac_bunts: u32,
    /// Total bases, as reported by the API.
    pub total_bases: u32,
    /// Grounded into double plays.
    pub gidp: u32,
    /// Recomputed batting average.
    pub avg: Option<f64>,
    /// Recomputed on-base percentage.
    pub obp: Option<f64>,
    /// Recomputed slugging.
    pub slg: Option<f64>,
    /// Recomputed OPS.
    pub ops: Option<f64>,
    /// Average string the API sent, parsed. `None` for sentinels like `.---`.
    pub api_avg: Option<f64>,
    /// API on-base percentage.
    pub api_obp: Option<f64>,
    /// API slugging.
    pub api_slg: Option<f64>,
    /// API OPS.
    pub api_ops: Option<f64>,
}

impl BattingStint {
    /// Fill [`Self::avg`], [`Self::obp`], [`Self::slg`], and [`Self::ops`] from counts.
    pub fn recompute_rates(&mut self) {
        self.avg = rates::batting_average(self.hits, self.at_bats);
        self.obp = rates::on_base_percentage(
            self.hits,
            self.walks,
            self.hit_by_pitch,
            self.at_bats,
            self.sac_flies,
        );
        self.slg = rates::slugging(self.total_bases, self.at_bats);
        self.ops = rates::ops(self.obp, self.slg);
    }
}

/// One team-season pitching line.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PitchingStint {
    /// Season year.
    pub season: u16,
    /// Competitive level.
    pub level: Level,
    /// MLB person id.
    pub player_id: u32,
    /// Display name.
    pub player_name: String,
    /// Team the counting stats belong to.
    pub team_id: u32,
    /// Team name.
    pub team_name: String,
    /// Position abbreviation.
    pub position: Option<String>,
    /// Games played / pitched.
    pub games: u32,
    /// Games started.
    pub games_started: u32,
    /// Wins.
    pub wins: u32,
    /// Losses.
    pub losses: u32,
    /// Saves.
    pub saves: u32,
    /// Holds.
    pub holds: u32,
    /// Blown saves.
    pub blown_saves: u32,
    /// Save opportunities.
    pub save_opportunities: u32,
    /// Complete games.
    pub complete_games: u32,
    /// Shutouts.
    pub shutouts: u32,
    /// Outs recorded. Prefer this over the notation string for math.
    pub outs: u32,
    /// Baseball innings notation (`187.2`).
    pub innings_pitched: String,
    /// Hits allowed.
    pub hits: u32,
    /// Runs allowed.
    pub runs: u32,
    /// Earned runs.
    pub earned_runs: u32,
    /// Home runs allowed.
    pub home_runs: u32,
    /// Walks allowed, including intentional walks.
    pub walks: u32,
    /// Intentional walks. Already included in [`Self::walks`].
    pub intentional_walks: u32,
    /// Strikeouts.
    pub strikeouts: u32,
    /// Hit batsmen. Not included in WHIP.
    pub hit_batters: u32,
    /// Balks.
    pub balks: u32,
    /// Wild pitches.
    pub wild_pitches: u32,
    /// Batters faced.
    pub batters_faced: u32,
    /// Strikes thrown.
    pub strikes: u32,
    /// Pitches thrown.
    pub number_of_pitches: u32,
    /// Recomputed ERA.
    pub era: Option<f64>,
    /// Recomputed WHIP.
    pub whip: Option<f64>,
    /// ERA string the API sent, parsed.
    pub api_era: Option<f64>,
    /// WHIP string the API sent, parsed.
    pub api_whip: Option<f64>,
}

impl PitchingStint {
    /// Fill [`Self::era`] and [`Self::whip`] from earned runs, walks, hits, and outs.
    pub fn recompute_rates(&mut self) {
        let innings = Innings::from_outs(self.outs);
        self.era = rates::earned_run_average(self.earned_runs, innings);
        self.whip = rates::whip(self.walks, self.hits, innings);
        if self.innings_pitched.is_empty() {
            self.innings_pitched = innings.notation();
        }
    }
}

/// One game in a player's game log.
///
/// Counting stats are for that game. Rate stats are recomputed from those
/// counts. The API's AVG/ERA/WHIP fields on a game log are season-to-date
/// through that game, so they are not stored.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GameLogEntry {
    /// Season year.
    pub season: u16,
    /// Competitive level.
    pub level: Level,
    /// Hitting or pitching.
    pub group: StatGroup,
    /// MLB person id.
    pub player_id: u32,
    /// Display name.
    pub player_name: String,
    /// Stats API `gamePk`.
    pub game_pk: u64,
    /// Game date, `YYYY-MM-DD`.
    pub game_date: String,
    /// Player's team id.
    pub team_id: u32,
    /// Player's team name.
    pub team_name: String,
    /// Opponent team id.
    pub opponent_id: Option<u32>,
    /// Opponent team name.
    pub opponent_name: Option<String>,
    /// Whether the player's team was home.
    pub is_home: Option<bool>,
    /// Whether the player's team won. This is the team result, not a pitcher decision.
    pub is_win: Option<bool>,
    /// Position abbreviation.
    pub position: Option<String>,
    /// API `summary` string, for example `1-4 | 2B, 2 K, RBI`.
    pub summary: Option<String>,
    /// At-bats (batting) or batters' at-bats (pitching), when present.
    pub at_bats: u32,
    /// Plate appearances. Often absent on pitching logs.
    pub plate_appearances: u32,
    /// Runs scored (batting) or runs allowed (pitching).
    pub runs: u32,
    /// Hits or hits allowed.
    pub hits: u32,
    /// Doubles.
    pub doubles: u32,
    /// Triples.
    pub triples: u32,
    /// Home runs or home runs allowed.
    pub home_runs: u32,
    /// RBI. Zero on pitching rows.
    pub rbi: u32,
    /// Walks.
    pub walks: u32,
    /// Strikeouts.
    pub strikeouts: u32,
    /// Hit by pitch, or hit batsmen for a pitcher.
    pub hit_by_pitch: u32,
    /// Total bases.
    pub total_bases: u32,
    /// Sacrifice flies.
    pub sac_flies: u32,
    /// Outs recorded. Zero on batting rows.
    pub outs: u32,
    /// Baseball innings notation. Empty for batting rows.
    pub innings_pitched: String,
    /// Earned runs. Zero on batting rows.
    pub earned_runs: u32,
    /// Recomputed AVG for a batting log, or `None` for pitching.
    pub avg: Option<f64>,
    /// Recomputed ERA for a pitching log.
    pub era: Option<f64>,
    /// Recomputed WHIP for a pitching log.
    pub whip: Option<f64>,
}

impl GameLogEntry {
    /// Recompute the per-game rate from this row's counting stats.
    pub fn recompute_rates(&mut self) {
        match self.group {
            StatGroup::Hitting => {
                self.avg = rates::batting_average(self.hits, self.at_bats);
                self.era = None;
                self.whip = None;
            }
            StatGroup::Pitching => {
                let innings = Innings::from_outs(self.outs);
                self.era = rates::earned_run_average(self.earned_runs, innings);
                self.whip = rates::whip(self.walks, self.hits, innings);
                self.avg = None;
                if self.innings_pitched.is_empty() {
                    self.innings_pitched = innings.notation();
                }
            }
        }
    }
}

/// A club in a season, from `GET /api/v1/teams`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Team {
    /// Team id.
    pub team_id: u32,
    /// Season the roster snapshot belongs to.
    pub season: u16,
    /// Competitive level.
    pub level: Level,
    /// Full name (`Seattle Mariners`).
    pub name: String,
    /// Abbreviation (`SEA`, `AZ`).
    pub abbreviation: Option<String>,
    /// Club nickname (`Mariners`).
    pub team_name: Option<String>,
    /// Location (`Seattle`, `Phoenix`).
    pub location_name: Option<String>,
    /// League name.
    pub league_name: Option<String>,
    /// Division name.
    pub division_name: Option<String>,
    /// MLB parent organization id. Present for minor-league clubs.
    pub parent_org_id: Option<u32>,
    /// MLB parent organization name.
    pub parent_org_name: Option<String>,
}

/// Which counting or rate stat a leaderboard can sort by.
///
/// The set is closed. Request strings never become SQL identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeaderStat {
    /// Home runs.
    HomeRuns,
    /// RBI.
    Rbi,
    /// Batting average.
    Avg,
    /// On-base percentage.
    Obp,
    /// Slugging.
    Slg,
    /// OPS.
    Ops,
    /// Hits.
    Hits,
    /// Stolen bases.
    StolenBases,
    /// Runs.
    Runs,
    /// Walks.
    Walks,
    /// Strikeouts.
    Strikeouts,
    /// Doubles.
    Doubles,
    /// Triples.
    Triples,
    /// Plate appearances.
    PlateAppearances,
    /// At-bats.
    AtBats,
    /// ERA. Lower is better.
    Era,
    /// WHIP. Lower is better.
    Whip,
    /// Wins.
    Wins,
    /// Losses.
    Losses,
    /// Saves.
    Saves,
    /// Innings pitched, ordered by outs.
    InningsPitched,
    /// Earned runs.
    EarnedRuns,
    /// Hits allowed.
    HitsAllowed,
    /// Home runs allowed.
    HomeRunsAllowed,
    /// Pitching strikeouts. Distinct from batting strikeouts so the group is unambiguous.
    PitchingStrikeouts,
    /// Walks allowed.
    PitchingWalks,
    /// Runs allowed.
    RunsAllowed,
}

impl LeaderStat {
    /// Wire name (`hr`, `avg`, `era`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            LeaderStat::HomeRuns => "hr",
            LeaderStat::Rbi => "rbi",
            LeaderStat::Avg => "avg",
            LeaderStat::Obp => "obp",
            LeaderStat::Slg => "slg",
            LeaderStat::Ops => "ops",
            LeaderStat::Hits => "h",
            LeaderStat::StolenBases => "sb",
            LeaderStat::Runs => "r",
            LeaderStat::Walks => "bb",
            LeaderStat::Strikeouts => "so",
            LeaderStat::Doubles => "2b",
            LeaderStat::Triples => "3b",
            LeaderStat::PlateAppearances => "pa",
            LeaderStat::AtBats => "ab",
            LeaderStat::Era => "era",
            LeaderStat::Whip => "whip",
            LeaderStat::Wins => "w",
            LeaderStat::Losses => "l",
            LeaderStat::Saves => "sv",
            LeaderStat::InningsPitched => "ip",
            LeaderStat::EarnedRuns => "er",
            LeaderStat::HitsAllowed => "h",
            LeaderStat::HomeRunsAllowed => "hr",
            LeaderStat::PitchingStrikeouts => "so",
            LeaderStat::PitchingWalks => "bb",
            LeaderStat::RunsAllowed => "r",
        }
    }

    /// Whether this stat is a rate that needs a sample-size floor to mean anything.
    pub fn is_rate(self) -> bool {
        matches!(
            self,
            LeaderStat::Avg
                | LeaderStat::Obp
                | LeaderStat::Slg
                | LeaderStat::Ops
                | LeaderStat::Era
                | LeaderStat::Whip
        )
    }

    /// ERA and WHIP sort ascending.
    pub fn lower_is_better(self) -> bool {
        matches!(self, LeaderStat::Era | LeaderStat::Whip)
    }

    /// Stat group this leaderboard reads.
    pub fn group(self) -> StatGroup {
        match self {
            LeaderStat::HomeRuns
            | LeaderStat::Rbi
            | LeaderStat::Avg
            | LeaderStat::Obp
            | LeaderStat::Slg
            | LeaderStat::Ops
            | LeaderStat::Hits
            | LeaderStat::StolenBases
            | LeaderStat::Runs
            | LeaderStat::Walks
            | LeaderStat::Strikeouts
            | LeaderStat::Doubles
            | LeaderStat::Triples
            | LeaderStat::PlateAppearances
            | LeaderStat::AtBats => StatGroup::Hitting,
            LeaderStat::Era
            | LeaderStat::Whip
            | LeaderStat::Wins
            | LeaderStat::Losses
            | LeaderStat::Saves
            | LeaderStat::InningsPitched
            | LeaderStat::EarnedRuns
            | LeaderStat::HitsAllowed
            | LeaderStat::HomeRunsAllowed
            | LeaderStat::PitchingStrikeouts
            | LeaderStat::PitchingWalks
            | LeaderStat::RunsAllowed => StatGroup::Pitching,
        }
    }

    /// Parse a leaderboard stat for a group.
    ///
    /// `h`, `hr`, `bb`, and `so` exist on both sides; `group` decides which.
    pub fn parse(group: StatGroup, raw: &str) -> Option<Self> {
        let token = raw.trim().to_ascii_lowercase();
        let stat = match (group, token.as_str()) {
            (StatGroup::Hitting, "hr" | "homeruns" | "home_runs") => LeaderStat::HomeRuns,
            (StatGroup::Hitting, "rbi") => LeaderStat::Rbi,
            (StatGroup::Hitting, "avg" | "ba") => LeaderStat::Avg,
            (StatGroup::Hitting, "obp") => LeaderStat::Obp,
            (StatGroup::Hitting, "slg") => LeaderStat::Slg,
            (StatGroup::Hitting, "ops") => LeaderStat::Ops,
            (StatGroup::Hitting, "h" | "hits") => LeaderStat::Hits,
            (StatGroup::Hitting, "sb" | "stolen_bases") => LeaderStat::StolenBases,
            (StatGroup::Hitting, "r" | "runs") => LeaderStat::Runs,
            (StatGroup::Hitting, "bb" | "walks") => LeaderStat::Walks,
            (StatGroup::Hitting, "so" | "k" | "strikeouts") => LeaderStat::Strikeouts,
            (StatGroup::Hitting, "2b" | "doubles") => LeaderStat::Doubles,
            (StatGroup::Hitting, "3b" | "triples") => LeaderStat::Triples,
            (StatGroup::Hitting, "pa" | "plate_appearances") => LeaderStat::PlateAppearances,
            (StatGroup::Hitting, "ab" | "at_bats") => LeaderStat::AtBats,
            (StatGroup::Pitching, "era") => LeaderStat::Era,
            (StatGroup::Pitching, "whip") => LeaderStat::Whip,
            (StatGroup::Pitching, "w" | "wins") => LeaderStat::Wins,
            (StatGroup::Pitching, "l" | "losses") => LeaderStat::Losses,
            (StatGroup::Pitching, "sv" | "saves") => LeaderStat::Saves,
            (StatGroup::Pitching, "ip" | "innings" | "innings_pitched") => {
                LeaderStat::InningsPitched
            }
            (StatGroup::Pitching, "er" | "earned_runs") => LeaderStat::EarnedRuns,
            (StatGroup::Pitching, "so" | "k" | "strikeouts") => LeaderStat::PitchingStrikeouts,
            (StatGroup::Pitching, "bb" | "walks") => LeaderStat::PitchingWalks,
            (StatGroup::Pitching, "h" | "hits") => LeaderStat::HitsAllowed,
            (StatGroup::Pitching, "hr" | "home_runs") => LeaderStat::HomeRunsAllowed,
            (StatGroup::Pitching, "r" | "runs") => LeaderStat::RunsAllowed,
            _ => return None,
        };
        Some(stat)
    }
}

/// Short attribution carried on API responses.
pub const ATTRIBUTION_NOTE: &str = "Statistics from the MLB Stats API, copyright MLB Advanced Media, L.P. Individual, non-commercial, non-bulk use only.";

/// Terms URL printed on every Stats API response.
pub const MLBAM_TERMS_URL: &str = "http://gdx.mlb.com/components/copyright.txt";
