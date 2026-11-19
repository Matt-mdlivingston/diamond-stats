//! In-memory [`StatsApi`](crate::StatsApi) for tests. No sockets.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use diamond_stats_core::api_model::{StatsEnvelope, TeamsEnvelope};
use diamond_stats_core::game_api::{BoxscoreEnvelope, PlayByPlayEnvelope, ScheduleEnvelope};

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
    /// One club keyed by `(team_id, season)`.
    team_by_id: Mutex<HashMap<(u32, u16), TeamsEnvelope>>,
    /// Schedule for a sport and date.
    schedule_dates: Mutex<HashMap<(u32, String), ScheduleEnvelope>>,
    /// Schedule for one gamePk.
    schedule_games: Mutex<HashMap<u64, ScheduleEnvelope>>,
    /// Box scores keyed by gamePk.
    boxscores: Mutex<HashMap<u64, BoxscoreEnvelope>>,
    /// Play-by-play keyed by gamePk.
    plays: Mutex<HashMap<u64, PlayByPlayEnvelope>>,
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

    /// Record one club, for a gamePk lookup that needs `sportId`.
    pub fn insert_team(&self, team_id: u32, season: u16, envelope: TeamsEnvelope) {
        self.inner
            .team_by_id
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert((team_id, season), envelope);
    }

    /// Record a schedule date.
    pub fn insert_schedule_date(&self, sport_id: u32, date: &str, envelope: ScheduleEnvelope) {
        self.inner
            .schedule_dates
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert((sport_id, date.to_string()), envelope);
    }

    /// Record `schedule?gamePk=`.
    pub fn insert_schedule_game(&self, game_pk: u64, envelope: ScheduleEnvelope) {
        self.inner
            .schedule_games
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(game_pk, envelope);
    }

    /// Record a box score.
    pub fn insert_boxscore(&self, game_pk: u64, envelope: BoxscoreEnvelope) {
        self.inner
            .boxscores
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(game_pk, envelope);
    }

    /// Record play-by-play.
    pub fn insert_plays(&self, game_pk: u64, envelope: PlayByPlayEnvelope) {
        self.inner
            .plays
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(game_pk, envelope);
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

    async fn team(&self, team_id: u32, season: u16) -> Result<TeamsEnvelope, IngestError> {
        self.record(format!("team:{team_id}:{season}"));
        self.inner
            .team_by_id
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&(team_id, season))
            .cloned()
            .ok_or_else(|| {
                IngestError::Message(format!("fixture has no team {team_id} season {season}"))
            })
    }

    async fn schedule_date(
        &self,
        sport_id: u32,
        date: String,
    ) -> Result<ScheduleEnvelope, IngestError> {
        self.record(format!("schedule-date:{sport_id}:{date}"));
        self.inner
            .schedule_dates
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&(sport_id, date.clone()))
            .cloned()
            .ok_or_else(|| {
                IngestError::Message(format!(
                    "fixture has no schedule for sport {sport_id} {date}"
                ))
            })
    }

    async fn schedule_game(&self, game_pk: u64) -> Result<ScheduleEnvelope, IngestError> {
        self.record(format!("schedule-game:{game_pk}"));
        self.inner
            .schedule_games
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&game_pk)
            .cloned()
            .ok_or_else(|| IngestError::Message(format!("fixture has no schedule game {game_pk}")))
    }

    async fn boxscore(&self, game_pk: u64) -> Result<BoxscoreEnvelope, IngestError> {
        self.record(format!("boxscore:{game_pk}"));
        self.inner
            .boxscores
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&game_pk)
            .cloned()
            .ok_or_else(|| IngestError::Message(format!("fixture has no box score {game_pk}")))
    }

    async fn play_by_play(&self, game_pk: u64) -> Result<PlayByPlayEnvelope, IngestError> {
        self.record(format!("plays:{game_pk}"));
        self.inner
            .plays
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&game_pk)
            .cloned()
            .ok_or_else(|| IngestError::Message(format!("fixture has no play-by-play {game_pk}")))
    }
}
