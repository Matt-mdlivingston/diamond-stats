//! Arrow record batches written as Parquet.
//!
//! Column order is part of the contract with `diamond-stats-query`, which reads
//! these files by name. Recomputed rates are not stored; the query layer
//! derives them from the counting columns.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use arrow::array::{
    ArrayRef, BooleanArray, Float64Array, Int32Array, Int64Array, RecordBatch, StringArray,
};
use arrow::datatypes::{DataType, Field, Schema};
use diamond_stats_core::{BattingStint, GameLogEntry, PitchingStint, Team};
use parquet::arrow::ArrowWriter;

use crate::error::IngestError;

/// Write batting stints, replacing any file at `path`.
pub fn write_batting(path: &Path, rows: &[BattingStint]) -> Result<(), IngestError> {
    let schema = batting_schema();
    let batch = if rows.is_empty() {
        RecordBatch::new_empty(schema.clone())
    } else {
        let arrays: Vec<ArrayRef> = vec![
            i32s(&map(rows, |row| u32::from(row.season))),
            utf8(&map(rows, |row| row.level.slug().to_string())),
            i32s(&map(rows, |row| row.player_id)),
            utf8(&map(rows, |row| row.player_name.clone())),
            i32s(&map(rows, |row| row.team_id)),
            utf8(&map(rows, |row| row.team_name.clone())),
            utf8_opt(&map(rows, |row| row.position.clone())),
            i32s(&map(rows, |row| row.games)),
            i32s(&map(rows, |row| row.at_bats)),
            i32s(&map(rows, |row| row.plate_appearances)),
            i32s(&map(rows, |row| row.runs)),
            i32s(&map(rows, |row| row.hits)),
            i32s(&map(rows, |row| row.doubles)),
            i32s(&map(rows, |row| row.triples)),
            i32s(&map(rows, |row| row.home_runs)),
            i32s(&map(rows, |row| row.rbi)),
            i32s(&map(rows, |row| row.walks)),
            i32s(&map(rows, |row| row.intentional_walks)),
            i32s(&map(rows, |row| row.strikeouts)),
            i32s(&map(rows, |row| row.hit_by_pitch)),
            i32s(&map(rows, |row| row.stolen_bases)),
            i32s(&map(rows, |row| row.caught_stealing)),
            i32s(&map(rows, |row| row.sac_flies)),
            i32s(&map(rows, |row| row.sac_bunts)),
            i32s(&map(rows, |row| row.total_bases)),
            i32s(&map(rows, |row| row.gidp)),
            f64_opt(&map(rows, |row| row.api_avg)),
            f64_opt(&map(rows, |row| row.api_obp)),
            f64_opt(&map(rows, |row| row.api_slg)),
            f64_opt(&map(rows, |row| row.api_ops)),
        ];
        RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|err| IngestError::Arrow(err.to_string()))?
    };
    write_batch(path, schema, batch)
}

/// Write pitching stints, replacing any file at `path`.
pub fn write_pitching(path: &Path, rows: &[PitchingStint]) -> Result<(), IngestError> {
    let schema = pitching_schema();
    let batch = if rows.is_empty() {
        RecordBatch::new_empty(schema.clone())
    } else {
        let arrays: Vec<ArrayRef> = vec![
            i32s(&map(rows, |row| u32::from(row.season))),
            utf8(&map(rows, |row| row.level.slug().to_string())),
            i32s(&map(rows, |row| row.player_id)),
            utf8(&map(rows, |row| row.player_name.clone())),
            i32s(&map(rows, |row| row.team_id)),
            utf8(&map(rows, |row| row.team_name.clone())),
            utf8_opt(&map(rows, |row| row.position.clone())),
            i32s(&map(rows, |row| row.games)),
            i32s(&map(rows, |row| row.games_started)),
            i32s(&map(rows, |row| row.wins)),
            i32s(&map(rows, |row| row.losses)),
            i32s(&map(rows, |row| row.saves)),
            i32s(&map(rows, |row| row.holds)),
            i32s(&map(rows, |row| row.blown_saves)),
            i32s(&map(rows, |row| row.save_opportunities)),
            i32s(&map(rows, |row| row.complete_games)),
            i32s(&map(rows, |row| row.shutouts)),
            i32s(&map(rows, |row| row.outs)),
            utf8(&map(rows, |row| row.innings_pitched.clone())),
            i32s(&map(rows, |row| row.hits)),
            i32s(&map(rows, |row| row.runs)),
            i32s(&map(rows, |row| row.earned_runs)),
            i32s(&map(rows, |row| row.home_runs)),
            i32s(&map(rows, |row| row.walks)),
            i32s(&map(rows, |row| row.intentional_walks)),
            i32s(&map(rows, |row| row.strikeouts)),
            i32s(&map(rows, |row| row.hit_batters)),
            i32s(&map(rows, |row| row.balks)),
            i32s(&map(rows, |row| row.wild_pitches)),
            i32s(&map(rows, |row| row.batters_faced)),
            i32s(&map(rows, |row| row.strikes)),
            i32s(&map(rows, |row| row.number_of_pitches)),
            f64_opt(&map(rows, |row| row.api_era)),
            f64_opt(&map(rows, |row| row.api_whip)),
        ];
        RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|err| IngestError::Arrow(err.to_string()))?
    };
    write_batch(path, schema, batch)
}

/// Write the team list for a season and level.
pub fn write_teams(path: &Path, rows: &[Team]) -> Result<(), IngestError> {
    let schema = team_schema();
    let batch = if rows.is_empty() {
        RecordBatch::new_empty(schema.clone())
    } else {
        let arrays: Vec<ArrayRef> = vec![
            i32s(&map(rows, |row| row.team_id)),
            i32s(&map(rows, |row| u32::from(row.season))),
            utf8(&map(rows, |row| row.level.slug().to_string())),
            utf8(&map(rows, |row| row.name.clone())),
            utf8_opt(&map(rows, |row| row.abbreviation.clone())),
            utf8_opt(&map(rows, |row| row.team_name.clone())),
            utf8_opt(&map(rows, |row| row.location_name.clone())),
            utf8_opt(&map(rows, |row| row.league_name.clone())),
            utf8_opt(&map(rows, |row| row.division_name.clone())),
            i32_opt(&map(rows, |row| row.parent_org_id)),
            utf8_opt(&map(rows, |row| row.parent_org_name.clone())),
        ];
        RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|err| IngestError::Arrow(err.to_string()))?
    };
    write_batch(path, schema, batch)
}

/// Write a batting game log. An empty slice still creates a file so the player is materialized.
pub fn write_batting_gamelog(path: &Path, rows: &[GameLogEntry]) -> Result<(), IngestError> {
    let schema = batting_gamelog_schema();
    let batch = if rows.is_empty() {
        RecordBatch::new_empty(schema.clone())
    } else {
        let arrays = gamelog_common(rows);
        let mut arrays = arrays;
        arrays.extend([
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
            i32s(&map(rows, |row| row.total_bases)),
            i32s(&map(rows, |row| row.sac_flies)),
        ]);
        RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|err| IngestError::Arrow(err.to_string()))?
    };
    write_batch(path, schema, batch)
}

/// Write a pitching game log, including an empty file when the player did not pitch.
pub fn write_pitching_gamelog(path: &Path, rows: &[GameLogEntry]) -> Result<(), IngestError> {
    let schema = pitching_gamelog_schema();
    let batch = if rows.is_empty() {
        RecordBatch::new_empty(schema.clone())
    } else {
        let mut arrays = gamelog_common(rows);
        arrays.extend([
            i32s(&map(rows, |row| row.outs)),
            utf8(&map(rows, |row| row.innings_pitched.clone())),
            i32s(&map(rows, |row| row.hits)),
            i32s(&map(rows, |row| row.runs)),
            i32s(&map(rows, |row| row.earned_runs)),
            i32s(&map(rows, |row| row.home_runs)),
            i32s(&map(rows, |row| row.walks)),
            i32s(&map(rows, |row| row.strikeouts)),
            i32s(&map(rows, |row| row.hit_by_pitch)),
        ]);
        RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|err| IngestError::Arrow(err.to_string()))?
    };
    write_batch(path, schema, batch)
}

fn gamelog_common(rows: &[GameLogEntry]) -> Vec<ArrayRef> {
    vec![
        i32s(&map(rows, |row| u32::from(row.season))),
        utf8(&map(rows, |row| row.level.slug().to_string())),
        i32s(&map(rows, |row| row.player_id)),
        utf8(&map(rows, |row| row.player_name.clone())),
        i64s(&map(rows, |row| row.game_pk)),
        utf8(&map(rows, |row| row.game_date.clone())),
        i32s(&map(rows, |row| row.team_id)),
        utf8(&map(rows, |row| row.team_name.clone())),
        i32_opt(&map(rows, |row| row.opponent_id)),
        utf8_opt(&map(rows, |row| row.opponent_name.clone())),
        bool_opt(&map(rows, |row| row.is_home)),
        bool_opt(&map(rows, |row| row.is_win)),
        utf8_opt(&map(rows, |row| row.position.clone())),
        utf8_opt(&map(rows, |row| row.summary.clone())),
    ]
}

fn batting_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        int("season"),
        utf("level"),
        int("player_id"),
        utf("player_name"),
        int("team_id"),
        utf("team_name"),
        utf_null("position"),
        int("games"),
        int("at_bats"),
        int("plate_appearances"),
        int("runs"),
        int("hits"),
        int("doubles"),
        int("triples"),
        int("home_runs"),
        int("rbi"),
        int("walks"),
        int("intentional_walks"),
        int("strikeouts"),
        int("hit_by_pitch"),
        int("stolen_bases"),
        int("caught_stealing"),
        int("sac_flies"),
        int("sac_bunts"),
        int("total_bases"),
        int("gidp"),
        float_null("api_avg"),
        float_null("api_obp"),
        float_null("api_slg"),
        float_null("api_ops"),
    ]))
}

fn pitching_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        int("season"),
        utf("level"),
        int("player_id"),
        utf("player_name"),
        int("team_id"),
        utf("team_name"),
        utf_null("position"),
        int("games"),
        int("games_started"),
        int("wins"),
        int("losses"),
        int("saves"),
        int("holds"),
        int("blown_saves"),
        int("save_opportunities"),
        int("complete_games"),
        int("shutouts"),
        int("outs"),
        utf("innings_pitched"),
        int("hits"),
        int("runs"),
        int("earned_runs"),
        int("home_runs"),
        int("walks"),
        int("intentional_walks"),
        int("strikeouts"),
        int("hit_batters"),
        int("balks"),
        int("wild_pitches"),
        int("batters_faced"),
        int("strikes"),
        int("number_of_pitches"),
        float_null("api_era"),
        float_null("api_whip"),
    ]))
}

fn team_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        int("team_id"),
        int("season"),
        utf("level"),
        utf("name"),
        utf_null("abbreviation"),
        utf_null("team_name"),
        utf_null("location_name"),
        utf_null("league_name"),
        utf_null("division_name"),
        int_null("parent_org_id"),
        utf_null("parent_org_name"),
    ]))
}

fn batting_gamelog_schema() -> Arc<Schema> {
    let mut fields = gamelog_common_fields();
    fields.extend([
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
        int("total_bases"),
        int("sac_flies"),
    ]);
    Arc::new(Schema::new(fields))
}

fn pitching_gamelog_schema() -> Arc<Schema> {
    let mut fields = gamelog_common_fields();
    fields.extend([
        int("outs"),
        utf("innings_pitched"),
        int("hits"),
        int("runs"),
        int("earned_runs"),
        int("home_runs"),
        int("walks"),
        int("strikeouts"),
        int("hit_batters"),
    ]);
    Arc::new(Schema::new(fields))
}

fn gamelog_common_fields() -> Vec<Field> {
    vec![
        int("season"),
        utf("level"),
        int("player_id"),
        utf("player_name"),
        Field::new("game_pk", DataType::Int64, false),
        utf("game_date"),
        int("team_id"),
        utf("team_name"),
        int_null("opponent_id"),
        utf_null("opponent_name"),
        Field::new("is_home", DataType::Boolean, true),
        Field::new("is_win", DataType::Boolean, true),
        utf_null("position"),
        utf_null("summary"),
    ]
}

fn int(name: &str) -> Field {
    Field::new(name, DataType::Int32, false)
}

fn int_null(name: &str) -> Field {
    Field::new(name, DataType::Int32, true)
}

fn utf(name: &str) -> Field {
    Field::new(name, DataType::Utf8, false)
}

fn utf_null(name: &str) -> Field {
    Field::new(name, DataType::Utf8, true)
}

fn float_null(name: &str) -> Field {
    Field::new(name, DataType::Float64, true)
}

fn write_batch(path: &Path, schema: Arc<Schema>, batch: RecordBatch) -> Result<(), IngestError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("parquet.tmp");
    let file = fs::File::create(&tmp)?;
    let mut writer = ArrowWriter::try_new(file, schema, None)
        .map_err(|err| IngestError::Parquet(err.to_string()))?;
    writer
        .write(&batch)
        .map_err(|err| IngestError::Parquet(err.to_string()))?;
    writer
        .close()
        .map_err(|err| IngestError::Parquet(err.to_string()))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn map<T, U>(rows: &[T], f: impl Fn(&T) -> U) -> Vec<U> {
    rows.iter().map(f).collect()
}

fn i32s(values: &[u32]) -> ArrayRef {
    Arc::new(Int32Array::from(
        values.iter().map(|value| *value as i32).collect::<Vec<_>>(),
    ))
}

fn i32_opt(values: &[Option<u32>]) -> ArrayRef {
    Arc::new(Int32Array::from(
        values
            .iter()
            .map(|value| value.map(|number| number as i32))
            .collect::<Vec<_>>(),
    ))
}

fn i64s(values: &[u64]) -> ArrayRef {
    Arc::new(Int64Array::from(
        values.iter().map(|value| *value as i64).collect::<Vec<_>>(),
    ))
}

fn utf8(values: &[String]) -> ArrayRef {
    Arc::new(StringArray::from(values.to_vec()))
}

fn utf8_opt(values: &[Option<String>]) -> ArrayRef {
    Arc::new(StringArray::from(values.to_vec()))
}

fn f64_opt(values: &[Option<f64>]) -> ArrayRef {
    Arc::new(Float64Array::from(values.to_vec()))
}

fn bool_opt(values: &[Option<bool>]) -> ArrayRef {
    Arc::new(BooleanArray::from(values.to_vec()))
}
