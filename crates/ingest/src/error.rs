//! Ingest failures: HTTP, cache, pagination, and Parquet writes.

use diamond_stats_core::CoreError;
use thiserror::Error;

/// An error while fetching, normalizing, or writing a partition.
#[derive(Debug, Error)]
pub enum IngestError {
    /// The HTTP client failed before a status code was available.
    #[error("HTTP error fetching {url}: {source}")]
    Http {
        /// Request URL.
        url: String,
        /// Underlying client error.
        source: reqwest::Error,
    },
    /// The Stats API returned a non-success status that was not retried.
    #[error("stats API returned HTTP {status} for {url}: {body}")]
    Status {
        /// HTTP status.
        status: u16,
        /// Request URL.
        url: String,
        /// A short slice of the body, for logs. Not a stored dataset.
        body: String,
    },
    /// Response bytes were not JSON.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Filesystem failure for the cache or a Parquet file.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Normalization rejected a payload.
    #[error(transparent)]
    Core(#[from] CoreError),
    /// Paging did not collect the number of splits the API reported.
    #[error(
        "pagination for {group} sportId {sport_id} season {season}: API reported {expected} splits, collected {got}"
    )]
    Pagination {
        /// Hitting or pitching.
        group: String,
        /// `sportId` that was requested.
        sport_id: u32,
        /// Season that was requested.
        season: u16,
        /// `totalSplits` from the API.
        expected: u32,
        /// Unique rows collected from `splits` only.
        got: usize,
    },
    /// Arrow failed to build a batch.
    #[error("arrow error: {0}")]
    Arrow(String),
    /// Parquet failed to write a file.
    #[error("parquet error: {0}")]
    Parquet(String),
    /// A catch-all with context.
    #[error("{0}")]
    Message(String),
}
