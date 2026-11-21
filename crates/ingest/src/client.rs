//! HTTP client for the MLB Stats API.
//!
//! Requests are cached, paced, and retried. The [`StatsApi`] trait is what the
//! pipeline and the HTTP server depend on, so tests never open a socket.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use diamond_stats_core::api_model::{StatsEnvelope, TeamsEnvelope};
use diamond_stats_core::game_api::{BoxscoreEnvelope, PlayByPlayEnvelope, ScheduleEnvelope};
use serde::de::DeserializeOwned;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::Instant;
use tracing::debug;

use crate::cache;
use crate::error::IngestError;
use crate::urls::{
    PlayerQuery, SeasonPageQuery, USER_AGENT, backoff, boxscore_url, game_log_url,
    play_by_play_url, player_season_url, schedule_date_url, schedule_game_url, season_stats_url,
    should_retry_status, team_url, teams_url,
};

/// Operations the ingest pipeline needs from a stats source.
pub trait StatsApi: Clone + Send + Sync + 'static {
    /// One page of season stats. `playerPool=all` is the caller's responsibility
    /// on the HTTP implementation; the fixture returns whatever it was given.
    fn season_page(
        &self,
        query: SeasonPageQuery,
    ) -> impl Future<Output = Result<StatsEnvelope, IngestError>> + Send;

    /// Season stats for one person, including per-team stints.
    fn player_season(
        &self,
        player_id: u32,
        query: PlayerQuery,
    ) -> impl Future<Output = Result<StatsEnvelope, IngestError>> + Send;

    /// Game log for one person-season.
    fn game_log(
        &self,
        player_id: u32,
        query: PlayerQuery,
    ) -> impl Future<Output = Result<StatsEnvelope, IngestError>> + Send;

    /// Clubs for a sport and season.
    fn teams(
        &self,
        sport_id: u32,
        season: u16,
    ) -> impl Future<Output = Result<TeamsEnvelope, IngestError>> + Send;

    /// One club, used to learn `sportId` from a `gamePk` schedule payload.
    fn team(
        &self,
        team_id: u32,
        season: u16,
    ) -> impl Future<Output = Result<TeamsEnvelope, IngestError>> + Send;

    /// Regular-season schedule for one sport and date.
    fn schedule_date(
        &self,
        sport_id: u32,
        date: String,
    ) -> impl Future<Output = Result<ScheduleEnvelope, IngestError>> + Send;

    /// Schedule row for one `gamePk`.
    fn schedule_game(
        &self,
        game_pk: u64,
    ) -> impl Future<Output = Result<ScheduleEnvelope, IngestError>> + Send;

    /// Box score for one game.
    fn boxscore(
        &self,
        game_pk: u64,
    ) -> impl Future<Output = Result<BoxscoreEnvelope, IngestError>> + Send;

    /// Play-by-play for one game.
    fn play_by_play(
        &self,
        game_pk: u64,
    ) -> impl Future<Output = Result<PlayByPlayEnvelope, IngestError>> + Send;
}

/// Settings for [`HttpStatsClient`].
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// API origin, without a trailing path. Defaults to `https://statsapi.mlb.com`.
    pub base_url: String,
    /// `User-Agent` header.
    pub user_agent: String,
    /// Directory of cached response bodies.
    pub cache_dir: std::path::PathBuf,
    /// Minimum gap between request starts.
    pub min_interval: Duration,
    /// Maximum requests in flight.
    pub max_in_flight: usize,
    /// Extra attempts after the first, for 429 and 5xx.
    pub max_retries: u32,
    /// Per-request timeout.
    pub timeout: Duration,
}

impl ClientConfig {
    /// Defaults aimed at a polite one-shot ingest of a season.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            base_url: "https://statsapi.mlb.com".to_string(),
            user_agent: USER_AGENT.to_string(),
            cache_dir: diamond_stats_core::paths::cache_dir(data_dir),
            min_interval: Duration::from_millis(100),
            max_in_flight: 2,
            max_retries: 3,
            timeout: Duration::from_secs(30),
        }
    }
}

/// Live client. Clones share the pace gate, semaphore, and cache.
#[derive(Clone)]
pub struct HttpStatsClient {
    http: reqwest::Client,
    config: ClientConfig,
    pace: Arc<Pace>,
    semaphore: Arc<Semaphore>,
}

struct Pace {
    min_interval: Duration,
    last_start: Mutex<Option<Instant>>,
}

impl Pace {
    async fn wait(&self) {
        let mut guard = self.last_start.lock().await;
        loop {
            let now = Instant::now();
            if let Some(last) = *guard {
                let earliest = last + self.min_interval;
                if now < earliest {
                    let sleep_for = earliest - now;
                    drop(guard);
                    tokio::time::sleep(sleep_for).await;
                    guard = self.last_start.lock().await;
                    continue;
                }
            }
            *guard = Some(now);
            return;
        }
    }
}

impl HttpStatsClient {
    /// Build a client. Fails if the TLS stack cannot be initialized.
    pub fn new(config: ClientConfig) -> Result<Self, IngestError> {
        let http = reqwest::Client::builder()
            .user_agent(config.user_agent.clone())
            .timeout(config.timeout)
            .gzip(true)
            .build()
            .map_err(|source| IngestError::Http {
                url: config.base_url.clone(),
                source,
            })?;
        let max_in_flight = config.max_in_flight.max(1);
        Ok(Self {
            http,
            pace: Arc::new(Pace {
                min_interval: config.min_interval,
                last_start: Mutex::new(None),
            }),
            semaphore: Arc::new(Semaphore::new(max_in_flight)),
            config,
        })
    }

    async fn get_typed<T: DeserializeOwned>(&self, url: &str) -> Result<T, IngestError> {
        let value = self.get_json(url).await?;
        serde_json::from_value(value).map_err(IngestError::from)
    }

    async fn get_json(&self, url: &str) -> Result<serde_json::Value, IngestError> {
        if let Some(bytes) = cache::read(&self.config.cache_dir, url)? {
            debug!(url, "statsapi cache hit");
            return serde_json::from_slice(&bytes).map_err(IngestError::from);
        }
        debug!(url, "statsapi cache miss");
        let bytes = self.fetch_with_retry(url).await?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        cache::write(&self.config.cache_dir, url, &bytes)?;
        Ok(value)
    }

    async fn fetch_with_retry(&self, url: &str) -> Result<Vec<u8>, IngestError> {
        let mut attempt = 0_u32;
        loop {
            self.pace.wait().await;
            let permit = self
                .semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| IngestError::Message("request semaphore closed".into()))?;
            let response = self.http.get(url).send().await;
            drop(permit);
            match response {
                Ok(resp) if resp.status().is_success() => {
                    return resp
                        .bytes()
                        .await
                        .map(|bytes| bytes.to_vec())
                        .map_err(|source| IngestError::Http {
                            url: url.to_string(),
                            source,
                        });
                }
                Ok(resp)
                    if should_retry_status(resp.status().as_u16())
                        && attempt < self.config.max_retries =>
                {
                    let status = resp.status().as_u16();
                    let delay = retry_after(&resp).unwrap_or_else(|| backoff(attempt));
                    debug!(url, status, ?delay, attempt, "retrying stats API request");
                    attempt += 1;
                    tokio::time::sleep(delay).await;
                }
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let body = resp.text().await.unwrap_or_default();
                    let body: String = body.chars().take(300).collect();
                    return Err(IngestError::Status {
                        status,
                        url: url.to_string(),
                        body,
                    });
                }
                Err(source)
                    if attempt < self.config.max_retries
                        && (source.is_timeout() || source.is_connect() || source.is_request()) =>
                {
                    let delay = backoff(attempt);
                    debug!(url, error = %source, ?delay, attempt, "retrying stats API transport error");
                    attempt += 1;
                    tokio::time::sleep(delay).await;
                }
                Err(source) => {
                    return Err(IngestError::Http {
                        url: url.to_string(),
                        source,
                    });
                }
            }
        }
    }
}

fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let header = response.headers().get(reqwest::header::RETRY_AFTER)?;
    let text = header.to_str().ok()?;
    let seconds: u64 = text.parse().ok()?;
    Some(Duration::from_secs(seconds.min(30)))
}

impl StatsApi for HttpStatsClient {
    async fn season_page(&self, query: SeasonPageQuery) -> Result<StatsEnvelope, IngestError> {
        let url = season_stats_url(&self.config.base_url, query);
        let value = self.get_json(&url).await?;
        serde_json::from_value(value).map_err(IngestError::from)
    }

    async fn player_season(
        &self,
        player_id: u32,
        query: PlayerQuery,
    ) -> Result<StatsEnvelope, IngestError> {
        let url = player_season_url(&self.config.base_url, player_id, query);
        let value = self.get_json(&url).await?;
        serde_json::from_value(value).map_err(IngestError::from)
    }

    async fn game_log(
        &self,
        player_id: u32,
        query: PlayerQuery,
    ) -> Result<StatsEnvelope, IngestError> {
        let url = game_log_url(&self.config.base_url, player_id, query);
        let value = self.get_json(&url).await?;
        serde_json::from_value(value).map_err(IngestError::from)
    }

    async fn teams(&self, sport_id: u32, season: u16) -> Result<TeamsEnvelope, IngestError> {
        let url = teams_url(&self.config.base_url, sport_id, season);
        self.get_typed(&url).await
    }

    async fn team(&self, team_id: u32, season: u16) -> Result<TeamsEnvelope, IngestError> {
        let url = team_url(&self.config.base_url, team_id, season);
        self.get_typed(&url).await
    }

    async fn schedule_date(
        &self,
        sport_id: u32,
        date: String,
    ) -> Result<ScheduleEnvelope, IngestError> {
        let url = schedule_date_url(&self.config.base_url, sport_id, &date);
        self.get_typed(&url).await
    }

    async fn schedule_game(&self, game_pk: u64) -> Result<ScheduleEnvelope, IngestError> {
        let url = schedule_game_url(&self.config.base_url, game_pk);
        self.get_typed(&url).await
    }

    async fn boxscore(&self, game_pk: u64) -> Result<BoxscoreEnvelope, IngestError> {
        let url = boxscore_url(&self.config.base_url, game_pk);
        self.get_typed(&url).await
    }

    async fn play_by_play(&self, game_pk: u64) -> Result<PlayByPlayEnvelope, IngestError> {
        let url = play_by_play_url(&self.config.base_url, game_pk);
        self.get_typed(&url).await
    }
}
