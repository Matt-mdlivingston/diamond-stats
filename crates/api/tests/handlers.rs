//! Handler tests. The store is Parquet built from recorded fixtures.
//! The game-log path uses an in-memory stats client, not the network.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use diamond_stats_api::{AppState, router};
use diamond_stats_core::api_model::StatsEnvelope;
use diamond_stats_core::game_api::{BoxscoreEnvelope, PlayByPlayEnvelope, ScheduleEnvelope};
use diamond_stats_core::normalize::batting_stints;
use diamond_stats_core::{Level, NormalizeCtx, StatGroup};
use diamond_stats_ingest::{FixtureClient, PlayerQuery, write_batting};
use http::Request;
use http_body_util::BodyExt;
use serde_json::Value;
use tokio::sync::Mutex;
use tower::ServiceExt;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../core/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap()
}

fn app() -> (tempfile::TempDir, axum::Router, FixtureClient) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = NormalizeCtx {
        season: 2025,
        level: Level::Mlb,
        group: StatGroup::Hitting,
    };
    let mlb = stats("hitting_anthony_mlb.json");
    let mut rows = batting_stints(&mlb.stats[0].splits, ctx).unwrap();
    let raleigh = stats("hitting_raleigh.json");
    rows.extend(batting_stints(&raleigh.stats[0].splits, ctx).unwrap());
    write_batting(
        &diamond_stats_core::paths::partition_file(dir.path(), "batting", 2025, Level::Mlb),
        &rows,
    )
    .unwrap();
    let aaa = stats("hitting_anthony_aaa.json");
    write_batting(
        &diamond_stats_core::paths::partition_file(dir.path(), "batting", 2025, Level::Aaa),
        &batting_stints(
            &aaa.stats[0].splits,
            NormalizeCtx {
                season: 2025,
                level: Level::Aaa,
                group: StatGroup::Hitting,
            },
        )
        .unwrap(),
    )
    .unwrap();

    let client = FixtureClient::new();
    client.insert_game_log(
        592_450,
        PlayerQuery {
            season: 2025,
            sport_id: Level::Mlb.sport_id(),
            group: StatGroup::Hitting,
        },
        stats("judge_gamelog_two_games.json"),
    );
    let router = router(AppState {
        data_dir: dir.path().to_path_buf(),
        api: client.clone(),
        materialize: Arc::new(Mutex::new(())),
    });
    (dir, router, client)
}

fn stats(name: &str) -> StatsEnvelope {
    serde_json::from_str(&fixture(name)).unwrap()
}

async fn get(app: &axum::Router, uri: &str) -> (http::StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    (status, json)
}

#[tokio::test]
async fn health_levels_and_openapi() {
    let (_dir, app, _) = app();
    let (status, body) = get(&app, "/health").await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(body["status"], "ok");

    let (status, body) = get(&app, "/v1/levels").await;
    assert_eq!(status, http::StatusCode::OK);
    let slugs: Vec<&str> = body["levels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|level| level["slug"].as_str().unwrap())
        .collect();
    assert!(slugs.contains(&"mlb"));
    assert!(slugs.contains(&"aaa"));
    assert!(slugs.contains(&"aa"));
    assert!(slugs.contains(&"high_a"));
    assert!(slugs.contains(&"single_a"));
    assert!(slugs.contains(&"rookie"));
    assert!(!slugs.contains(&"minors"));

    let (status, body) = get(&app, "/api-docs/openapi.json").await;
    assert_eq!(status, http::StatusCode::OK);
    let text = body.to_string();
    assert!(text.contains("/v1/leaders"));
    assert!(text.contains("/v1/players/{player_id}/gamelog"));
    assert!(text.contains("/v1/games/{game_pk}/plays"));
    assert!(text.contains("/v1/matchups"));
}

#[tokio::test]
async fn seasons_cross_levels_and_errors_are_json() {
    let (_dir, app, _) = app();
    let (status, body) = get(&app, "/v1/players/701350/seasons").await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(body["player_name"], "Roman Anthony");
    assert_eq!(body["batting"].as_array().unwrap().len(), 2);
    let levels: Vec<&str> = body["batting"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["level"].as_str().unwrap())
        .collect();
    assert!(levels.contains(&"mlb"));
    assert!(levels.contains(&"aaa"));
    assert!(
        body["attribution"]["terms"]
            .as_str()
            .unwrap()
            .contains("copyright.txt")
    );

    let (status, body) = get(&app, "/v1/players/701350/seasons?season=2025").await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(body["batting"].as_array().unwrap().len(), 2);
    let (status, body) = get(&app, "/v1/players/701350/seasons?season=2025&level=mlb").await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(body["batting"].as_array().unwrap().len(), 1);
    assert_eq!(body["batting"][0]["level"], "mlb");
    let (status, body) = get(&app, "/v1/players/701350/seasons?level=aa").await;
    assert_eq!(status, http::StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"]["code"], "not_found");

    let (status, body) = get(&app, "/v1/players/1/seasons").await;
    assert_eq!(status, http::StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    let (status, body) = get(
        &app,
        "/v1/leaders?season=2025&level=nope&group=hitting&stat=hr",
    )
    .await;
    assert_eq!(status, http::StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "bad_request");

    let (status, _) = get(
        &app,
        "/v1/leaders?season=2025&level=mlb&group=hitting&stat=hr%3Bdrop",
    )
    .await;
    assert_eq!(status, http::StatusCode::BAD_REQUEST);

    let (status, body) = get(
        &app,
        "/v1/leaders?season=2025&level=mlb&group=hitting&stat=hr&limit=3",
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(body["leaders"][0]["player_id"], 663_728);
    assert_eq!(body["leaders"][0]["value"], 60.0);
    assert_eq!(body["order"], "desc");
}

#[tokio::test]
async fn gamelog_is_fetched_once_then_read_from_parquet() {
    let (dir, app, client) = app();
    let uri = "/v1/players/592450/gamelog?season=2025&level=mlb&group=hitting";
    let (status, body) = get(&app, uri).await;
    assert_eq!(status, http::StatusCode::OK, "{body}");
    assert_eq!(body["hitting"].as_array().unwrap().len(), 2);
    assert_eq!(body["hitting"][0]["game_pk"], 778_557);
    assert_eq!(body["hitting"][0]["hits"], 1);
    assert_eq!(client.calls().len(), 1);

    let (status, _) = get(&app, uri).await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(client.calls().len(), 1, "second read must not refetch");
    assert!(
        diamond_stats_core::paths::gamelog_file(
            dir.path(),
            StatGroup::Hitting,
            2025,
            Level::Mlb,
            592_450
        )
        .is_file()
    );
}

#[tokio::test]
async fn box_score_and_plays_are_fetched_once() {
    let dir = tempfile::tempdir().unwrap();
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
        serde_json::from_str::<PlayByPlayEnvelope>(&fixture("playbyplay_mlb_777420_one_pa.json"))
            .unwrap(),
    );
    let app = router(AppState {
        data_dir: dir.path().to_path_buf(),
        api: client.clone(),
        materialize: Arc::new(Mutex::new(())),
    });

    let (status, body) = get(&app, "/v1/games?season=2025&level=mlb&date=2025-06-21").await;
    assert_eq!(status, http::StatusCode::OK, "{body}");
    assert_eq!(body["games"].as_array().unwrap().len(), 2);
    assert_eq!(body["games"][0]["away_score"], 3);

    let (status, body) = get(&app, "/v1/games/777420").await;
    assert_eq!(status, http::StatusCode::OK, "{body}");
    assert_eq!(body["boxscore_loaded"], true);
    assert_eq!(body["game"]["home_team_name"], "Tampa Bay Rays");
    let keith = body["batting"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["player_id"] == 690_993)
        .unwrap();
    assert_eq!(keith["at_bats"], 4);
    assert_eq!(keith["hits"], 1);
    assert_eq!(keith["avg"], 0.25);
    let calls = client.calls().len();

    let (status, _) = get(&app, "/v1/games/777420").await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(
        client.calls().len(),
        calls,
        "box score must be read from parquet"
    );

    let (status, body) = get(&app, "/v1/games/777420/plays").await;
    assert_eq!(status, http::StatusCode::OK, "{body}");
    assert_eq!(body["plate_appearances"][0]["event"], "Lineout");
    assert_eq!(
        body["plate_appearances"][0]["pitches"][0]["start_speed"],
        94.0
    );
    assert_eq!(
        body["plate_appearances"][0]["pitches"][0]["pitch_type_code"],
        "FF"
    );
    assert!(
        body["pitch_tracking_note"]
            .as_str()
            .unwrap()
            .contains("every pitch")
    );
    let calls = client.calls().len();
    let (status, _) = get(&app, "/v1/games/777420/plays").await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(client.calls().len(), calls);

    let (status, body) = get(
        &app,
        "/v1/matchups?season=2025&level=mlb&batter_id=690993&pitcher_id=686752",
    )
    .await;
    assert_eq!(status, http::StatusCode::OK, "{body}");
    assert_eq!(body["plate_appearances"].as_array().unwrap().len(), 1);
    assert_eq!(body["plate_appearances"][0]["event"], "Lineout");

    let (status, body) = get(&app, "/v1/matchups?season=2025&level=mlb").await;
    assert_eq!(status, http::StatusCode::BAD_REQUEST, "{body}");
}
