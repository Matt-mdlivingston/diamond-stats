//! DuckDB reads. One in-memory connection per call; the process does not keep
//! a writer lock on the files.

use std::path::{Path, PathBuf};

use diamond_stats_core::paths::{self, dataset_glob, gamelog_file, partition_file};
use diamond_stats_core::{
    BattingStint, GameLogEntry, Innings, LeaderStat, Level, PitchingStint, StatGroup, Team, rates,
};
use duckdb::{Connection, Row, params};

use crate::error::QueryError;

/// Handle on a data directory full of Parquet partitions.
#[derive(Debug, Clone)]
pub struct StatsStore {
    data_dir: PathBuf,
}

/// A leaderboard request. `stat` selects both the dataset and the sort.
#[derive(Debug, Clone, Copy)]
pub struct LeaderRequest {
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Closed set of stats. This is not interpolated into SQL.
    pub stat: LeaderStat,
    /// Maximum rows after sorting. Must be 1..=100.
    pub limit: u32,
    /// Minimum summed plate appearances. Ignored for pitching stats.
    pub min_pa: u32,
    /// Minimum summed outs. Ignored for batting stats.
    pub min_outs: u32,
}

/// One leaderboard row after aggregation across team stints.
#[derive(Debug, Clone)]
pub struct LeaderRow {
    /// 1-based rank after the sort. Ties break by player id.
    pub rank: u32,
    /// MLB person id.
    pub player_id: u32,
    /// Display name.
    pub player_name: String,
    /// Teams that contributed, joined with ` | `.
    pub team_names: String,
    /// Sort value. Innings pitched is true innings (`outs / 3`), not baseball notation.
    pub value: f64,
    /// Summed plate appearances. Zero for pitching rows.
    pub plate_appearances: u32,
    /// Summed outs. Zero for batting rows.
    pub outs: u32,
    /// Baseball innings notation when this is a pitching row.
    pub innings_pitched: Option<String>,
}

/// Every stint on file for one player.
#[derive(Debug, Clone)]
pub struct PlayerSeasons {
    /// MLB person id.
    pub player_id: u32,
    /// Name from the first row, if any.
    pub player_name: Option<String>,
    /// Batting stints across levels.
    pub batting: Vec<BattingStint>,
    /// Pitching stints across levels.
    pub pitching: Vec<PitchingStint>,
}

impl PlayerSeasons {
    /// True when both sides are empty.
    pub fn is_empty(&self) -> bool {
        self.batting.is_empty() && self.pitching.is_empty()
    }
}

/// Batting and pitching stints for one club.
#[derive(Debug, Clone)]
pub struct Roster {
    /// Team id.
    pub team_id: u32,
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Name from the teams table, when that partition exists.
    pub team_name: Option<String>,
    /// Batting stints labeled with this team.
    pub batting: Vec<BattingStint>,
    /// Pitching stints labeled with this team.
    pub pitching: Vec<PitchingStint>,
}

/// Result of reading a lazily materialized game log.
#[derive(Debug, Clone)]
pub enum GameLogLoad {
    /// No file has been written for this player-season-level-group.
    NotMaterialized,
    /// The file exists. It may contain zero games.
    Games(Vec<GameLogEntry>),
}

impl StatsStore {
    /// Read partitions under `data_dir`.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// Data directory this store reads.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Season leaders for one stat. Counting stats are summed across stints
    /// before rates are recomputed, so a traded hitter is one row.
    pub fn leaders(&self, request: LeaderRequest) -> Result<Vec<LeaderRow>, QueryError> {
        if request.limit == 0 || request.limit > 100 {
            return Err(QueryError::Message(
                "limit must be between 1 and 100".into(),
            ));
        }
        match request.stat.group() {
            StatGroup::Hitting => self.batting_leaders(request),
            StatGroup::Pitching => self.pitching_leaders(request),
        }
    }

    /// Stints for one player at every ingested level.
    pub fn player_seasons(&self, player_id: u32) -> Result<PlayerSeasons, QueryError> {
        let batting_present = dataset_has_files(&self.data_dir, "batting");
        let pitching_present = dataset_has_files(&self.data_dir, "pitching");
        if !batting_present && !pitching_present {
            return Err(QueryError::DatasetMissing {
                dataset: "batting/pitching",
            });
        }
        let conn = connect()?;
        let batting = if batting_present {
            query_batting(
                &conn,
                &self.data_dir,
                "WHERE player_id = ? ORDER BY season, level, team_id",
                params![player_id as i32],
            )?
        } else {
            Vec::new()
        };
        let pitching = if pitching_present {
            query_pitching(
                &conn,
                &self.data_dir,
                "WHERE player_id = ? ORDER BY season, level, team_id",
                params![player_id as i32],
            )?
        } else {
            Vec::new()
        };
        let player_name = batting
            .first()
            .map(|row| row.player_name.clone())
            .or_else(|| pitching.first().map(|row| row.player_name.clone()));
        Ok(PlayerSeasons {
            player_id,
            player_name,
            batting,
            pitching,
        })
    }

    /// Stints whose `team_id` is the requested club.
    pub fn team_roster(
        &self,
        team_id: u32,
        season: u16,
        level: Level,
    ) -> Result<Roster, QueryError> {
        let batting_file = partition_file(&self.data_dir, "batting", season, level);
        let pitching_file = partition_file(&self.data_dir, "pitching", season, level);
        if !batting_file.is_file() && !pitching_file.is_file() {
            return Err(QueryError::DataNotLoaded {
                dataset: "batting/pitching",
                season,
                level,
            });
        }
        let conn = connect()?;
        let filter = "WHERE season = ? AND level = ? AND team_id = ? ORDER BY plate_appearances DESC, player_id";
        let batting = if batting_file.is_file() {
            query_batting(
                &conn,
                &self.data_dir,
                filter,
                params![i32::from(season), level.slug(), team_id as i32],
            )?
        } else {
            Vec::new()
        };
        let pitching_filter =
            "WHERE season = ? AND level = ? AND team_id = ? ORDER BY outs DESC, player_id";
        let pitching = if pitching_file.is_file() {
            query_pitching(
                &conn,
                &self.data_dir,
                pitching_filter,
                params![i32::from(season), level.slug(), team_id as i32],
            )?
        } else {
            Vec::new()
        };
        let team_name = self.teams(season, level).ok().and_then(|teams| {
            teams
                .into_iter()
                .find(|team| team.team_id == team_id)
                .map(|team| team.name)
        });
        Ok(Roster {
            team_id,
            season,
            level,
            team_name,
            batting,
            pitching,
        })
    }

    /// Clubs ingested for a season and level.
    pub fn teams(&self, season: u16, level: Level) -> Result<Vec<Team>, QueryError> {
        let path = partition_file(&self.data_dir, "teams", season, level);
        if !path.is_file() {
            return Err(QueryError::DataNotLoaded {
                dataset: "teams",
                season,
                level,
            });
        }
        let conn = connect()?;
        let sql = format!(
            "SELECT team_id, season, level, name, abbreviation, team_name, location_name, \
             league_name, division_name, parent_org_id, parent_org_name \
             FROM {} WHERE season = ? AND level = ? ORDER BY name",
            read_fn(&path)?
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![i32::from(season), level.slug()])?;
        let mut teams = Vec::new();
        while let Some(row) = rows.next()? {
            teams.push(team_from_row(row)?);
        }
        Ok(teams)
    }

    /// A player's game log, if it has been materialized.
    pub fn game_log(
        &self,
        player_id: u32,
        season: u16,
        level: Level,
        group: StatGroup,
    ) -> Result<GameLogLoad, QueryError> {
        let path = gamelog_file(&self.data_dir, group, season, level, player_id);
        if !path.is_file() {
            return Ok(GameLogLoad::NotMaterialized);
        }
        let conn = connect()?;
        let games = match group {
            StatGroup::Hitting => read_batting_gamelog(&conn, &path)?,
            StatGroup::Pitching => read_pitching_gamelog(&conn, &path)?,
        };
        Ok(GameLogLoad::Games(games))
    }

    fn batting_leaders(&self, request: LeaderRequest) -> Result<Vec<LeaderRow>, QueryError> {
        let path = require_partition(&self.data_dir, "batting", request.season, request.level)?;
        let conn = connect()?;
        let sql = format!(
            "SELECT player_id, min(player_name), \
                    string_agg(DISTINCT team_name, ' | '), \
                    SUM(plate_appearances), SUM(at_bats), SUM(hits), SUM(walks), \
                    SUM(hit_by_pitch), SUM(sac_flies), SUM(total_bases), SUM(home_runs), \
                    SUM(rbi), SUM(runs), SUM(stolen_bases), SUM(strikeouts), SUM(doubles), \
                    SUM(triples) \
             FROM {} WHERE season = ? AND level = ? \
             GROUP BY player_id \
             HAVING SUM(plate_appearances) >= ?",
            read_fn(&path)?
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![
            i32::from(request.season),
            request.level.slug(),
            i64::from(request.min_pa)
        ])?;
        let mut leaders = Vec::new();
        while let Some(row) = rows.next()? {
            let plate_appearances = get_u32(row, 3)?;
            let at_bats = get_u32(row, 4)?;
            let hits = get_u32(row, 5)?;
            let walks = get_u32(row, 6)?;
            let hit_by_pitch = get_u32(row, 7)?;
            let sac_flies = get_u32(row, 8)?;
            let total_bases = get_u32(row, 9)?;
            let home_runs = get_u32(row, 10)?;
            let rbi = get_u32(row, 11)?;
            let runs = get_u32(row, 12)?;
            let stolen_bases = get_u32(row, 13)?;
            let strikeouts = get_u32(row, 14)?;
            let doubles = get_u32(row, 15)?;
            let triples = get_u32(row, 16)?;
            let value = batting_value(
                request.stat,
                plate_appearances,
                at_bats,
                hits,
                walks,
                hit_by_pitch,
                sac_flies,
                total_bases,
                home_runs,
                rbi,
                runs,
                stolen_bases,
                strikeouts,
                doubles,
                triples,
            );
            let Some(value) = value else {
                continue;
            };
            leaders.push(LeaderRow {
                rank: 0,
                player_id: get_u32(row, 0)?,
                player_name: row.get(1)?,
                team_names: row.get(2)?,
                value,
                plate_appearances,
                outs: 0,
                innings_pitched: None,
            });
        }
        finish_leaders(&mut leaders, request);
        Ok(leaders)
    }

    fn pitching_leaders(&self, request: LeaderRequest) -> Result<Vec<LeaderRow>, QueryError> {
        let path = require_partition(&self.data_dir, "pitching", request.season, request.level)?;
        let conn = connect()?;
        let sql = format!(
            "SELECT player_id, min(player_name), \
                    string_agg(DISTINCT team_name, ' | '), \
                    SUM(outs), SUM(earned_runs), SUM(hits), SUM(walks), SUM(strikeouts), \
                    SUM(wins), SUM(losses), SUM(saves), SUM(home_runs), SUM(runs) \
             FROM {} WHERE season = ? AND level = ? \
             GROUP BY player_id \
             HAVING SUM(outs) >= ?",
            read_fn(&path)?
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![
            i32::from(request.season),
            request.level.slug(),
            i64::from(request.min_outs)
        ])?;
        let mut leaders = Vec::new();
        while let Some(row) = rows.next()? {
            let outs = get_u32(row, 3)?;
            let earned_runs = get_u32(row, 4)?;
            let hits = get_u32(row, 5)?;
            let walks = get_u32(row, 6)?;
            let strikeouts = get_u32(row, 7)?;
            let wins = get_u32(row, 8)?;
            let losses = get_u32(row, 9)?;
            let saves = get_u32(row, 10)?;
            let home_runs = get_u32(row, 11)?;
            let runs = get_u32(row, 12)?;
            let value = pitching_value(
                request.stat,
                outs,
                earned_runs,
                hits,
                walks,
                strikeouts,
                wins,
                losses,
                saves,
                home_runs,
                runs,
            );
            let Some(value) = value else {
                continue;
            };
            leaders.push(LeaderRow {
                rank: 0,
                player_id: get_u32(row, 0)?,
                player_name: row.get(1)?,
                team_names: row.get(2)?,
                value,
                plate_appearances: 0,
                outs,
                innings_pitched: Some(Innings::from_outs(outs).notation()),
            });
        }
        finish_leaders(&mut leaders, request);
        Ok(leaders)
    }
}

fn finish_leaders(leaders: &mut Vec<LeaderRow>, request: LeaderRequest) {
    let lower = request.stat.lower_is_better();
    leaders.sort_by(|left, right| {
        let value = if lower {
            left.value.partial_cmp(&right.value)
        } else {
            right.value.partial_cmp(&left.value)
        }
        .unwrap_or(std::cmp::Ordering::Equal);
        value.then(left.player_id.cmp(&right.player_id))
    });
    leaders.truncate(request.limit as usize);
    for (index, row) in leaders.iter_mut().enumerate() {
        row.rank = u32::try_from(index + 1).unwrap_or(u32::MAX);
    }
}

#[allow(clippy::too_many_arguments)]
fn batting_value(
    stat: LeaderStat,
    plate_appearances: u32,
    at_bats: u32,
    hits: u32,
    walks: u32,
    hit_by_pitch: u32,
    sac_flies: u32,
    total_bases: u32,
    home_runs: u32,
    rbi: u32,
    runs: u32,
    stolen_bases: u32,
    strikeouts: u32,
    doubles: u32,
    triples: u32,
) -> Option<f64> {
    let obp = rates::on_base_percentage(hits, walks, hit_by_pitch, at_bats, sac_flies);
    let slg = rates::slugging(total_bases, at_bats);
    match stat {
        LeaderStat::HomeRuns => Some(f64::from(home_runs)),
        LeaderStat::Rbi => Some(f64::from(rbi)),
        LeaderStat::Avg => rates::batting_average(hits, at_bats),
        LeaderStat::Obp => obp,
        LeaderStat::Slg => slg,
        LeaderStat::Ops => rates::ops(obp, slg),
        LeaderStat::Hits => Some(f64::from(hits)),
        LeaderStat::StolenBases => Some(f64::from(stolen_bases)),
        LeaderStat::Runs => Some(f64::from(runs)),
        LeaderStat::Walks => Some(f64::from(walks)),
        LeaderStat::Strikeouts => Some(f64::from(strikeouts)),
        LeaderStat::Doubles => Some(f64::from(doubles)),
        LeaderStat::Triples => Some(f64::from(triples)),
        LeaderStat::PlateAppearances => Some(f64::from(plate_appearances)),
        LeaderStat::AtBats => Some(f64::from(at_bats)),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn pitching_value(
    stat: LeaderStat,
    outs: u32,
    earned_runs: u32,
    hits: u32,
    walks: u32,
    strikeouts: u32,
    wins: u32,
    losses: u32,
    saves: u32,
    home_runs: u32,
    runs: u32,
) -> Option<f64> {
    let innings = Innings::from_outs(outs);
    match stat {
        LeaderStat::Era => rates::earned_run_average(earned_runs, innings),
        LeaderStat::Whip => rates::whip(walks, hits, innings),
        LeaderStat::Wins => Some(f64::from(wins)),
        LeaderStat::Losses => Some(f64::from(losses)),
        LeaderStat::Saves => Some(f64::from(saves)),
        LeaderStat::InningsPitched => (outs > 0).then_some(innings.as_decimal()),
        LeaderStat::EarnedRuns => Some(f64::from(earned_runs)),
        LeaderStat::PitchingStrikeouts => Some(f64::from(strikeouts)),
        LeaderStat::PitchingWalks => Some(f64::from(walks)),
        LeaderStat::HitsAllowed => Some(f64::from(hits)),
        LeaderStat::HomeRunsAllowed => Some(f64::from(home_runs)),
        LeaderStat::RunsAllowed => Some(f64::from(runs)),
        _ => None,
    }
}

fn query_batting(
    conn: &Connection,
    data_dir: &Path,
    filter: &str,
    values: impl duckdb::Params,
) -> Result<Vec<BattingStint>, QueryError> {
    let sql = format!(
        "SELECT season, level, player_id, player_name, team_id, team_name, position, \
                games, at_bats, plate_appearances, runs, hits, doubles, triples, home_runs, rbi, \
                walks, intentional_walks, strikeouts, hit_by_pitch, stolen_bases, caught_stealing, \
                sac_flies, sac_bunts, total_bases, gidp, api_avg, api_obp, api_slg, api_ops \
         FROM {} {filter}",
        read_fn(&dataset_glob(data_dir, "batting"))?
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(values)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(batting_from_row(row)?);
    }
    Ok(out)
}

fn query_pitching(
    conn: &Connection,
    data_dir: &Path,
    filter: &str,
    values: impl duckdb::Params,
) -> Result<Vec<PitchingStint>, QueryError> {
    let sql = format!(
        "SELECT season, level, player_id, player_name, team_id, team_name, position, \
                games, games_started, wins, losses, saves, holds, blown_saves, save_opportunities, \
                complete_games, shutouts, outs, innings_pitched, hits, runs, earned_runs, home_runs, \
                walks, intentional_walks, strikeouts, hit_batters, balks, wild_pitches, \
                batters_faced, strikes, number_of_pitches, api_era, api_whip \
         FROM {} {filter}",
        read_fn(&dataset_glob(data_dir, "pitching"))?
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(values)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(pitching_from_row(row)?);
    }
    Ok(out)
}

fn batting_from_row(row: &Row<'_>) -> Result<BattingStint, QueryError> {
    let mut stint = BattingStint {
        season: get_u16(row, 0)?,
        level: parse_level(&row.get::<_, String>(1)?)?,
        player_id: get_u32(row, 2)?,
        player_name: row.get(3)?,
        team_id: get_u32(row, 4)?,
        team_name: row.get(5)?,
        position: row.get(6)?,
        games: get_u32(row, 7)?,
        at_bats: get_u32(row, 8)?,
        plate_appearances: get_u32(row, 9)?,
        runs: get_u32(row, 10)?,
        hits: get_u32(row, 11)?,
        doubles: get_u32(row, 12)?,
        triples: get_u32(row, 13)?,
        home_runs: get_u32(row, 14)?,
        rbi: get_u32(row, 15)?,
        walks: get_u32(row, 16)?,
        intentional_walks: get_u32(row, 17)?,
        strikeouts: get_u32(row, 18)?,
        hit_by_pitch: get_u32(row, 19)?,
        stolen_bases: get_u32(row, 20)?,
        caught_stealing: get_u32(row, 21)?,
        sac_flies: get_u32(row, 22)?,
        sac_bunts: get_u32(row, 23)?,
        total_bases: get_u32(row, 24)?,
        gidp: get_u32(row, 25)?,
        avg: None,
        obp: None,
        slg: None,
        ops: None,
        api_avg: row.get(26)?,
        api_obp: row.get(27)?,
        api_slg: row.get(28)?,
        api_ops: row.get(29)?,
    };
    stint.recompute_rates();
    Ok(stint)
}

fn pitching_from_row(row: &Row<'_>) -> Result<PitchingStint, QueryError> {
    let mut stint = PitchingStint {
        season: get_u16(row, 0)?,
        level: parse_level(&row.get::<_, String>(1)?)?,
        player_id: get_u32(row, 2)?,
        player_name: row.get(3)?,
        team_id: get_u32(row, 4)?,
        team_name: row.get(5)?,
        position: row.get(6)?,
        games: get_u32(row, 7)?,
        games_started: get_u32(row, 8)?,
        wins: get_u32(row, 9)?,
        losses: get_u32(row, 10)?,
        saves: get_u32(row, 11)?,
        holds: get_u32(row, 12)?,
        blown_saves: get_u32(row, 13)?,
        save_opportunities: get_u32(row, 14)?,
        complete_games: get_u32(row, 15)?,
        shutouts: get_u32(row, 16)?,
        outs: get_u32(row, 17)?,
        innings_pitched: row.get(18)?,
        hits: get_u32(row, 19)?,
        runs: get_u32(row, 20)?,
        earned_runs: get_u32(row, 21)?,
        home_runs: get_u32(row, 22)?,
        walks: get_u32(row, 23)?,
        intentional_walks: get_u32(row, 24)?,
        strikeouts: get_u32(row, 25)?,
        hit_batters: get_u32(row, 26)?,
        balks: get_u32(row, 27)?,
        wild_pitches: get_u32(row, 28)?,
        batters_faced: get_u32(row, 29)?,
        strikes: get_u32(row, 30)?,
        number_of_pitches: get_u32(row, 31)?,
        era: None,
        whip: None,
        api_era: row.get(32)?,
        api_whip: row.get(33)?,
    };
    stint.recompute_rates();
    Ok(stint)
}

fn read_batting_gamelog(conn: &Connection, path: &Path) -> Result<Vec<GameLogEntry>, QueryError> {
    let sql = format!(
        "SELECT season, level, player_id, player_name, game_pk, game_date, team_id, team_name, \
                opponent_id, opponent_name, is_home, is_win, position, summary, \
                at_bats, plate_appearances, runs, hits, doubles, triples, home_runs, rbi, \
                walks, strikeouts, hit_by_pitch, total_bases, sac_flies \
         FROM {} ORDER BY game_date, game_pk",
        read_fn(path)?
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut games = Vec::new();
    while let Some(row) = rows.next()? {
        let mut game = GameLogEntry {
            season: get_u16(row, 0)?,
            level: parse_level(&row.get::<_, String>(1)?)?,
            group: StatGroup::Hitting,
            player_id: get_u32(row, 2)?,
            player_name: row.get(3)?,
            game_pk: get_u64(row, 4)?,
            game_date: row.get(5)?,
            team_id: get_u32(row, 6)?,
            team_name: row.get(7)?,
            opponent_id: get_opt_u32(row, 8)?,
            opponent_name: row.get(9)?,
            is_home: row.get(10)?,
            is_win: row.get(11)?,
            position: row.get(12)?,
            summary: row.get(13)?,
            at_bats: get_u32(row, 14)?,
            plate_appearances: get_u32(row, 15)?,
            runs: get_u32(row, 16)?,
            hits: get_u32(row, 17)?,
            doubles: get_u32(row, 18)?,
            triples: get_u32(row, 19)?,
            home_runs: get_u32(row, 20)?,
            rbi: get_u32(row, 21)?,
            walks: get_u32(row, 22)?,
            strikeouts: get_u32(row, 23)?,
            hit_by_pitch: get_u32(row, 24)?,
            total_bases: get_u32(row, 25)?,
            sac_flies: get_u32(row, 26)?,
            outs: 0,
            innings_pitched: String::new(),
            earned_runs: 0,
            avg: None,
            era: None,
            whip: None,
        };
        game.recompute_rates();
        games.push(game);
    }
    Ok(games)
}

fn read_pitching_gamelog(conn: &Connection, path: &Path) -> Result<Vec<GameLogEntry>, QueryError> {
    let sql = format!(
        "SELECT season, level, player_id, player_name, game_pk, game_date, team_id, team_name, \
                opponent_id, opponent_name, is_home, is_win, position, summary, \
                outs, innings_pitched, hits, runs, earned_runs, home_runs, walks, strikeouts, \
                hit_batters \
         FROM {} ORDER BY game_date, game_pk",
        read_fn(path)?
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut games = Vec::new();
    while let Some(row) = rows.next()? {
        let mut game = GameLogEntry {
            season: get_u16(row, 0)?,
            level: parse_level(&row.get::<_, String>(1)?)?,
            group: StatGroup::Pitching,
            player_id: get_u32(row, 2)?,
            player_name: row.get(3)?,
            game_pk: get_u64(row, 4)?,
            game_date: row.get(5)?,
            team_id: get_u32(row, 6)?,
            team_name: row.get(7)?,
            opponent_id: get_opt_u32(row, 8)?,
            opponent_name: row.get(9)?,
            is_home: row.get(10)?,
            is_win: row.get(11)?,
            position: row.get(12)?,
            summary: row.get(13)?,
            at_bats: 0,
            plate_appearances: 0,
            runs: get_u32(row, 17)?,
            hits: get_u32(row, 16)?,
            doubles: 0,
            triples: 0,
            home_runs: get_u32(row, 19)?,
            rbi: 0,
            walks: get_u32(row, 20)?,
            strikeouts: get_u32(row, 21)?,
            hit_by_pitch: get_u32(row, 22)?,
            total_bases: 0,
            sac_flies: 0,
            outs: get_u32(row, 14)?,
            innings_pitched: row.get(15)?,
            earned_runs: get_u32(row, 18)?,
            avg: None,
            era: None,
            whip: None,
        };
        game.recompute_rates();
        games.push(game);
    }
    Ok(games)
}

fn team_from_row(row: &Row<'_>) -> Result<Team, QueryError> {
    Ok(Team {
        team_id: get_u32(row, 0)?,
        season: get_u16(row, 1)?,
        level: parse_level(&row.get::<_, String>(2)?)?,
        name: row.get(3)?,
        abbreviation: row.get(4)?,
        team_name: row.get(5)?,
        location_name: row.get(6)?,
        league_name: row.get(7)?,
        division_name: row.get(8)?,
        parent_org_id: get_opt_u32(row, 9)?,
        parent_org_name: row.get(10)?,
    })
}

fn connect() -> Result<Connection, QueryError> {
    Ok(Connection::open_in_memory()?)
}

fn read_fn(path: &Path) -> Result<String, QueryError> {
    let text = path.to_string_lossy();
    if !paths::is_safe_sql_path(&text) {
        return Err(QueryError::UnsafePath);
    }
    Ok(format!("read_parquet('{text}', hive_partitioning = false)"))
}

fn require_partition(
    data_dir: &Path,
    dataset: &'static str,
    season: u16,
    level: Level,
) -> Result<PathBuf, QueryError> {
    let path = partition_file(data_dir, dataset, season, level);
    if path.is_file() {
        Ok(path)
    } else {
        Err(QueryError::DataNotLoaded {
            dataset,
            season,
            level,
        })
    }
}

fn dataset_has_files(data_dir: &Path, dataset: &str) -> bool {
    fn walk(dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("parquet") {
                return true;
            }
            if path.is_dir() && walk(&path) {
                return true;
            }
        }
        false
    }
    walk(&paths::parquet_root(data_dir).join(dataset))
}

fn parse_level(slug: &str) -> Result<Level, QueryError> {
    Level::parse(slug).map_err(|err| QueryError::Message(err.to_string()))
}

fn get_u32(row: &Row<'_>, idx: usize) -> Result<u32, QueryError> {
    if let Ok(value) = row.get::<_, i32>(idx) {
        return u32::try_from(value).map_err(|err| QueryError::Message(err.to_string()));
    }
    if let Ok(value) = row.get::<_, i64>(idx) {
        return u32::try_from(value).map_err(|err| QueryError::Message(err.to_string()));
    }
    let value: i128 = row.get(idx)?;
    u32::try_from(value).map_err(|err| QueryError::Message(err.to_string()))
}

fn get_opt_u32(row: &Row<'_>, idx: usize) -> Result<Option<u32>, QueryError> {
    if let Ok(value) = row.get::<_, Option<i32>>(idx) {
        return value
            .map(|number| u32::try_from(number).map_err(|err| QueryError::Message(err.to_string())))
            .transpose();
    }
    if let Ok(value) = row.get::<_, Option<i64>>(idx) {
        return value
            .map(|number| u32::try_from(number).map_err(|err| QueryError::Message(err.to_string())))
            .transpose();
    }
    let value: Option<i128> = row.get(idx)?;
    value
        .map(|number| u32::try_from(number).map_err(|err| QueryError::Message(err.to_string())))
        .transpose()
}

fn get_u16(row: &Row<'_>, idx: usize) -> Result<u16, QueryError> {
    let value = get_u32(row, idx)?;
    u16::try_from(value).map_err(|err| QueryError::Message(err.to_string()))
}

fn get_u64(row: &Row<'_>, idx: usize) -> Result<u64, QueryError> {
    if let Ok(value) = row.get::<_, i64>(idx) {
        return u64::try_from(value).map_err(|err| QueryError::Message(err.to_string()));
    }
    let value: i128 = row.get(idx)?;
    u64::try_from(value).map_err(|err| QueryError::Message(err.to_string()))
}
