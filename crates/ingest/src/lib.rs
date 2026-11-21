//! Fetch the MLB Stats API and write normalized Parquet.
//!
//! The live client paces requests, retries transient failures, and caches raw
//! JSON under the data directory. Tests use [`FixtureClient`] instead of the
//! network. Game logs, box scores, and play-by-play are written one player or
//! one game at a time. Season ingest does not crawl them.

#![warn(missing_docs)]

mod cache;
mod client;
mod error;
mod fetch;
mod fixture;
mod games;
mod parquet;
mod parquet_games;
mod pipeline;
mod urls;

pub use client::{ClientConfig, HttpStatsClient, StatsApi};
pub use error::IngestError;
pub use fixture::FixtureClient;
pub use games::{
    DateGamesOptions, DateGamesSummary, GameWriteSummary, ingest_games, materialize_boxscore,
    materialize_game_header, materialize_plays, materialize_schedule,
};
pub use parquet::{
    write_batting, write_batting_gamelog, write_pitching, write_pitching_gamelog, write_teams,
};
pub use parquet_games::{
    write_box_batting, write_box_pitching, write_pitches, write_plays, write_schedule,
};
pub use pipeline::{
    GroupSummary, IngestOptions, IngestSummary, LevelSummary, ingest, materialize_game_log,
};
pub use urls::{PlayerQuery, SeasonPageQuery, USER_AGENT};
