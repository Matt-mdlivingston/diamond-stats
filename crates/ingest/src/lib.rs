//! Fetch the MLB Stats API and write normalized Parquet.
//!
//! The live client paces requests, retries transient failures, and caches raw
//! JSON under the data directory. Tests use [`FixtureClient`] instead of the
//! network. Game logs are written one player at a time by
//! [`materialize_game_log`]; season ingest does not crawl them.

#![warn(missing_docs)]

mod cache;
mod client;
mod error;
mod fetch;
mod fixture;
mod parquet;
mod pipeline;
mod urls;

pub use client::{ClientConfig, HttpStatsClient, StatsApi};
pub use error::IngestError;
pub use fixture::FixtureClient;
pub use parquet::{
    write_batting, write_batting_gamelog, write_pitching, write_pitching_gamelog, write_teams,
};
pub use pipeline::{
    GroupSummary, IngestOptions, IngestSummary, LevelSummary, ingest, materialize_game_log,
};
pub use urls::{PlayerQuery, SeasonPageQuery, USER_AGENT};
