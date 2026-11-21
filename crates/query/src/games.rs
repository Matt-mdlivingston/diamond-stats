//! Schedule, box score, play-by-play, and batter-vs-pitcher reads.
//!
//! DuckDB scans the Parquet files. Rates on a box line are recomputed here from
//! the counting columns. A matchup only sees games whose plays have been stored.

use std::path::{Path, PathBuf};

use diamond_stats_core::paths::{self, dataset_glob};
use diamond_stats_core::rates::{self, Innings};
use diamond_stats_core::{
    BoxBattingLine, BoxPitchingLine, Level, Pitch, PlateAppearance, ScheduledGame,
};
use duckdb::{Connection, Row};

use crate::error::QueryError;
use crate::store::{
    StatsStore, connect, dataset_has_files, get_opt_u32, get_u16, get_u32, get_u64, parse_level,
    read_fn,
};

/// A schedule date that may not have been fetched yet.
#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleLoad {
    /// No file has been written for this date.
    NotMaterialized,
    /// The file exists. It may contain zero games.
    Games(Vec<ScheduledGame>),
}

/// Batting and pitching lines for one game.
#[derive(Debug, Clone)]
pub struct BoxScore {
    /// Batters in the box score.
    pub batting: Vec<BoxBattingLine>,
    /// Pitchers in the box score.
    pub pitching: Vec<BoxPitchingLine>,
}

/// Plays and pitches stored for one game.
#[derive(Debug, Clone)]
pub struct PlayFeed {
    /// Plate appearances.
    pub plate_appearances: Vec<PlateAppearance>,
    /// Pitch events. Empty when the feed had none.
    pub pitches: Vec<Pitch>,
}

/// A batter-vs-pitcher query over stored plate appearances.
#[derive(Debug, Clone, Copy)]
pub struct MatchupRequest {
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Batter. At least one of batter or pitcher is required.
    pub batter_id: Option<u32>,
    /// Pitcher.
    pub pitcher_id: Option<u32>,
    /// Maximum plate appearances. 1..=200.
    pub limit: u32,
}

impl StatsStore {
    /// Games stored for one schedule date.
    pub fn schedule(
        &self,
        season: u16,
        level: Level,
        date: &str,
    ) -> Result<ScheduleLoad, QueryError> {
        let path = paths::schedule_file(self.data_dir(), season, level, date);
        if !path.is_file() {
            return Ok(ScheduleLoad::NotMaterialized);
        }
        let conn = connect()?;
        Ok(ScheduleLoad::Games(read_schedule(&conn, &path, None)?))
    }

    /// One game, from a `gamePk` header or from any stored schedule file.
    ///
    /// When the same game appears as a postponement and later as a final, the
    /// final row wins.
    pub fn game_by_pk(&self, game_pk: u64) -> Result<Option<ScheduledGame>, QueryError> {
        let conn = connect()?;
        let mut found = Vec::new();
        let header_name = format!("game={game_pk}.parquet");
        for path in find_named(
            &paths::parquet_root(self.data_dir()).join("game_header"),
            &header_name,
        ) {
            found.extend(read_schedule(&conn, &path, None)?);
        }
        if dataset_has_files(self.data_dir(), "schedule") {
            let glob = dataset_glob(self.data_dir(), "schedule");
            found.extend(read_schedule(&conn, &glob, Some(game_pk))?);
        }
        Ok(prefer_game(found))
    }

    /// Box score lines, if both Parquet files have been written.
    pub fn boxscore(&self, game: &ScheduledGame) -> Result<Option<BoxScore>, QueryError> {
        let batting_path =
            paths::box_batting_file(self.data_dir(), game.season, game.level, game.game_pk);
        let pitching_path =
            paths::box_pitching_file(self.data_dir(), game.season, game.level, game.game_pk);
        if !batting_path.is_file() || !pitching_path.is_file() {
            return Ok(None);
        }
        let conn = connect()?;
        Ok(Some(BoxScore {
            batting: read_box_batting(&conn, &batting_path)?,
            pitching: read_box_pitching(&conn, &pitching_path)?,
        }))
    }

    /// Plays and pitches, if both files have been written.
    pub fn plays(&self, game: &ScheduledGame) -> Result<Option<PlayFeed>, QueryError> {
        let plays_path = paths::plays_file(self.data_dir(), game.season, game.level, game.game_pk);
        let pitches_path =
            paths::pitches_file(self.data_dir(), game.season, game.level, game.game_pk);
        if !plays_path.is_file() || !pitches_path.is_file() {
            return Ok(None);
        }
        let conn = connect()?;
        Ok(Some(PlayFeed {
            plate_appearances: read_plays(&conn, &plays_path)?,
            pitches: read_pitches(&conn, &pitches_path)?,
        }))
    }

    /// Plate appearances between a batter and a pitcher in stored games.
    ///
    /// This does not fetch missing games. An empty list means those players
    /// have no stored plate appearance at this season and level.
    pub fn matchups(&self, request: MatchupRequest) -> Result<Vec<PlateAppearance>, QueryError> {
        if request.limit == 0 || request.limit > 200 {
            return Err(QueryError::Message(
                "limit must be between 1 and 200".into(),
            ));
        }
        if request.batter_id.is_none() && request.pitcher_id.is_none() {
            return Err(QueryError::Message(
                "matchup requires batter_id or pitcher_id".into(),
            ));
        }
        if !dataset_has_files(self.data_dir(), "plays") {
            return Err(QueryError::DatasetMissing { dataset: "plays" });
        }
        let conn = connect()?;
        let from = read_fn(&dataset_glob(self.data_dir(), "plays"))?;
        let columns = play_columns();
        let (predicate, values) = matchup_predicate(&request);
        let sql = format!(
            "SELECT {columns} FROM {from} WHERE season = ? AND level = ? AND {predicate} \
             ORDER BY official_date, game_pk, at_bat_index"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(duckdb::params_from_iter(values))?;
        let mut plays = Vec::new();
        while let Some(row) = rows.next()? {
            plays.push(play_from_row(row)?);
            if plays.len() as u32 >= request.limit {
                break;
            }
        }
        Ok(plays)
    }
}

fn matchup_predicate(request: &MatchupRequest) -> (String, Vec<duckdb::types::Value>) {
    use duckdb::types::Value;
    let mut predicate = String::new();
    let mut values = vec![
        Value::from(i32::from(request.season)),
        Value::from(request.level.slug().to_string()),
    ];
    if let Some(batter) = request.batter_id {
        predicate.push_str("batter_id = ?");
        values.push(Value::from(batter as i32));
    }
    if let Some(pitcher) = request.pitcher_id {
        if !predicate.is_empty() {
            predicate.push_str(" AND ");
        }
        predicate.push_str("pitcher_id = ?");
        values.push(Value::from(pitcher as i32));
    }
    (predicate, values)
}

fn prefer_game(mut games: Vec<ScheduledGame>) -> Option<ScheduledGame> {
    games.sort_by(|left, right| {
        right
            .boxscore_ready()
            .cmp(&left.boxscore_ready())
            .then(right.official_date.cmp(&left.official_date))
            .then(right.schedule_date.cmp(&left.schedule_date))
            .then(left.game_pk.cmp(&right.game_pk))
    });
    games.into_iter().next()
}

fn read_schedule(
    conn: &Connection,
    path: &Path,
    game_pk: Option<u64>,
) -> Result<Vec<ScheduledGame>, QueryError> {
    let from = read_fn(path)?;
    let (filter, values): (String, Vec<duckdb::types::Value>) = if let Some(game_pk) = game_pk {
        (
            "WHERE game_pk = ?".to_string(),
            vec![duckdb::types::Value::from(game_pk as i64)],
        )
    } else {
        (String::new(), Vec::new())
    };
    let sql = format!(
        "SELECT game_pk, season, level, schedule_date, official_date, game_type, \
                detailed_state, abstract_state, coded_state, away_team_id, away_team_name, \
                away_score, home_team_id, home_team_name, home_score, venue_name, \
                double_header, game_number \
         FROM {from} {filter} ORDER BY game_pk"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(duckdb::params_from_iter(values))?;
    let mut games = Vec::new();
    while let Some(row) = rows.next()? {
        games.push(schedule_from_row(row)?);
    }
    Ok(games)
}

fn schedule_from_row(row: &Row<'_>) -> Result<ScheduledGame, QueryError> {
    Ok(ScheduledGame {
        game_pk: get_u64(row, 0)?,
        season: get_u16(row, 1)?,
        level: parse_level(&row.get::<_, String>(2)?)?,
        schedule_date: row.get(3)?,
        official_date: row.get(4)?,
        game_type: row.get(5)?,
        detailed_state: row.get(6)?,
        abstract_state: row.get(7)?,
        coded_state: row.get(8)?,
        away_team_id: get_u32(row, 9)?,
        away_team_name: row.get(10)?,
        away_score: get_opt_u32(row, 11)?,
        home_team_id: get_u32(row, 12)?,
        home_team_name: row.get(13)?,
        home_score: get_opt_u32(row, 14)?,
        venue_name: row.get(15)?,
        double_header: row.get(16)?,
        game_number: get_u32(row, 17)?,
    })
}

fn read_box_batting(conn: &Connection, path: &Path) -> Result<Vec<BoxBattingLine>, QueryError> {
    let sql = format!(
        "SELECT season, level, game_pk, schedule_date, official_date, team_id, team_name, \
                is_home, player_id, player_name, position, batting_order, at_bats, \
                plate_appearances, runs, hits, doubles, triples, home_runs, rbi, walks, \
                strikeouts, hit_by_pitch, sac_flies, total_bases, stolen_bases, summary \
         FROM {} ORDER BY team_id, batting_order NULLS LAST, player_id",
        read_fn(path)?
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut lines = Vec::new();
    while let Some(row) = rows.next()? {
        let mut line = BoxBattingLine {
            season: get_u16(row, 0)?,
            level: parse_level(&row.get::<_, String>(1)?)?,
            game_pk: get_u64(row, 2)?,
            schedule_date: row.get(3)?,
            official_date: row.get(4)?,
            team_id: get_u32(row, 5)?,
            team_name: row.get(6)?,
            is_home: row.get(7)?,
            player_id: get_u32(row, 8)?,
            player_name: row.get(9)?,
            position: row.get(10)?,
            batting_order: get_opt_u32(row, 11)?,
            at_bats: get_u32(row, 12)?,
            plate_appearances: get_u32(row, 13)?,
            runs: get_u32(row, 14)?,
            hits: get_u32(row, 15)?,
            doubles: get_u32(row, 16)?,
            triples: get_u32(row, 17)?,
            home_runs: get_u32(row, 18)?,
            rbi: get_u32(row, 19)?,
            walks: get_u32(row, 20)?,
            strikeouts: get_u32(row, 21)?,
            hit_by_pitch: get_u32(row, 22)?,
            sac_flies: get_u32(row, 23)?,
            total_bases: get_u32(row, 24)?,
            stolen_bases: get_u32(row, 25)?,
            summary: row.get(26)?,
            avg: None,
        };
        line.avg = rates::batting_average(line.hits, line.at_bats);
        lines.push(line);
    }
    Ok(lines)
}

fn read_box_pitching(conn: &Connection, path: &Path) -> Result<Vec<BoxPitchingLine>, QueryError> {
    let sql = format!(
        "SELECT season, level, game_pk, schedule_date, official_date, team_id, team_name, \
                is_home, player_id, player_name, position, outs, innings_pitched, hits, runs, \
                earned_runs, home_runs, walks, strikeouts, hit_batters, batters_faced, \
                number_of_pitches, wins, losses, saves, summary \
         FROM {} ORDER BY team_id, outs DESC, player_id",
        read_fn(path)?
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut lines = Vec::new();
    while let Some(row) = rows.next()? {
        let outs = get_u32(row, 11)?;
        let mut line = BoxPitchingLine {
            season: get_u16(row, 0)?,
            level: parse_level(&row.get::<_, String>(1)?)?,
            game_pk: get_u64(row, 2)?,
            schedule_date: row.get(3)?,
            official_date: row.get(4)?,
            team_id: get_u32(row, 5)?,
            team_name: row.get(6)?,
            is_home: row.get(7)?,
            player_id: get_u32(row, 8)?,
            player_name: row.get(9)?,
            position: row.get(10)?,
            outs,
            innings_pitched: row.get(12)?,
            hits: get_u32(row, 13)?,
            runs: get_u32(row, 14)?,
            earned_runs: get_u32(row, 15)?,
            home_runs: get_u32(row, 16)?,
            walks: get_u32(row, 17)?,
            strikeouts: get_u32(row, 18)?,
            hit_batters: get_u32(row, 19)?,
            batters_faced: get_u32(row, 20)?,
            number_of_pitches: get_u32(row, 21)?,
            wins: get_u32(row, 22)?,
            losses: get_u32(row, 23)?,
            saves: get_u32(row, 24)?,
            summary: row.get(25)?,
            era: None,
            whip: None,
        };
        let innings = Innings::from_outs(outs);
        line.era = rates::earned_run_average(line.earned_runs, innings);
        line.whip = rates::whip(line.walks, line.hits, innings);
        lines.push(line);
    }
    Ok(lines)
}

fn play_columns() -> &'static str {
    "game_pk, season, level, schedule_date, official_date, at_bat_index, inning, half_inning, \
     batter_id, batter_name, pitcher_id, pitcher_name, bat_side, pitch_hand, event, event_type, \
     description, rbi, is_scoring_play, away_score, home_score"
}

fn read_plays(conn: &Connection, path: &Path) -> Result<Vec<PlateAppearance>, QueryError> {
    let sql = format!(
        "SELECT {} FROM {} ORDER BY at_bat_index",
        play_columns(),
        read_fn(path)?
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut plays = Vec::new();
    while let Some(row) = rows.next()? {
        plays.push(play_from_row(row)?);
    }
    Ok(plays)
}

fn play_from_row(row: &Row<'_>) -> Result<PlateAppearance, QueryError> {
    Ok(PlateAppearance {
        game_pk: get_u64(row, 0)?,
        season: get_u16(row, 1)?,
        level: parse_level(&row.get::<_, String>(2)?)?,
        schedule_date: row.get(3)?,
        official_date: row.get(4)?,
        at_bat_index: get_u32(row, 5)?,
        inning: get_u32(row, 6)?,
        half_inning: row.get(7)?,
        batter_id: get_u32(row, 8)?,
        batter_name: row.get(9)?,
        pitcher_id: get_u32(row, 10)?,
        pitcher_name: row.get(11)?,
        bat_side: row.get(12)?,
        pitch_hand: row.get(13)?,
        event: row.get(14)?,
        event_type: row.get(15)?,
        description: row.get(16)?,
        rbi: get_u32(row, 17)?,
        is_scoring_play: row.get(18)?,
        away_score: get_u32(row, 19)?,
        home_score: get_u32(row, 20)?,
    })
}

fn read_pitches(conn: &Connection, path: &Path) -> Result<Vec<Pitch>, QueryError> {
    let sql = format!(
        "SELECT game_pk, season, level, schedule_date, official_date, at_bat_index, pitch_number, \
                call_code, call_description, pitch_type_code, pitch_type, start_speed, is_ball, \
                is_strike, is_in_play \
         FROM {} ORDER BY at_bat_index, pitch_number",
        read_fn(path)?
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut pitches = Vec::new();
    while let Some(row) = rows.next()? {
        pitches.push(Pitch {
            game_pk: get_u64(row, 0)?,
            season: get_u16(row, 1)?,
            level: parse_level(&row.get::<_, String>(2)?)?,
            schedule_date: row.get(3)?,
            official_date: row.get(4)?,
            at_bat_index: get_u32(row, 5)?,
            pitch_number: get_u32(row, 6)?,
            call_code: row.get(7)?,
            call_description: row.get(8)?,
            pitch_type_code: row.get(9)?,
            pitch_type: row.get(10)?,
            start_speed: row.get(11)?,
            is_ball: row.get(12)?,
            is_strike: row.get(13)?,
            is_in_play: row.get(14)?,
        });
    }
    Ok(pitches)
}

fn find_named(root: &Path, file_name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, file_name, &mut out);
    out
}

fn walk(dir: &Path, file_name: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, file_name, out);
        } else if path.file_name().and_then(|name| name.to_str()) == Some(file_name) {
            out.push(path);
        }
    }
}
