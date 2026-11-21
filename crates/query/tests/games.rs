//! Box score and play-by-play reads over Parquet written from recorded fixtures.

use std::path::PathBuf;

use diamond_stats_core::game_api::{BoxscoreEnvelope, PlayByPlayEnvelope, ScheduleEnvelope};
use diamond_stats_core::game_norm::{box_score, play_by_play, schedule_games};
use diamond_stats_core::normalize::batting_stints;
use diamond_stats_core::{LeaderStat, Level, NormalizeCtx, StatGroup};
use diamond_stats_ingest::{
    write_batting, write_box_batting, write_box_pitching, write_pitches, write_plays,
    write_schedule,
};
use diamond_stats_query::{LeaderRequest, MatchupRequest, ScheduleLoad, StatsStore};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../core/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap()
}

#[test]
fn level_filter_keeps_a_double_a_line_off_the_mlb_board() {
    let dir = tempfile::tempdir().unwrap();
    let doc = serde_json::from_str::<diamond_stats_core::api_model::StatsEnvelope>(&fixture(
        "hitting_aa_one.json",
    ))
    .unwrap();
    let rows = batting_stints(
        &doc.stats[0].splits,
        NormalizeCtx {
            season: 2025,
            level: Level::Aa,
            group: StatGroup::Hitting,
        },
    )
    .unwrap();
    write_batting(
        &diamond_stats_core::paths::partition_file(dir.path(), "batting", 2025, Level::Aa),
        &rows,
    )
    .unwrap();
    write_batting(
        &diamond_stats_core::paths::partition_file(dir.path(), "batting", 2025, Level::Mlb),
        &[],
    )
    .unwrap();
    let store = StatsStore::new(dir.path());
    let leaders = store
        .leaders(LeaderRequest {
            season: 2025,
            level: Level::Aa,
            stat: LeaderStat::HomeRuns,
            limit: 5,
            min_pa: 0,
            min_outs: 0,
        })
        .unwrap();
    assert_eq!(leaders.len(), 1);
    assert_eq!(leaders[0].player_name, "Juan Sulbaran");
    let mlb = store
        .leaders(LeaderRequest {
            season: 2025,
            level: Level::Mlb,
            stat: LeaderStat::HomeRuns,
            limit: 5,
            min_pa: 0,
            min_outs: 0,
        })
        .unwrap();
    assert!(mlb.is_empty());
}

#[test]
fn schedule_box_and_matchup_round_trip_the_recorded_game() {
    let dir = tempfile::tempdir().unwrap();
    let schedule: ScheduleEnvelope =
        serde_json::from_str(&fixture("schedule_mlb_2025-06-21_two.json")).unwrap();
    let games = schedule_games(&schedule, 2025, Level::Mlb, "2025-06-21").unwrap();
    write_schedule(
        &diamond_stats_core::paths::schedule_file(dir.path(), 2025, Level::Mlb, "2025-06-21"),
        &games,
    )
    .unwrap();
    let game = games.iter().find(|game| game.game_pk == 777_420).unwrap();
    let box_doc: BoxscoreEnvelope =
        serde_json::from_str(&fixture("boxscore_mlb_777420_trimmed.json")).unwrap();
    let (batting, pitching) = box_score(&box_doc, game).unwrap();
    write_box_batting(
        &diamond_stats_core::paths::box_batting_file(dir.path(), 2025, Level::Mlb, 777_420),
        &batting,
    )
    .unwrap();
    write_box_pitching(
        &diamond_stats_core::paths::box_pitching_file(dir.path(), 2025, Level::Mlb, 777_420),
        &pitching,
    )
    .unwrap();
    let plays_doc: PlayByPlayEnvelope =
        serde_json::from_str(&fixture("playbyplay_mlb_777420_one_pa.json")).unwrap();
    let (plays, pitches) = play_by_play(&plays_doc, game).unwrap();
    write_plays(
        &diamond_stats_core::paths::plays_file(dir.path(), 2025, Level::Mlb, 777_420),
        &plays,
    )
    .unwrap();
    write_pitches(
        &diamond_stats_core::paths::pitches_file(dir.path(), 2025, Level::Mlb, 777_420),
        &pitches,
    )
    .unwrap();

    let store = StatsStore::new(dir.path());
    let ScheduleLoad::Games(listed) = store.schedule(2025, Level::Mlb, "2025-06-21").unwrap()
    else {
        panic!("schedule was written");
    };
    assert_eq!(listed.len(), 2);
    assert_eq!(
        store.schedule(2025, Level::Mlb, "2025-06-22").unwrap(),
        ScheduleLoad::NotMaterialized
    );

    let loaded = store.game_by_pk(777_420).unwrap().unwrap();
    assert_eq!(loaded.away_score, Some(3));
    assert_eq!(loaded.home_team_name, "Tampa Bay Rays");
    let lines = store.boxscore(&loaded).unwrap().unwrap();
    let keith = lines
        .batting
        .iter()
        .find(|row| row.player_id == 690_993)
        .unwrap();
    assert_eq!(keith.at_bats, 4);
    assert_eq!(keith.hits, 1);
    assert_eq!(keith.avg, Some(0.25));
    let hurter = lines
        .pitching
        .iter()
        .find(|row| row.player_id == 676_428)
        .unwrap();
    assert_eq!(hurter.outs, 2);
    assert!(hurter.era.unwrap().abs() < 1e-9);

    let feed = store.plays(&loaded).unwrap().unwrap();
    assert_eq!(feed.plate_appearances.len(), 1);
    assert_eq!(feed.pitches[0].start_speed, Some(94.0));
    assert_eq!(feed.pitches[0].pitch_type_code.as_deref(), Some("FF"));

    let matchup = store
        .matchups(MatchupRequest {
            season: 2025,
            level: Level::Mlb,
            batter_id: Some(690_993),
            pitcher_id: Some(686_752),
            limit: 10,
        })
        .unwrap();
    assert_eq!(matchup.len(), 1);
    assert_eq!(matchup[0].event.as_deref(), Some("Lineout"));
    assert!(
        matchup[0]
            .description
            .as_deref()
            .unwrap()
            .contains("Colt Keith")
    );

    let other_level = store
        .matchups(MatchupRequest {
            season: 2025,
            level: Level::Aa,
            batter_id: Some(690_993),
            pitcher_id: None,
            limit: 10,
        })
        .unwrap();
    assert!(other_level.is_empty());
}

#[test]
fn schedule_load_compares_for_the_missing_date() {
    // ScheduleLoad is compared in the round trip. This keeps PartialEq honest if it changes.
    assert_ne!(
        ScheduleLoad::NotMaterialized,
        ScheduleLoad::Games(Vec::new())
    );
}
