//! Schedule, box score, and play-by-play ingest.
//!
//! A date listing is one schedule request. Box scores and play-by-play are one
//! game at a time, and only for games whose status is an official final.
//! Postponed games stay on the schedule. The HTTP client caches each URL, paces
//! requests, and retries 429 and 5xx. This module does not crawl a season of games.

use std::path::Path;

use diamond_stats_core::game_api::ScheduleEnvelope;
use diamond_stats_core::game_norm::{box_score, one_schedule_game, play_by_play, schedule_games};
use diamond_stats_core::paths::{
    box_batting_file, box_pitching_file, game_header_file, pitches_file, plays_file, schedule_file,
};
use diamond_stats_core::{
    BoxBattingLine, BoxPitchingLine, Level, Pitch, PitchTracking, PlateAppearance, ScheduledGame,
    parse_iso_date,
};
use tracing::info;

use crate::client::StatsApi;
use crate::error::IngestError;
use crate::parquet_games::{
    write_box_batting, write_box_pitching, write_game_header, write_pitches, write_plays,
    write_schedule,
};

/// What `ingest-games` should pull.
#[derive(Debug, Clone)]
pub struct DateGamesOptions {
    /// Data directory.
    pub data_dir: std::path::PathBuf,
    /// Season year. Must match the schedule's season.
    pub season: u16,
    /// Level, which selects `sportId`.
    pub level: Level,
    /// Schedule date, `YYYY-MM-DD`.
    pub date: String,
    /// How many final games to fetch box scores and play-by-play for.
    /// `None` means every final game on the date. `Some(0)` stores the schedule only.
    pub box_limit: Option<u32>,
}

/// One game whose box score and play-by-play were written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameWriteSummary {
    /// `gamePk`.
    pub game_pk: u64,
    /// Batting lines written.
    pub batting_lines: usize,
    /// Pitching lines written.
    pub pitching_lines: usize,
    /// Plate appearances written.
    pub plate_appearances: usize,
    /// Pitch events written.
    pub pitches: usize,
    /// Pitches that included a release speed.
    pub pitches_with_velocity: usize,
    /// Pitches that included a pitch type.
    pub pitches_with_pitch_type: usize,
}

/// Result of storing one date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateGamesSummary {
    /// Season.
    pub season: u16,
    /// Level.
    pub level: Level,
    /// Schedule date.
    pub date: String,
    /// Games on the schedule, including postponed games.
    pub games: usize,
    /// Games whose status says a box score exists.
    pub final_games: usize,
    /// Games whose box score and play-by-play were written, in `gamePk` order.
    pub fetched: Vec<GameWriteSummary>,
}

/// Fetch a date's schedule and, up to `box_limit`, each final game's box and plays.
pub async fn ingest_games(
    api: &impl StatsApi,
    options: &DateGamesOptions,
) -> Result<DateGamesSummary, IngestError> {
    let date = parse_iso_date(&options.date).map_err(IngestError::from)?;
    let games =
        materialize_schedule(api, &options.data_dir, options.season, options.level, &date).await?;
    let final_games = games.iter().filter(|game| game.boxscore_ready()).count();
    let mut ready: Vec<ScheduledGame> = games
        .iter()
        .filter(|game| game.boxscore_ready())
        .cloned()
        .collect();
    ready.sort_by_key(|game| game.game_pk);
    if let Some(limit) = options.box_limit {
        ready.truncate(limit as usize);
    }
    let mut fetched = Vec::with_capacity(ready.len());
    for game in &ready {
        let summary = materialize_game_detail(api, &options.data_dir, game).await?;
        info!(
            game_pk = game.game_pk,
            batting = summary.batting_lines,
            pitching = summary.pitching_lines,
            plays = summary.plate_appearances,
            pitches = summary.pitches,
            velocity = summary.pitches_with_velocity,
            pitch_type = summary.pitches_with_pitch_type,
            "wrote box score and play-by-play"
        );
        fetched.push(summary);
    }
    Ok(DateGamesSummary {
        season: options.season,
        level: options.level,
        date,
        games: games.len(),
        final_games,
        fetched,
    })
}

/// Fetch and store the schedule for one date. An empty day is still written.
pub async fn materialize_schedule(
    api: &impl StatsApi,
    data_dir: &Path,
    season: u16,
    level: Level,
    date: &str,
) -> Result<Vec<ScheduledGame>, IngestError> {
    let date = parse_iso_date(date).map_err(IngestError::from)?;
    let envelope = api.schedule_date(level.sport_id(), date.clone()).await?;
    let games = schedule_games(&envelope, season, level, &date)?;
    let path = schedule_file(data_dir, season, level, &date);
    write_schedule(&path, &games)?;
    info!(
        season,
        %level,
        date,
        games = games.len(),
        "wrote schedule"
    );
    Ok(games)
}

/// Resolve one `gamePk` that is not already on disk.
///
/// The schedule payload has no `sportId`. The away club's team record supplies it.
/// `schedule_date` on this row is the game's `officialDate`, because a single-game
/// lookup is not a full date listing.
pub async fn materialize_game_header(
    api: &impl StatsApi,
    data_dir: &Path,
    game_pk: u64,
) -> Result<ScheduledGame, IngestError> {
    let envelope = api.schedule_game(game_pk).await?;
    let (team_id, season) = schedule_identity(&envelope, game_pk)?;
    let teams = api.team(team_id, season).await?;
    let sport_id = teams
        .teams
        .iter()
        .find(|team| team.id == team_id)
        .and_then(|team| team.sport.as_ref())
        .and_then(|sport| sport.id)
        .ok_or_else(|| {
            IngestError::Message(format!(
                "team {team_id} did not include a sportId for game {game_pk}"
            ))
        })?;
    let level = Level::from_sport_id(sport_id).ok_or_else(|| {
        IngestError::Message(format!(
            "game {game_pk} is sportId {sport_id}, which diamond-stats does not store"
        ))
    })?;
    let game = one_schedule_game(&envelope, level)?;
    let path = game_header_file(data_dir, game.season, game.level, game.game_pk);
    write_game_header(&path, &game)?;
    info!(game_pk, %level, "wrote game header");
    Ok(game)
}

/// Fetch and store the box score for a final game.
pub async fn materialize_boxscore(
    api: &impl StatsApi,
    data_dir: &Path,
    game: &ScheduledGame,
) -> Result<(Vec<BoxBattingLine>, Vec<BoxPitchingLine>), IngestError> {
    let envelope = api.boxscore(game.game_pk).await?;
    let (batting, pitching) = box_score(&envelope, game)?;
    write_box_batting(
        &box_batting_file(data_dir, game.season, game.level, game.game_pk),
        &batting,
    )?;
    write_box_pitching(
        &box_pitching_file(data_dir, game.season, game.level, game.game_pk),
        &pitching,
    )?;
    Ok((batting, pitching))
}

/// Fetch and store play-by-play. Empty feeds are written so they are not retried.
pub async fn materialize_plays(
    api: &impl StatsApi,
    data_dir: &Path,
    game: &ScheduledGame,
) -> Result<(Vec<PlateAppearance>, Vec<Pitch>), IngestError> {
    let envelope = api.play_by_play(game.game_pk).await?;
    let (plays, pitches) = play_by_play(&envelope, game)?;
    write_plays(
        &plays_file(data_dir, game.season, game.level, game.game_pk),
        &plays,
    )?;
    write_pitches(
        &pitches_file(data_dir, game.season, game.level, game.game_pk),
        &pitches,
    )?;
    Ok((plays, pitches))
}

async fn materialize_game_detail(
    api: &impl StatsApi,
    data_dir: &Path,
    game: &ScheduledGame,
) -> Result<GameWriteSummary, IngestError> {
    let (batting, pitching) = materialize_boxscore(api, data_dir, game).await?;
    let (plays, pitches) = materialize_plays(api, data_dir, game).await?;
    let tracking = PitchTracking::from_rows(&plays, &pitches);
    Ok(GameWriteSummary {
        game_pk: game.game_pk,
        batting_lines: batting.len(),
        pitching_lines: pitching.len(),
        plate_appearances: plays.len(),
        pitches: pitches.len(),
        pitches_with_velocity: usize::try_from(tracking.pitches_with_velocity)
            .unwrap_or(usize::MAX),
        pitches_with_pitch_type: usize::try_from(tracking.pitches_with_pitch_type)
            .unwrap_or(usize::MAX),
    })
}

fn schedule_identity(envelope: &ScheduleEnvelope, game_pk: u64) -> Result<(u32, u16), IngestError> {
    let game = envelope
        .dates
        .iter()
        .flat_map(|date| date.games.iter())
        .find(|game| game.game_pk == game_pk)
        .or_else(|| {
            envelope
                .dates
                .iter()
                .flat_map(|date| date.games.iter())
                .next()
        });
    let Some(game) = game else {
        return Err(IngestError::GameNotFound { game_pk });
    };
    let season = game
        .season
        .as_deref()
        .ok_or_else(|| IngestError::Message(format!("game {game_pk} is missing a season")))?
        .parse::<u16>()
        .map_err(|_| IngestError::Message(format!("game {game_pk} has a non-numeric season")))?;
    Ok((game.teams.away.team.id, season))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::FixtureClient;
    use diamond_stats_core::api_model::TeamsEnvelope;
    use diamond_stats_core::game_api::{BoxscoreEnvelope, PlayByPlayEnvelope, ScheduleEnvelope};
    use diamond_stats_core::paths::{box_batting_file, pitches_file, plays_file, schedule_file};
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../core/fixtures")
            .join(name);
        std::fs::read_to_string(&path).unwrap()
    }

    #[tokio::test]
    async fn date_ingest_skips_games_past_the_limit_and_writes_files() {
        let client = FixtureClient::new();
        let schedule: ScheduleEnvelope =
            serde_json::from_str(&fixture("schedule_mlb_2025-06-21_two.json")).unwrap();
        client.insert_schedule_date(Level::Mlb.sport_id(), "2025-06-21", schedule);
        client.insert_boxscore(
            777_420,
            serde_json::from_str::<BoxscoreEnvelope>(&fixture("boxscore_mlb_777420_trimmed.json"))
                .unwrap(),
        );
        client.insert_plays(
            777_420,
            serde_json::from_str::<PlayByPlayEnvelope>(&fixture(
                "playbyplay_mlb_777420_one_pa.json",
            ))
            .unwrap(),
        );
        let dir = tempfile::tempdir().unwrap();
        let summary = ingest_games(
            &client,
            &DateGamesOptions {
                data_dir: dir.path().to_path_buf(),
                season: 2025,
                level: Level::Mlb,
                date: "2025-06-21".into(),
                box_limit: Some(1),
            },
        )
        .await
        .unwrap();
        assert_eq!(summary.games, 2);
        assert_eq!(summary.final_games, 2);
        assert_eq!(summary.fetched.len(), 1);
        assert_eq!(summary.fetched[0].game_pk, 777_420);
        assert_eq!(summary.fetched[0].batting_lines, 2);
        assert_eq!(summary.fetched[0].pitching_lines, 2);
        assert_eq!(summary.fetched[0].plate_appearances, 1);
        assert_eq!(summary.fetched[0].pitches, 3);
        assert_eq!(summary.fetched[0].pitches_with_velocity, 3);
        assert_eq!(summary.fetched[0].pitches_with_pitch_type, 3);
        assert!(schedule_file(dir.path(), 2025, Level::Mlb, "2025-06-21").is_file());
        assert!(box_batting_file(dir.path(), 2025, Level::Mlb, 777_420).is_file());
        assert!(plays_file(dir.path(), 2025, Level::Mlb, 777_420).is_file());
        assert!(pitches_file(dir.path(), 2025, Level::Mlb, 777_420).is_file());
        let calls = client.calls();
        assert!(calls.iter().any(|call| call == "boxscore:777420"));
        assert!(!calls.iter().any(|call| call.contains("777422")));
    }

    #[tokio::test]
    async fn postponed_games_are_stored_and_not_fetched() {
        let client = FixtureClient::new();
        let schedule: ScheduleEnvelope = serde_json::from_str(&fixture(
            "schedule_high_a_2025-06-21_final_and_postponed.json",
        ))
        .unwrap();
        client.insert_schedule_date(Level::HighA.sport_id(), "2025-06-21", schedule);
        let minimal = r#"{
            "copyright": "Copyright 2026 MLB Advanced Media, L.P.  Use of any content on this page acknowledges agreement to the terms posted here http://gdx.mlb.com/components/copyright.txt",
            "teams": {
                "away": {"team": {"id": 573, "name": "Asheville Tourists", "sport": {"id": 13}}, "players": {}, "batters": [], "pitchers": []},
                "home": {"team": {"id": 426, "name": "Wilmington Blue Rocks", "sport": {"id": 13}}, "players": {}, "batters": [], "pitchers": []}
            }
        }"#;
        client.insert_boxscore(783_928, serde_json::from_str(minimal).unwrap());
        client.insert_plays(
            783_928,
            serde_json::from_str(
                r#"{"copyright":"Copyright 2026 MLB Advanced Media, L.P.","allPlays":[]}"#,
            )
            .unwrap(),
        );
        let dir = tempfile::tempdir().unwrap();
        let summary = ingest_games(
            &client,
            &DateGamesOptions {
                data_dir: dir.path().to_path_buf(),
                season: 2025,
                level: Level::HighA,
                date: "2025-06-21".into(),
                box_limit: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(summary.games, 2);
        assert_eq!(summary.final_games, 1);
        assert_eq!(summary.fetched.len(), 1);
        assert_eq!(summary.fetched[0].game_pk, 783_928);
        assert_eq!(summary.fetched[0].plate_appearances, 0);
        assert_eq!(summary.fetched[0].pitches_with_velocity, 0);
        let calls = client.calls();
        assert!(calls.iter().any(|call| call == "boxscore:783928"));
        assert!(!calls.iter().any(|call| call.contains("789134")));
    }

    #[tokio::test]
    async fn game_pk_header_learns_the_level_from_the_club() {
        let client = FixtureClient::new();
        let mut schedule: ScheduleEnvelope =
            serde_json::from_str(&fixture("schedule_mlb_2025-06-21_two.json")).unwrap();
        schedule.dates[0].games.truncate(1);
        schedule.total_games = Some(1);
        client.insert_schedule_game(777_420, schedule);
        let team: TeamsEnvelope = serde_json::from_str(
            r#"{"copyright":"Copyright 2026 MLB Advanced Media, L.P.","teams":[{"id":116,"name":"Detroit Tigers","season":2025,"sport":{"id":1}}]}"#,
        )
        .unwrap();
        client.insert_team(116, 2025, team);
        let dir = tempfile::tempdir().unwrap();
        let game = materialize_game_header(&client, dir.path(), 777_420)
            .await
            .unwrap();
        assert_eq!(game.level, Level::Mlb);
        assert_eq!(game.away_score, Some(3));
        assert!(game.boxscore_ready());
        assert!(game_header_file(dir.path(), 2025, Level::Mlb, 777_420).is_file());
        assert_eq!(
            client.calls(),
            vec![
                "schedule-game:777420".to_string(),
                "team:116:2025".to_string()
            ]
        );
    }
}
