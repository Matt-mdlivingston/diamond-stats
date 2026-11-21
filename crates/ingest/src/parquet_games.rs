//! Parquet for schedules, box scores, and play-by-play.
//!
//! Column order is the contract with `diamond-stats-query`. Rate stats are not
//! stored. An empty slice still writes a file so a fetched game is not fetched again.

use std::path::Path;
use std::sync::Arc;

use arrow::array::{ArrayRef, RecordBatch};
use arrow::datatypes::{DataType, Field, Schema};
use diamond_stats_core::{BoxBattingLine, BoxPitchingLine, Pitch, PlateAppearance, ScheduledGame};

use crate::error::IngestError;
use crate::parquet::{
    boolean, bools, f64_opt, float_null, i32_opt, i32s, i64s, int, int_null, map, utf, utf_null,
    utf8, utf8_opt, write_batch,
};

/// Write every game returned for one schedule date, including an empty day.
pub fn write_schedule(path: &Path, rows: &[ScheduledGame]) -> Result<(), IngestError> {
    write_schedule_rows(path, rows)
}

/// Write the one-row header used when a game was resolved from `gamePk` alone.
pub fn write_game_header(path: &Path, game: &ScheduledGame) -> Result<(), IngestError> {
    write_schedule_rows(path, std::slice::from_ref(game))
}

/// Write batting box lines. An empty slice still creates the file.
pub fn write_box_batting(path: &Path, rows: &[BoxBattingLine]) -> Result<(), IngestError> {
    let schema = box_batting_schema();
    let batch = if rows.is_empty() {
        RecordBatch::new_empty(schema.clone())
    } else {
        let arrays: Vec<ArrayRef> = vec![
            i32s(&map(rows, |row| u32::from(row.season))),
            utf8(&map(rows, |row| row.level.slug().to_string())),
            i64s(&map(rows, |row| row.game_pk)),
            utf8(&map(rows, |row| row.schedule_date.clone())),
            utf8(&map(rows, |row| row.official_date.clone())),
            i32s(&map(rows, |row| row.team_id)),
            utf8(&map(rows, |row| row.team_name.clone())),
            bools(&map(rows, |row| row.is_home)),
            i32s(&map(rows, |row| row.player_id)),
            utf8(&map(rows, |row| row.player_name.clone())),
            utf8_opt(&map(rows, |row| row.position.clone())),
            i32_opt(&map(rows, |row| row.batting_order)),
            i32s(&map(rows, |row| row.at_bats)),
            i32s(&map(rows, |row| row.plate_appearances)),
            i32s(&map(rows, |row| row.runs)),
            i32s(&map(rows, |row| row.hits)),
            i32s(&map(rows, |row| row.doubles)),
            i32s(&map(rows, |row| row.triples)),
            i32s(&map(rows, |row| row.home_runs)),
            i32s(&map(rows, |row| row.rbi)),
            i32s(&map(rows, |row| row.walks)),
            i32s(&map(rows, |row| row.strikeouts)),
            i32s(&map(rows, |row| row.hit_by_pitch)),
            i32s(&map(rows, |row| row.sac_flies)),
            i32s(&map(rows, |row| row.total_bases)),
            i32s(&map(rows, |row| row.stolen_bases)),
            utf8_opt(&map(rows, |row| row.summary.clone())),
        ];
        RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|err| IngestError::Arrow(err.to_string()))?
    };
    write_batch(path, schema, batch)
}

/// Write pitching box lines, including an empty file.
pub fn write_box_pitching(path: &Path, rows: &[BoxPitchingLine]) -> Result<(), IngestError> {
    let schema = box_pitching_schema();
    let batch = if rows.is_empty() {
        RecordBatch::new_empty(schema.clone())
    } else {
        let arrays: Vec<ArrayRef> = vec![
            i32s(&map(rows, |row| u32::from(row.season))),
            utf8(&map(rows, |row| row.level.slug().to_string())),
            i64s(&map(rows, |row| row.game_pk)),
            utf8(&map(rows, |row| row.schedule_date.clone())),
            utf8(&map(rows, |row| row.official_date.clone())),
            i32s(&map(rows, |row| row.team_id)),
            utf8(&map(rows, |row| row.team_name.clone())),
            bools(&map(rows, |row| row.is_home)),
            i32s(&map(rows, |row| row.player_id)),
            utf8(&map(rows, |row| row.player_name.clone())),
            utf8_opt(&map(rows, |row| row.position.clone())),
            i32s(&map(rows, |row| row.outs)),
            utf8(&map(rows, |row| row.innings_pitched.clone())),
            i32s(&map(rows, |row| row.hits)),
            i32s(&map(rows, |row| row.runs)),
            i32s(&map(rows, |row| row.earned_runs)),
            i32s(&map(rows, |row| row.home_runs)),
            i32s(&map(rows, |row| row.walks)),
            i32s(&map(rows, |row| row.strikeouts)),
            i32s(&map(rows, |row| row.hit_batters)),
            i32s(&map(rows, |row| row.batters_faced)),
            i32s(&map(rows, |row| row.number_of_pitches)),
            i32s(&map(rows, |row| row.wins)),
            i32s(&map(rows, |row| row.losses)),
            i32s(&map(rows, |row| row.saves)),
            utf8_opt(&map(rows, |row| row.summary.clone())),
        ];
        RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|err| IngestError::Arrow(err.to_string()))?
    };
    write_batch(path, schema, batch)
}

/// Write plate appearances. An empty slice still creates the file.
pub fn write_plays(path: &Path, rows: &[PlateAppearance]) -> Result<(), IngestError> {
    let schema = plays_schema();
    let batch = if rows.is_empty() {
        RecordBatch::new_empty(schema.clone())
    } else {
        let arrays: Vec<ArrayRef> = vec![
            i64s(&map(rows, |row| row.game_pk)),
            i32s(&map(rows, |row| u32::from(row.season))),
            utf8(&map(rows, |row| row.level.slug().to_string())),
            utf8(&map(rows, |row| row.schedule_date.clone())),
            utf8(&map(rows, |row| row.official_date.clone())),
            i32s(&map(rows, |row| row.at_bat_index)),
            i32s(&map(rows, |row| row.inning)),
            utf8(&map(rows, |row| row.half_inning.clone())),
            i32s(&map(rows, |row| row.batter_id)),
            utf8(&map(rows, |row| row.batter_name.clone())),
            i32s(&map(rows, |row| row.pitcher_id)),
            utf8(&map(rows, |row| row.pitcher_name.clone())),
            utf8_opt(&map(rows, |row| row.bat_side.clone())),
            utf8_opt(&map(rows, |row| row.pitch_hand.clone())),
            utf8_opt(&map(rows, |row| row.event.clone())),
            utf8_opt(&map(rows, |row| row.event_type.clone())),
            utf8_opt(&map(rows, |row| row.description.clone())),
            i32s(&map(rows, |row| row.rbi)),
            bools(&map(rows, |row| row.is_scoring_play)),
            i32s(&map(rows, |row| row.away_score)),
            i32s(&map(rows, |row| row.home_score)),
        ];
        RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|err| IngestError::Arrow(err.to_string()))?
    };
    write_batch(path, schema, batch)
}

/// Write pitches. An empty file means the feed had no pitch events.
pub fn write_pitches(path: &Path, rows: &[Pitch]) -> Result<(), IngestError> {
    let schema = pitches_schema();
    let batch = if rows.is_empty() {
        RecordBatch::new_empty(schema.clone())
    } else {
        let arrays: Vec<ArrayRef> = vec![
            i64s(&map(rows, |row| row.game_pk)),
            i32s(&map(rows, |row| u32::from(row.season))),
            utf8(&map(rows, |row| row.level.slug().to_string())),
            utf8(&map(rows, |row| row.schedule_date.clone())),
            utf8(&map(rows, |row| row.official_date.clone())),
            i32s(&map(rows, |row| row.at_bat_index)),
            i32s(&map(rows, |row| row.pitch_number)),
            utf8_opt(&map(rows, |row| row.call_code.clone())),
            utf8_opt(&map(rows, |row| row.call_description.clone())),
            utf8_opt(&map(rows, |row| row.pitch_type_code.clone())),
            utf8_opt(&map(rows, |row| row.pitch_type.clone())),
            f64_opt(&map(rows, |row| row.start_speed)),
            bools(&map(rows, |row| row.is_ball)),
            bools(&map(rows, |row| row.is_strike)),
            bools(&map(rows, |row| row.is_in_play)),
        ];
        RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|err| IngestError::Arrow(err.to_string()))?
    };
    write_batch(path, schema, batch)
}

fn write_schedule_rows(path: &Path, rows: &[ScheduledGame]) -> Result<(), IngestError> {
    let schema = schedule_schema();
    let batch = if rows.is_empty() {
        RecordBatch::new_empty(schema.clone())
    } else {
        let arrays: Vec<ArrayRef> = vec![
            i64s(&map(rows, |row| row.game_pk)),
            i32s(&map(rows, |row| u32::from(row.season))),
            utf8(&map(rows, |row| row.level.slug().to_string())),
            utf8(&map(rows, |row| row.schedule_date.clone())),
            utf8(&map(rows, |row| row.official_date.clone())),
            utf8(&map(rows, |row| row.game_type.clone())),
            utf8(&map(rows, |row| row.detailed_state.clone())),
            utf8(&map(rows, |row| row.abstract_state.clone())),
            utf8(&map(rows, |row| row.coded_state.clone())),
            i32s(&map(rows, |row| row.away_team_id)),
            utf8(&map(rows, |row| row.away_team_name.clone())),
            i32_opt(&map(rows, |row| row.away_score)),
            i32s(&map(rows, |row| row.home_team_id)),
            utf8(&map(rows, |row| row.home_team_name.clone())),
            i32_opt(&map(rows, |row| row.home_score)),
            utf8_opt(&map(rows, |row| row.venue_name.clone())),
            utf8(&map(rows, |row| row.double_header.clone())),
            i32s(&map(rows, |row| row.game_number)),
        ];
        RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|err| IngestError::Arrow(err.to_string()))?
    };
    write_batch(path, schema, batch)
}

fn schedule_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("game_pk", DataType::Int64, false),
        int("season"),
        utf("level"),
        utf("schedule_date"),
        utf("official_date"),
        utf("game_type"),
        utf("detailed_state"),
        utf("abstract_state"),
        utf("coded_state"),
        int("away_team_id"),
        utf("away_team_name"),
        int_null("away_score"),
        int("home_team_id"),
        utf("home_team_name"),
        int_null("home_score"),
        utf_null("venue_name"),
        utf("double_header"),
        int("game_number"),
    ]))
}

fn box_batting_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        int("season"),
        utf("level"),
        Field::new("game_pk", DataType::Int64, false),
        utf("schedule_date"),
        utf("official_date"),
        int("team_id"),
        utf("team_name"),
        boolean("is_home"),
        int("player_id"),
        utf("player_name"),
        utf_null("position"),
        int_null("batting_order"),
        int("at_bats"),
        int("plate_appearances"),
        int("runs"),
        int("hits"),
        int("doubles"),
        int("triples"),
        int("home_runs"),
        int("rbi"),
        int("walks"),
        int("strikeouts"),
        int("hit_by_pitch"),
        int("sac_flies"),
        int("total_bases"),
        int("stolen_bases"),
        utf_null("summary"),
    ]))
}

fn box_pitching_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        int("season"),
        utf("level"),
        Field::new("game_pk", DataType::Int64, false),
        utf("schedule_date"),
        utf("official_date"),
        int("team_id"),
        utf("team_name"),
        boolean("is_home"),
        int("player_id"),
        utf("player_name"),
        utf_null("position"),
        int("outs"),
        utf("innings_pitched"),
        int("hits"),
        int("runs"),
        int("earned_runs"),
        int("home_runs"),
        int("walks"),
        int("strikeouts"),
        int("hit_batters"),
        int("batters_faced"),
        int("number_of_pitches"),
        int("wins"),
        int("losses"),
        int("saves"),
        utf_null("summary"),
    ]))
}

fn plays_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("game_pk", DataType::Int64, false),
        int("season"),
        utf("level"),
        utf("schedule_date"),
        utf("official_date"),
        int("at_bat_index"),
        int("inning"),
        utf("half_inning"),
        int("batter_id"),
        utf("batter_name"),
        int("pitcher_id"),
        utf("pitcher_name"),
        utf_null("bat_side"),
        utf_null("pitch_hand"),
        utf_null("event"),
        utf_null("event_type"),
        utf_null("description"),
        int("rbi"),
        boolean("is_scoring_play"),
        int("away_score"),
        int("home_score"),
    ]))
}

fn pitches_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("game_pk", DataType::Int64, false),
        int("season"),
        utf("level"),
        utf("schedule_date"),
        utf("official_date"),
        int("at_bat_index"),
        int("pitch_number"),
        utf_null("call_code"),
        utf_null("call_description"),
        utf_null("pitch_type_code"),
        utf_null("pitch_type"),
        float_null("start_speed"),
        boolean("is_ball"),
        boolean("is_strike"),
        boolean("is_in_play"),
    ]))
}
