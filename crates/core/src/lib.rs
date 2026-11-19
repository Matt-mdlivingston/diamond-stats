//! Domain model for diamond-stats.
//!
//! API JSON is parsed into [`api_model`] and normalized into [`domain`] rows.
//! Rate stats are functions of counting stats in [`rates`]. The MLB Stats API
//! remains the system of record for the numbers; this crate does not invent them.

#![warn(missing_docs)]

pub mod api_model;
pub mod domain;
pub mod error;
pub mod group;
pub mod level;
pub mod normalize;
pub mod paths;
pub mod rates;

pub use domain::{
    ATTRIBUTION_NOTE, BattingStint, GameLogEntry, LeaderStat, MLBAM_TERMS_URL, PitchingStint, Team,
};
pub use error::CoreError;
pub use group::StatGroup;
pub use level::{Level, LevelInfo};
pub use normalize::{NormalizeCtx, SplitDisposition};
pub use rates::Innings;
