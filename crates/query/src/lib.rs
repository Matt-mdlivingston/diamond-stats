//! Typed queries over the Parquet partitions produced by ingest.
//!
//! DuckDB scans and aggregates. Rate stats are computed in Rust from the summed
//! counting stats, using the same functions the normalizer uses, so AVG/ERA/WHIP
//! have one definition. Filter values are SQL parameters. Stat names are a closed
//! enum and never become SQL text.

#![warn(missing_docs)]

mod error;
mod store;

pub use error::QueryError;
pub use store::{GameLogLoad, LeaderRequest, LeaderRow, PlayerSeasons, Roster, StatsStore};
