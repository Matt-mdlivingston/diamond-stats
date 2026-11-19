//! In-memory [`StatsApi`](crate::StatsApi) for tests. No sockets.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use diamond_stats_core::api_model::{StatsEnvelope, TeamsEnvelope};

use crate::client::StatsApi;
use crate::error::IngestError;
use crate::urls::{PlayerQuery, SeasonPageQuery};

/// Canned responses keyed the same way the live client queries.
#[derive(Clone, Default)]
pub struct FixtureClient {
    inner: Arc<FixtureState>,
}

#[derive(Default)]
struct FixtureState {
    /// Season pages keyed by `(sport_id, group, offset)`.
    season_pages: Mutex<HashMap<(u32, String, u32), StatsEnvelope>>,
    /// People season payloads keyed by `(player_id, group)`.
    player_seasons: Mutex<HashMap<(u32, String), StatsEnvelope>>,
    /// Game logs keyed by `(player_id, group)`.
    game_logs: Mutex<HashMap<(u32, String), StatsEnvelope>>,
    /// Teams keyed by sport id.
    teams: Mutex<HashMap<u32, TeamsEnvelope>>,
    /// Descriptions of calls, so a test can assert a second read was local.
    calls: Mutex<Vec<String>>,
}

impl FixtureClient {
    /// Empty client. Every call fails until a response is inserted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a season-stats page.
    pub fn insert_season_page(&self, query: SeasonPageQuery, envelope: StatsEnvelope) {
        self.inner
            .season_pages
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(
                (query.sport_id, query.group.to_string(), query.offset),
                envelope,
            );
    }

    /// Record a people season payload.
    pub fn insert_player_season(
        &self,
        player_id: u32,
        query: PlayerQuery,
        envelope: StatsEnvelope,
    ) {
        self.inner
            .player_seasons
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert((player_id, query.group.to_string()), envelope);
    }

    /// Record a game log.
    pub fn insert_game_log(&self, player_id: u32, query: PlayerQuery, envelope: StatsEnvelope) {
        self.inner
            .game_logs
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert((player_id, query.group.to_string()), envelope);
    }

    /// Record a teams payload.
    pub fn insert_teams(&self, sport_id: u32, envelope: TeamsEnvelope) {
        self.inner
            .teams
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(sport_id, envelope);
    }

    /// Calls made so far, in order.
    pub fn calls(&self) -> Vec<String> {
        self.inner
            .calls
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn record(&self, call: String) {
        self.inner
            .calls
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(call);
    }
}

impl StatsApi for FixtureClient {
    async fn season_page(&self, query: SeasonPageQuery) -> Result<StatsEnvelope, IngestError> {
        self.record(format!(
            "season:{}:{}:{}",
            query.sport_id, query.group, query.offset
        ));
        self.inner
            .season_pages
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&(query.sport_id, query.group.to_string(), query.offset))
            .cloned()
            .ok_or_else(|| {
                IngestError::Message(format!(
                    "fixture has no season page sport {} {} offset {}",
                    query.sport_id, query.group, query.offset
                ))
            })
    }

    async fn player_season(
        &self,
        player_id: u32,
        query: PlayerQuery,
    ) -> Result<StatsEnvelope, IngestError> {
        self.record(format!("player-season:{player_id}:{}", query.group));
        self.inner
            .player_seasons
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&(player_id, query.group.to_string()))
            .cloned()
            .ok_or_else(|| {
                IngestError::Message(format!(
                    "fixture has no player season {player_id} {}",
                    query.group
                ))
            })
    }

    async fn game_log(
        &self,
        player_id: u32,
        query: PlayerQuery,
    ) -> Result<StatsEnvelope, IngestError> {
        self.record(format!("gamelog:{player_id}:{}", query.group));
        self.inner
            .game_logs
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&(player_id, query.group.to_string()))
            .cloned()
            .ok_or_else(|| {
                IngestError::Message(format!(
                    "fixture has no game log {player_id} {}",
                    query.group
                ))
            })
    }

    async fn teams(&self, sport_id: u32, season: u16) -> Result<TeamsEnvelope, IngestError> {
        self.record(format!("teams:{sport_id}:{season}"));
        self.inner
            .teams
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&sport_id)
            .cloned()
            .ok_or_else(|| {
                IngestError::Message(format!("fixture has no teams for sport {sport_id}"))
            })
    }
}
