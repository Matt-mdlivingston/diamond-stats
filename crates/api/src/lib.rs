//! HTTP API. Reads local Parquet and, for a game log that has not been
//! fetched yet, asks the Stats API for that one player and caches the file.
//!
//! The listener defaults to loopback. This process is not a bulk download
//! service: leaderboards are capped and there is no endpoint that dumps a season.

#![warn(missing_docs)]

mod error;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use diamond_stats_core::{
    ATTRIBUTION_NOTE, BattingStint, BoxBattingLine, BoxPitchingLine, GameLogEntry, LeaderStat,
    Level, MLBAM_TERMS_URL, Pitch, PitchTracking, PitchingStint, PlateAppearance, ScheduledGame,
    StatGroup, Team, parse_iso_date, pitch_tracking_note,
};
use diamond_stats_ingest::{
    ClientConfig, HttpStatsClient, IngestError, StatsApi, materialize_boxscore,
    materialize_game_header, materialize_game_log, materialize_plays, materialize_schedule,
};
use diamond_stats_query::{
    GameLogLoad, LeaderRequest, LeaderRow, MatchupRequest, QueryError, ScheduleLoad, SeasonFilter,
    StatsStore,
};
use error::ApiError;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tower_http::trace::TraceLayer;
use utoipa::{IntoParams, OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

pub use error::{ErrorBody, ErrorDetail};

/// Shared server state. `T` is the stats source used for lazy game logs.
#[derive(Clone)]
pub struct AppState<T> {
    /// Parquet and cache root.
    pub data_dir: PathBuf,
    /// Stats source. The live server uses [`HttpStatsClient`].
    pub api: T,
    /// Serializes lazy game-log fetches so two requests do not write one file.
    pub materialize: Arc<Mutex<()>>,
}

/// Attribution carried on data responses.
#[derive(Debug, Serialize, ToSchema)]
pub struct Attribution {
    /// Data provider.
    pub provider: &'static str,
    /// MLBAM terms.
    pub terms: &'static str,
    /// Use restriction.
    pub note: &'static str,
}

fn attribution() -> Attribution {
    Attribution {
        provider: "MLB Stats API",
        terms: MLBAM_TERMS_URL,
        note: ATTRIBUTION_NOTE,
    }
}

/// `GET /health`.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// Always `ok` when the process is serving.
    pub status: &'static str,
}

/// One level in `GET /v1/levels`.
#[derive(Debug, Serialize, ToSchema)]
pub struct LevelJson {
    /// Slug used in query parameters.
    pub slug: String,
    /// Display name.
    pub name: String,
    /// MLB Stats API `sportId`.
    pub sport_id: u32,
    /// `code` from `GET /api/v1/sports`.
    pub api_code: String,
}

/// `GET /v1/levels`.
#[derive(Debug, Serialize, ToSchema)]
pub struct LevelsResponse {
    /// Known levels. sportId 15 and 21 are omitted on purpose.
    pub levels: Vec<LevelJson>,
}

/// `GET /v1/teams`.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamsResponse {
    /// Season.
    pub season: u16,
    /// Level slug.
    pub level: Level,
    /// Clubs.
    pub teams: Vec<Team>,
    /// Provenance.
    pub attribution: Attribution,
}

/// `GET /v1/teams/{id}/roster`.
#[derive(Debug, Serialize, ToSchema)]
pub struct RosterResponse {
    /// Team id.
    pub team_id: u32,
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Club name, when the teams partition has it.
    pub team_name: Option<String>,
    /// Batting stints for this club only. A traded player's other stint is absent.
    pub batting: Vec<BattingStint>,
    /// Pitching stints for this club only.
    pub pitching: Vec<PitchingStint>,
    /// Provenance.
    pub attribution: Attribution,
}

/// `GET /v1/players/{id}/seasons`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlayerSeasonsResponse {
    /// MLB person id.
    pub player_id: u32,
    /// Display name.
    pub player_name: Option<String>,
    /// Batting stints at every ingested level.
    pub batting: Vec<BattingStint>,
    /// Pitching stints at every ingested level.
    pub pitching: Vec<PitchingStint>,
    /// Provenance.
    pub attribution: Attribution,
}

/// `GET /v1/players/{id}/gamelog`.
#[derive(Debug, Serialize, ToSchema)]
pub struct GameLogResponse {
    /// MLB person id.
    pub player_id: u32,
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Batting games. Empty when `group=pitching`.
    pub hitting: Vec<GameLogEntry>,
    /// Pitching games. Empty when `group=hitting`.
    pub pitching: Vec<GameLogEntry>,
    /// Provenance.
    pub attribution: Attribution,
}

/// One leaderboard row.
#[derive(Debug, Serialize, ToSchema)]
pub struct LeaderJson {
    /// 1-based rank.
    pub rank: u32,
    /// MLB person id.
    pub player_id: u32,
    /// Display name.
    pub player_name: String,
    /// Teams combined into this row.
    pub team_names: String,
    /// Sort value. For `ip` this is true innings (outs / 3), not `187.2` notation.
    pub value: f64,
    /// Plate appearances. Zero on pitching boards.
    pub plate_appearances: u32,
    /// Outs. Zero on batting boards.
    pub outs: u32,
    /// Baseball innings notation on pitching boards.
    pub innings_pitched: Option<String>,
}

/// `GET /v1/leaders`.
#[derive(Debug, Serialize, ToSchema)]
pub struct LeadersResponse {
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Stat group.
    pub group: StatGroup,
    /// Stat slug.
    pub stat: String,
    /// `asc` for ERA and WHIP, otherwise `desc`.
    pub order: String,
    /// Plate-appearance floor applied to a batting board.
    pub min_plate_appearances: Option<u32>,
    /// True-innings floor applied to a pitching board. Not baseball notation.
    pub min_innings: Option<f64>,
    /// Rows.
    pub leaders: Vec<LeaderJson>,
    /// Provenance.
    pub attribution: Attribution,
}

/// Query string for leaders.
#[derive(Debug, Deserialize, IntoParams)]
pub struct LeadersParams {
    /// Season year.
    pub season: u16,
    /// Level slug (`mlb`, `aaa`, ...).
    pub level: String,
    /// Stat slug (`hr`, `avg`, `era`, `so`, ...).
    pub stat: String,
    /// `hitting` or `pitching`.
    pub group: String,
    /// 1..=50. Defaults to 10.
    pub limit: Option<u32>,
    /// Override the batting sample floor. Rate stats default to 502 plate appearances.
    pub min_pa: Option<u32>,
    /// Override the pitching sample floor, in true innings. Rate stats default to 162.
    pub min_ip: Option<f64>,
}

/// Query string for a game log.
#[derive(Debug, Deserialize, IntoParams)]
pub struct GameLogParams {
    /// Season year.
    pub season: u16,
    /// Level slug.
    pub level: String,
    /// Optional `hitting` or `pitching`. Omit for both.
    pub group: Option<String>,
}

/// Query string for teams and rosters.
#[derive(Debug, Deserialize, IntoParams)]
pub struct SeasonLevelParams {
    /// Season year.
    pub season: u16,
    /// Level slug.
    pub level: String,
}

/// Optional filters for `GET /v1/players/{id}/seasons`.
///
/// Leaving `level` unset returns every ingested level. That is the cross-level
/// view of a season when `season` is set, and the career view when both are unset.
#[derive(Debug, Deserialize, IntoParams)]
pub struct PlayerSeasonParams {
    /// Season year. Omit for every ingested season.
    pub season: Option<u16>,
    /// Level slug. Omit for every ingested level.
    pub level: Option<String>,
}

/// Query string for a schedule date.
#[derive(Debug, Deserialize, IntoParams)]
pub struct GamesParams {
    /// Season year.
    pub season: u16,
    /// Level slug (`mlb`, `aaa`, `aa`, `high_a`, `single_a`, `rookie`).
    pub level: String,
    /// Schedule date, `YYYY-MM-DD`.
    pub date: String,
}

/// `GET /v1/games`.
#[derive(Debug, Serialize, ToSchema)]
pub struct GamesResponse {
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Schedule date that was requested.
    pub date: String,
    /// Games returned for that date, including postponements.
    pub games: Vec<ScheduledGame>,
    /// Provenance.
    pub attribution: Attribution,
}

/// `GET /v1/games/{game_pk}`.
#[derive(Debug, Serialize, ToSchema)]
pub struct GameResponse {
    /// Schedule row.
    pub game: ScheduledGame,
    /// False when the game is not an official final, so no box score was stored.
    pub boxscore_loaded: bool,
    /// Postponement or reschedule note, when there is one.
    pub note: Option<String>,
    /// Batting lines. Empty until the box score is loaded.
    pub batting: Vec<BoxBattingLine>,
    /// Pitching lines.
    pub pitching: Vec<BoxPitchingLine>,
    /// Provenance.
    pub attribution: Attribution,
}

/// One pitch in a play-by-play response.
#[derive(Debug, Serialize, ToSchema)]
pub struct PitchJson {
    /// Pitch number in the plate appearance, starting at 1.
    pub pitch_number: u32,
    /// Umpire call code.
    pub call_code: Option<String>,
    /// Umpire call description.
    pub call_description: Option<String>,
    /// Pitch-type code, when the feed sent one.
    pub pitch_type_code: Option<String>,
    /// Pitch-type name, when the feed sent one.
    pub pitch_type: Option<String>,
    /// Release speed in mph, when the feed sent one.
    pub start_speed: Option<f64>,
    /// The call was a ball.
    pub is_ball: bool,
    /// The call was a strike.
    pub is_strike: bool,
    /// The ball was put in play.
    pub is_in_play: bool,
}

/// One plate appearance, with the pitches the feed included.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlateAppearanceJson {
    /// Zero-based index.
    pub at_bat_index: u32,
    /// Inning.
    pub inning: u32,
    /// `top` or `bottom`.
    pub half_inning: String,
    /// Batter id.
    pub batter_id: u32,
    /// Batter name.
    pub batter_name: String,
    /// Pitcher id.
    pub pitcher_id: u32,
    /// Pitcher name.
    pub pitcher_name: String,
    /// Batter hand.
    pub bat_side: Option<String>,
    /// Pitcher hand.
    pub pitch_hand: Option<String>,
    /// Result label.
    pub event: Option<String>,
    /// Result code.
    pub event_type: Option<String>,
    /// Prose description.
    pub description: Option<String>,
    /// RBI.
    pub rbi: u32,
    /// Whether the play scored a run.
    pub is_scoring_play: bool,
    /// Away score after the play.
    pub away_score: u32,
    /// Home score after the play.
    pub home_score: u32,
    /// Pitches. Empty when the feed had no pitch events for this plate appearance.
    pub pitches: Vec<PitchJson>,
}

/// `GET /v1/games/{game_pk}/plays`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlaysResponse {
    /// `gamePk`.
    pub game_pk: u64,
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Schedule date.
    pub schedule_date: String,
    /// Official date. May differ when the game was rescheduled.
    pub official_date: String,
    /// Plate appearances.
    pub plate_appearances: Vec<PlateAppearanceJson>,
    /// Counts of what the feed actually contained.
    pub pitch_tracking: PitchTracking,
    /// A sentence describing the pitch-tracking gap, if any.
    pub pitch_tracking_note: String,
    /// Provenance.
    pub attribution: Attribution,
}

/// Query string for a batter-vs-pitcher list.
#[derive(Debug, Deserialize, IntoParams)]
pub struct MatchupParams {
    /// Season year.
    pub season: u16,
    /// Level slug.
    pub level: String,
    /// Batter id. At least one of `batter_id` or `pitcher_id` is required.
    pub batter_id: Option<u32>,
    /// Pitcher id.
    pub pitcher_id: Option<u32>,
    /// 1..=200. Defaults to 50.
    pub limit: Option<u32>,
}

/// `GET /v1/matchups`.
#[derive(Debug, Serialize, ToSchema)]
pub struct MatchupResponse {
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Batter filter, when set.
    pub batter_id: Option<u32>,
    /// Pitcher filter, when set.
    pub pitcher_id: Option<u32>,
    /// Stored plate appearances. This does not download games that are not on disk.
    pub plate_appearances: Vec<PlateAppearance>,
    /// Scope of the list.
    pub note: &'static str,
    /// Provenance.
    pub attribution: Attribution,
}

/// Build the router, including Swagger UI.
pub fn router<T: StatsApi>(state: AppState<T>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/levels", get(levels))
        .route("/v1/teams", get(list_teams))
        .route("/v1/teams/{team_id}/roster", get(roster))
        .route("/v1/players/{player_id}/seasons", get(player_seasons))
        .route("/v1/players/{player_id}/gamelog", get(player_gamelog))
        .route("/v1/leaders", get(leaders))
        .route("/v1/games", get(games_on_date))
        .route("/v1/games/{game_pk}", get(game_detail))
        .route("/v1/games/{game_pk}/plays", get(game_plays))
        .route("/v1/matchups", get(matchups))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Bind `addr` and serve until the process is stopped.
pub async fn serve(addr: SocketAddr, data_dir: PathBuf) -> Result<(), IngestError> {
    let api = HttpStatsClient::new(ClientConfig::new(&data_dir))?;
    let app = router(AppState {
        data_dir,
        api,
        materialize: Arc::new(Mutex::new(())),
    });
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|err| IngestError::Message(format!("bind {addr}: {err}")))?;
    tracing::info!(%addr, "diamond-stats listening");
    axum::serve(listener, app)
        .await
        .map_err(|err| IngestError::Message(err.to_string()))
}

#[utoipa::path(
    get,
    path = "/health",
    tag = "meta",
    responses((status = 200, description = "Process is up", body = HealthResponse))
)]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[utoipa::path(
    get,
    path = "/v1/levels",
    tag = "meta",
    responses((status = 200, description = "Levels diamond-stats can name", body = LevelsResponse))
)]
async fn levels() -> Json<LevelsResponse> {
    Json(LevelsResponse {
        levels: Level::ALL
            .into_iter()
            .map(|level| {
                let info = level.info();
                LevelJson {
                    slug: info.slug.to_string(),
                    name: info.name.to_string(),
                    sport_id: info.sport_id,
                    api_code: info.api_code.to_string(),
                }
            })
            .collect(),
    })
}

#[utoipa::path(
    get,
    path = "/v1/teams",
    tag = "teams",
    params(SeasonLevelParams),
    responses(
        (status = 200, description = "Clubs for a season and level", body = TeamsResponse),
        (status = 400, description = "Bad query", body = ErrorBody),
        (status = 503, description = "Season not ingested", body = ErrorBody)
    )
)]
async fn list_teams<T: StatsApi>(
    State(state): State<AppState<T>>,
    query: Result<Query<SeasonLevelParams>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<TeamsResponse>, ApiError> {
    let Query(query) = query.map_err(|err| ApiError::bad_request(err.to_string()))?;
    let level = parse_level(&query.level)?;
    let store = StatsStore::new(state.data_dir.clone());
    let teams = blocking(move || store.teams(query.season, level)).await?;
    Ok(Json(TeamsResponse {
        season: query.season,
        level,
        teams,
        attribution: attribution(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/teams/{team_id}/roster",
    tag = "teams",
    params(
        ("team_id" = u32, Path, description = "MLB team id"),
        SeasonLevelParams
    ),
    responses(
        (status = 200, description = "Stints for this club", body = RosterResponse),
        (status = 404, description = "Team not in the ingested season", body = ErrorBody),
        (status = 503, description = "Season not ingested", body = ErrorBody)
    )
)]
async fn roster<T: StatsApi>(
    State(state): State<AppState<T>>,
    Path(team_id): Path<String>,
    query: Result<Query<SeasonLevelParams>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<RosterResponse>, ApiError> {
    let team_id = parse_u32(&team_id, "team id")?;
    let Query(query) = query.map_err(|err| ApiError::bad_request(err.to_string()))?;
    let level = parse_level(&query.level)?;
    let store = StatsStore::new(state.data_dir.clone());
    let roster = blocking(move || store.team_roster(team_id, query.season, level)).await?;
    if roster.team_name.is_none() && roster.batting.is_empty() && roster.pitching.is_empty() {
        return Err(ApiError::not_found(format!(
            "team {team_id} is not in {level} {}",
            query.season
        )));
    }
    Ok(Json(RosterResponse {
        team_id: roster.team_id,
        season: roster.season,
        level: roster.level,
        team_name: roster.team_name,
        batting: roster.batting,
        pitching: roster.pitching,
        attribution: attribution(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/players/{player_id}/seasons",
    tag = "players",
    params(
        ("player_id" = u32, Path, description = "MLB person id"),
        PlayerSeasonParams
    ),
    responses(
        (status = 200, description = "Stints across ingested levels. level is optional; omit it for every level in the season.", body = PlayerSeasonsResponse),
        (status = 404, description = "Player has no ingested stints", body = ErrorBody),
        (status = 503, description = "Nothing has been ingested", body = ErrorBody)
    )
)]
async fn player_seasons<T: StatsApi>(
    State(state): State<AppState<T>>,
    Path(player_id): Path<String>,
    query: Result<Query<PlayerSeasonParams>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<PlayerSeasonsResponse>, ApiError> {
    let player_id = parse_u32(&player_id, "player id")?;
    let Query(query) = query.map_err(|err| ApiError::bad_request(err.to_string()))?;
    let level = match query.level.as_deref() {
        Some(raw) => Some(parse_level(raw)?),
        None => None,
    };
    let filter = SeasonFilter {
        season: query.season,
        level,
    };
    let store = StatsStore::new(state.data_dir.clone());
    let seasons = blocking(move || store.player_seasons_filtered(player_id, filter)).await?;
    if seasons.is_empty() {
        return Err(ApiError::not_found(format!(
            "no ingested stints for player {player_id}"
        )));
    }
    Ok(Json(PlayerSeasonsResponse {
        player_id: seasons.player_id,
        player_name: seasons.player_name,
        batting: seasons.batting,
        pitching: seasons.pitching,
        attribution: attribution(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/players/{player_id}/gamelog",
    tag = "players",
    params(
        ("player_id" = u32, Path, description = "MLB person id"),
        GameLogParams
    ),
    responses(
        (status = 200, description = "Per-game counting stats", body = GameLogResponse),
        (status = 502, description = "Stats API request failed", body = ErrorBody)
    )
)]
async fn player_gamelog<T: StatsApi>(
    State(state): State<AppState<T>>,
    Path(player_id): Path<String>,
    query: Result<Query<GameLogParams>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<GameLogResponse>, ApiError> {
    let player_id = parse_u32(&player_id, "player id")?;
    let Query(query) = query.map_err(|err| ApiError::bad_request(err.to_string()))?;
    let level = parse_level(&query.level)?;
    let groups = match query.group.as_deref() {
        None => vec![StatGroup::Hitting, StatGroup::Pitching],
        Some(raw) => {
            vec![StatGroup::parse(raw).map_err(|err| ApiError::bad_request(err.to_string()))?]
        }
    };
    let mut hitting = Vec::new();
    let mut pitching = Vec::new();
    for group in groups {
        let games = load_gamelog(&state, player_id, query.season, level, group).await?;
        match group {
            StatGroup::Hitting => hitting = games,
            StatGroup::Pitching => pitching = games,
        }
    }
    Ok(Json(GameLogResponse {
        player_id,
        season: query.season,
        level,
        hitting,
        pitching,
        attribution: attribution(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/leaders",
    tag = "leaders",
    params(LeadersParams),
    responses(
        (status = 200, description = "Top players for one stat", body = LeadersResponse),
        (status = 400, description = "Unknown stat, level, or group", body = ErrorBody),
        (status = 503, description = "Season not ingested", body = ErrorBody)
    )
)]
async fn leaders<T: StatsApi>(
    State(state): State<AppState<T>>,
    query: Result<Query<LeadersParams>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<LeadersResponse>, ApiError> {
    let Query(query) = query.map_err(|err| ApiError::bad_request(err.to_string()))?;
    let level = parse_level(&query.level)?;
    let group =
        StatGroup::parse(&query.group).map_err(|err| ApiError::bad_request(err.to_string()))?;
    let stat = LeaderStat::parse(group, &query.stat).ok_or_else(|| {
        ApiError::bad_request(format!(
            "stat '{}' is not a {} leaderboard stat",
            query.stat, group
        ))
    })?;
    if stat.group() != group {
        return Err(ApiError::bad_request(format!(
            "stat '{}' belongs to {}, not {group}",
            stat.as_str(),
            stat.group()
        )));
    }
    let limit = query.limit.unwrap_or(10);
    if !(1..=50).contains(&limit) {
        return Err(ApiError::bad_request("limit must be between 1 and 50"));
    }
    let (min_pa, min_outs, reported_pa, reported_ip) = thresholds(stat, query.min_pa, query.min_ip);
    let request = LeaderRequest {
        season: query.season,
        level,
        stat,
        limit,
        min_pa,
        min_outs,
    };
    let store = StatsStore::new(state.data_dir.clone());
    let rows = blocking(move || store.leaders(request)).await?;
    Ok(Json(LeadersResponse {
        season: query.season,
        level,
        group,
        stat: stat.as_str().to_string(),
        order: if stat.lower_is_better() {
            "asc"
        } else {
            "desc"
        }
        .to_string(),
        min_plate_appearances: reported_pa,
        min_innings: reported_ip,
        leaders: rows.into_iter().map(leader_json).collect(),
        attribution: attribution(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/games",
    tag = "games",
    params(GamesParams),
    responses(
        (status = 200, description = "Games on a schedule date", body = GamesResponse),
        (status = 400, description = "Bad date or level", body = ErrorBody),
        (status = 502, description = "Stats API request failed", body = ErrorBody)
    )
)]
async fn games_on_date<T: StatsApi>(
    State(state): State<AppState<T>>,
    query: Result<Query<GamesParams>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<GamesResponse>, ApiError> {
    let Query(query) = query.map_err(|err| ApiError::bad_request(err.to_string()))?;
    let level = parse_level(&query.level)?;
    let date = parse_iso_date(&query.date).map_err(|err| ApiError::bad_request(err.to_string()))?;
    let games = load_schedule(&state, query.season, level, &date).await?;
    Ok(Json(GamesResponse {
        season: query.season,
        level,
        date,
        games,
        attribution: attribution(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/games/{game_pk}",
    tag = "games",
    params(("game_pk" = u64, Path, description = "Stats API gamePk")),
    responses(
        (status = 200, description = "Game and, when final, its box score", body = GameResponse),
        (status = 404, description = "Unknown gamePk", body = ErrorBody),
        (status = 502, description = "Stats API request failed", body = ErrorBody)
    )
)]
async fn game_detail<T: StatsApi>(
    State(state): State<AppState<T>>,
    Path(game_pk): Path<String>,
) -> Result<Json<GameResponse>, ApiError> {
    let game_pk = parse_u64(&game_pk, "game pk")?;
    let game = load_game(&state, game_pk).await?;
    let (batting, pitching, loaded) = if game.boxscore_ready() {
        let lines = load_box(&state, &game).await?;
        (lines.batting, lines.pitching, true)
    } else {
        (Vec::new(), Vec::new(), false)
    };
    Ok(Json(GameResponse {
        note: game_note(&game, loaded),
        game,
        boxscore_loaded: loaded,
        batting,
        pitching,
        attribution: attribution(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/games/{game_pk}/plays",
    tag = "games",
    params(("game_pk" = u64, Path, description = "Stats API gamePk")),
    responses(
        (status = 200, description = "Plate appearances and pitches for one game", body = PlaysResponse),
        (status = 400, description = "Game is not final", body = ErrorBody),
        (status = 404, description = "Unknown gamePk", body = ErrorBody),
        (status = 502, description = "Stats API request failed", body = ErrorBody)
    )
)]
async fn game_plays<T: StatsApi>(
    State(state): State<AppState<T>>,
    Path(game_pk): Path<String>,
) -> Result<Json<PlaysResponse>, ApiError> {
    let game_pk = parse_u64(&game_pk, "game pk")?;
    let game = load_game(&state, game_pk).await?;
    if !game.boxscore_ready() {
        return Err(ApiError::bad_request(format!(
            "game {} is {}; play-by-play is stored for final games",
            game.game_pk, game.detailed_state
        )));
    }
    let feed = load_plays(&state, &game).await?;
    let tracking = PitchTracking::from_rows(&feed.plate_appearances, &feed.pitches);
    let plate_appearances = nest_plays(&feed.plate_appearances, &feed.pitches);
    Ok(Json(PlaysResponse {
        game_pk: game.game_pk,
        season: game.season,
        level: game.level,
        schedule_date: game.schedule_date,
        official_date: game.official_date,
        plate_appearances,
        pitch_tracking_note: pitch_tracking_note(&tracking).to_string(),
        pitch_tracking: tracking,
        attribution: attribution(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/matchups",
    tag = "games",
    params(MatchupParams),
    responses(
        (status = 200, description = "Stored plate appearances for a batter, a pitcher, or both", body = MatchupResponse),
        (status = 400, description = "Missing batter and pitcher, or a bad limit", body = ErrorBody),
        (status = 503, description = "No play-by-play has been ingested", body = ErrorBody)
    )
)]
async fn matchups<T: StatsApi>(
    State(state): State<AppState<T>>,
    query: Result<Query<MatchupParams>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<MatchupResponse>, ApiError> {
    let Query(query) = query.map_err(|err| ApiError::bad_request(err.to_string()))?;
    if query.batter_id.is_none() && query.pitcher_id.is_none() {
        return Err(ApiError::bad_request(
            "matchup requires batter_id or pitcher_id",
        ));
    }
    let level = parse_level(&query.level)?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=200).contains(&limit) {
        return Err(ApiError::bad_request("limit must be between 1 and 200"));
    }
    let request = MatchupRequest {
        season: query.season,
        level,
        batter_id: query.batter_id,
        pitcher_id: query.pitcher_id,
        limit,
    };
    let store = StatsStore::new(state.data_dir.clone());
    let plate_appearances = blocking(move || store.matchups(request)).await?;
    Ok(Json(MatchupResponse {
        season: query.season,
        level,
        batter_id: query.batter_id,
        pitcher_id: query.pitcher_id,
        plate_appearances,
        note: "Only games whose play-by-play has been ingested are included.",
        attribution: attribution(),
    }))
}

fn game_note(game: &ScheduledGame, box_loaded: bool) -> Option<String> {
    let mut parts = Vec::new();
    if game.schedule_date != game.official_date {
        parts.push(format!(
            "Listed on the {} schedule; officialDate is {} ({}).",
            game.schedule_date, game.official_date, game.detailed_state
        ));
    }
    if !box_loaded {
        parts.push(format!(
            "Box score and play-by-play are stored only for final games (status: {}).",
            game.detailed_state
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn nest_plays(plays: &[PlateAppearance], pitches: &[Pitch]) -> Vec<PlateAppearanceJson> {
    plays
        .iter()
        .map(|play| PlateAppearanceJson {
            at_bat_index: play.at_bat_index,
            inning: play.inning,
            half_inning: play.half_inning.clone(),
            batter_id: play.batter_id,
            batter_name: play.batter_name.clone(),
            pitcher_id: play.pitcher_id,
            pitcher_name: play.pitcher_name.clone(),
            bat_side: play.bat_side.clone(),
            pitch_hand: play.pitch_hand.clone(),
            event: play.event.clone(),
            event_type: play.event_type.clone(),
            description: play.description.clone(),
            rbi: play.rbi,
            is_scoring_play: play.is_scoring_play,
            away_score: play.away_score,
            home_score: play.home_score,
            pitches: pitches
                .iter()
                .filter(|pitch| {
                    pitch.at_bat_index == play.at_bat_index && pitch.game_pk == play.game_pk
                })
                .map(|pitch| PitchJson {
                    pitch_number: pitch.pitch_number,
                    call_code: pitch.call_code.clone(),
                    call_description: pitch.call_description.clone(),
                    pitch_type_code: pitch.pitch_type_code.clone(),
                    pitch_type: pitch.pitch_type.clone(),
                    start_speed: pitch.start_speed,
                    is_ball: pitch.is_ball,
                    is_strike: pitch.is_strike,
                    is_in_play: pitch.is_in_play,
                })
                .collect(),
        })
        .collect()
}

async fn load_schedule<T: StatsApi>(
    state: &AppState<T>,
    season: u16,
    level: Level,
    date: &str,
) -> Result<Vec<ScheduledGame>, ApiError> {
    let store = StatsStore::new(state.data_dir.clone());
    let date_owned = date.to_string();
    if let ScheduleLoad::Games(games) =
        blocking(move || store.schedule(season, level, &date_owned)).await?
    {
        return Ok(games);
    }
    let _guard = state.materialize.lock().await;
    let store = StatsStore::new(state.data_dir.clone());
    let date_owned = date.to_string();
    if let ScheduleLoad::Games(games) =
        blocking(move || store.schedule(season, level, &date_owned)).await?
    {
        return Ok(games);
    }
    let games = materialize_schedule(&state.api, &state.data_dir, season, level, date).await?;
    Ok(games)
}

async fn load_game<T: StatsApi>(
    state: &AppState<T>,
    game_pk: u64,
) -> Result<ScheduledGame, ApiError> {
    let store = StatsStore::new(state.data_dir.clone());
    if let Some(game) = blocking(move || store.game_by_pk(game_pk)).await? {
        return Ok(game);
    }
    let _guard = state.materialize.lock().await;
    let store = StatsStore::new(state.data_dir.clone());
    if let Some(game) = blocking(move || store.game_by_pk(game_pk)).await? {
        return Ok(game);
    }
    Ok(materialize_game_header(&state.api, &state.data_dir, game_pk).await?)
}

async fn load_box<T: StatsApi>(
    state: &AppState<T>,
    game: &ScheduledGame,
) -> Result<diamond_stats_query::BoxScore, ApiError> {
    let stored = game.clone();
    let store = StatsStore::new(state.data_dir.clone());
    if let Some(lines) = blocking(move || store.boxscore(&stored)).await? {
        return Ok(lines);
    }
    let _guard = state.materialize.lock().await;
    let stored = game.clone();
    let store = StatsStore::new(state.data_dir.clone());
    if let Some(lines) = blocking(move || store.boxscore(&stored)).await? {
        return Ok(lines);
    }
    materialize_boxscore(&state.api, &state.data_dir, game).await?;
    let stored = game.clone();
    let store = StatsStore::new(state.data_dir.clone());
    blocking(move || store.boxscore(&stored))
        .await?
        .ok_or_else(|| ApiError::internal("box score file was not written"))
}

async fn load_plays<T: StatsApi>(
    state: &AppState<T>,
    game: &ScheduledGame,
) -> Result<diamond_stats_query::PlayFeed, ApiError> {
    let stored = game.clone();
    let store = StatsStore::new(state.data_dir.clone());
    if let Some(feed) = blocking(move || store.plays(&stored)).await? {
        return Ok(feed);
    }
    let _guard = state.materialize.lock().await;
    let stored = game.clone();
    let store = StatsStore::new(state.data_dir.clone());
    if let Some(feed) = blocking(move || store.plays(&stored)).await? {
        return Ok(feed);
    }
    materialize_plays(&state.api, &state.data_dir, game).await?;
    let stored = game.clone();
    let store = StatsStore::new(state.data_dir.clone());
    blocking(move || store.plays(&stored))
        .await?
        .ok_or_else(|| ApiError::internal("play-by-play file was not written"))
}

fn thresholds(
    stat: LeaderStat,
    min_pa: Option<u32>,
    min_ip: Option<f64>,
) -> (u32, u32, Option<u32>, Option<f64>) {
    match stat.group() {
        StatGroup::Hitting => {
            let pa = min_pa.unwrap_or(if stat.is_rate() { 502 } else { 0 });
            (pa, 0, Some(pa), None)
        }
        StatGroup::Pitching => {
            let innings = min_ip.unwrap_or(if stat.is_rate() { 162.0 } else { 0.0 });
            let outs = if innings <= 0.0 {
                0
            } else {
                (innings * 3.0).round() as u32
            };
            (0, outs, None, Some(innings))
        }
    }
}

fn leader_json(row: LeaderRow) -> LeaderJson {
    LeaderJson {
        rank: row.rank,
        player_id: row.player_id,
        player_name: row.player_name,
        team_names: row.team_names,
        value: row.value,
        plate_appearances: row.plate_appearances,
        outs: row.outs,
        innings_pitched: row.innings_pitched,
    }
}

async fn load_gamelog<T: StatsApi>(
    state: &AppState<T>,
    player_id: u32,
    season: u16,
    level: Level,
    group: StatGroup,
) -> Result<Vec<GameLogEntry>, ApiError> {
    let store = StatsStore::new(state.data_dir.clone());
    if let GameLogLoad::Games(games) =
        blocking(move || store.game_log(player_id, season, level, group)).await?
    {
        return Ok(games);
    }
    let _guard = state.materialize.lock().await;
    let store = StatsStore::new(state.data_dir.clone());
    if let GameLogLoad::Games(games) =
        blocking(move || store.game_log(player_id, season, level, group)).await?
    {
        return Ok(games);
    }
    materialize_game_log(&state.api, &state.data_dir, player_id, season, level, group).await?;
    let store = StatsStore::new(state.data_dir.clone());
    match blocking(move || store.game_log(player_id, season, level, group)).await? {
        GameLogLoad::Games(games) => Ok(games),
        GameLogLoad::NotMaterialized => Err(ApiError::internal("game log file was not written")),
    }
}

async fn blocking<T, F>(work: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, QueryError> + Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result.map_err(ApiError::from),
        Err(err) => Err(ApiError::internal(err.to_string())),
    }
}

fn parse_level(raw: &str) -> Result<Level, ApiError> {
    Level::parse(raw).map_err(|err| ApiError::bad_request(err.to_string()))
}

fn parse_u32(raw: &str, what: &str) -> Result<u32, ApiError> {
    raw.parse::<u32>()
        .map_err(|_| ApiError::bad_request(format!("{what} must be an integer")))
}

fn parse_u64(raw: &str, what: &str) -> Result<u64, ApiError> {
    raw.parse::<u64>()
        .map_err(|_| ApiError::bad_request(format!("{what} must be an integer")))
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "diamond-stats",
        version = "0.1.0",
        description = "Individual, non-commercial queries over MLB and Minor League statistics cached from the MLB Stats API. Not a bulk download service. Data is copyright MLB Advanced Media, L.P."
    ),
    paths(
        health,
        levels,
        list_teams,
        roster,
        player_seasons,
        player_gamelog,
        leaders,
        games_on_date,
        game_detail,
        game_plays,
        matchups
    ),
    components(schemas(
        HealthResponse,
        LevelsResponse,
        LevelJson,
        TeamsResponse,
        RosterResponse,
        PlayerSeasonsResponse,
        GameLogResponse,
        LeadersResponse,
        LeaderJson,
        GamesResponse,
        GameResponse,
        PlaysResponse,
        PlateAppearanceJson,
        PitchJson,
        MatchupResponse,
        ErrorBody,
        ErrorDetail,
        Attribution,
        BattingStint,
        PitchingStint,
        GameLogEntry,
        Team,
        Level,
        StatGroup,
        ScheduledGame,
        BoxBattingLine,
        BoxPitchingLine,
        PlateAppearance,
        PitchTracking
    )),
    tags(
        (name = "meta", description = "Service and level catalog"),
        (name = "teams", description = "Clubs and single-team stints"),
        (name = "players", description = "A player's stints and game log"),
        (name = "leaders", description = "Small leaderboards"),
        (name = "games", description = "Schedules, box scores, and play-by-play")
    )
)]
struct ApiDoc;
