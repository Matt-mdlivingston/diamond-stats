//! Query errors. SQL values are bound parameters; these errors are about
//! missing files or the database, not about interpolating a request string.

use diamond_stats_core::{Level, StatGroup};
use thiserror::Error;

/// A failure while reading Parquet through DuckDB.
#[derive(Debug, Error)]
pub enum QueryError {
    /// DuckDB rejected the query or a cell conversion.
    #[error("duckdb error: {0}")]
    DuckDb(#[from] duckdb::Error),
    /// The season/level partition has not been ingested.
    #[error(
        "no {dataset} rows are loaded for {season} {level}. Run `diamond-stats ingest --season {season} --level {level}`"
    )]
    DataNotLoaded {
        /// Dataset directory name.
        dataset: &'static str,
        /// Requested season.
        season: u16,
        /// Requested level.
        level: Level,
    },
    /// No Parquet files exist for this dataset at all.
    #[error("no {dataset} parquet files have been written")]
    DatasetMissing {
        /// Dataset directory name.
        dataset: &'static str,
    },
    /// The player's game log has not been fetched yet.
    #[error("game log for player {player_id} {season} {level} {group} is not materialized")]
    GameLogMissing {
        /// MLB person id.
        player_id: u32,
        /// Season.
        season: u16,
        /// Level.
        level: Level,
        /// Hitting or pitching.
        group: StatGroup,
    },
    /// The data directory path cannot be safely inlined into SQL.
    #[error("data path contains characters that cannot be inlined into SQL")]
    UnsafePath,
    /// A limit or a cell was outside the range this layer accepts.
    #[error("{0}")]
    Message(String),
}
