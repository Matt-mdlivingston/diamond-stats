//! Query tests over Parquet built from trimmed, recorded Stats API responses.
//! Numbers asserted here are the values in `crates/core/fixtures`, not invented lines.

use std::path::PathBuf;

use diamond_stats_core::api_model::{StatsEnvelope, TeamsEnvelope};
use diamond_stats_core::normalize::{batting_stints, pitching_stints, teams};
use diamond_stats_core::{LeaderStat, Level, NormalizeCtx, StatGroup};
use diamond_stats_ingest::{write_batting, write_pitching, write_teams};
use diamond_stats_query::{GameLogLoad, LeaderRequest, QueryError, SeasonFilter, StatsStore};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../core/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap()
}

fn stats(name: &str) -> StatsEnvelope {
    serde_json::from_str(&fixture(name)).unwrap()
}

fn hitting(name: &str, level: Level) -> Vec<diamond_stats_core::BattingStint> {
    let doc = stats(name);
    batting_stints(
        &doc.stats[0].splits,
        NormalizeCtx {
            season: 2025,
            level,
            group: StatGroup::Hitting,
        },
    )
    .unwrap()
}

fn pitching(name: &str) -> Vec<diamond_stats_core::PitchingStint> {
    let doc = stats(name);
    pitching_stints(
        &doc.stats[0].splits,
        NormalizeCtx {
            season: 2025,
            level: Level::Mlb,
            group: StatGroup::Pitching,
        },
    )
    .unwrap()
}

fn store() -> (tempfile::TempDir, StatsStore) {
    let dir = tempfile::tempdir().unwrap();
    let mut mlb_batting = Vec::new();
    mlb_batting.extend(hitting("hitting_raleigh.json", Level::Mlb));
    mlb_batting.extend(hitting("hitting_nelson.json", Level::Mlb));
    mlb_batting.extend(hitting("hitting_anthony_mlb.json", Level::Mlb));
    mlb_batting.extend(hitting("naylor_people_hitting.json", Level::Mlb));
    write_batting(
        &diamond_stats_core::paths::partition_file(dir.path(), "batting", 2025, Level::Mlb),
        &mlb_batting,
    )
    .unwrap();
    write_batting(
        &diamond_stats_core::paths::partition_file(dir.path(), "batting", 2025, Level::Aaa),
        &hitting("hitting_anthony_aaa.json", Level::Aaa),
    )
    .unwrap();

    let mut mlb_pitching = Vec::new();
    mlb_pitching.extend(pitching("pitching_skenes.json"));
    mlb_pitching.extend(pitching("pitching_acton.json"));
    mlb_pitching.extend(pitching("rogers_people_pitching.json"));
    write_pitching(
        &diamond_stats_core::paths::partition_file(dir.path(), "pitching", 2025, Level::Mlb),
        &mlb_pitching,
    )
    .unwrap();

    let mlb_teams: TeamsEnvelope =
        serde_json::from_str(&fixture("teams_mlb_trimmed.json")).unwrap();
    write_teams(
        &diamond_stats_core::paths::partition_file(dir.path(), "teams", 2025, Level::Mlb),
        &teams(&mlb_teams, 2025, Level::Mlb).unwrap(),
    )
    .unwrap();

    let store = StatsStore::new(dir.path());
    (dir, store)
}

#[test]
fn home_run_leader_is_the_recorded_raleigh_line() {
    let (_dir, store) = store();
    let leaders = store
        .leaders(LeaderRequest {
            season: 2025,
            level: Level::Mlb,
            stat: LeaderStat::HomeRuns,
            limit: 5,
            min_pa: 0,
            min_outs: 0,
        })
        .unwrap();
    assert_eq!(leaders[0].player_id, 663_728);
    assert_eq!(leaders[0].player_name, "Cal Raleigh");
    assert_eq!(leaders[0].value, 60.0);
    assert_eq!(leaders[0].rank, 1);
    let naylor = leaders.iter().find(|row| row.player_id == 647_304).unwrap();
    assert_eq!(naylor.value, 20.0);
    assert!(naylor.team_names.contains("Arizona Diamondbacks"));
    assert!(naylor.team_names.contains("Seattle Mariners"));
}

#[test]
fn rate_leaders_honor_the_sample_floor_and_sum_stints() {
    let (_dir, store) = store();
    let raw = store
        .leaders(LeaderRequest {
            season: 2025,
            level: Level::Mlb,
            stat: LeaderStat::Avg,
            limit: 5,
            min_pa: 0,
            min_outs: 0,
        })
        .unwrap();
    assert_eq!(raw[0].player_id, 669_194);
    assert_eq!(raw[0].value, 1.0);

    let qualified = store
        .leaders(LeaderRequest {
            season: 2025,
            level: Level::Mlb,
            stat: LeaderStat::Avg,
            limit: 5,
            min_pa: 502,
            min_outs: 0,
        })
        .unwrap();
    assert_eq!(qualified[0].player_id, 647_304);
    assert!((qualified[0].value - (160.0 / 543.0)).abs() < 1e-9);
    assert_eq!(qualified[1].player_id, 663_728);
    assert!((qualified[1].value - (147.0 / 596.0)).abs() < 1e-9);
}

#[test]
fn era_leaders_sort_ascending_and_drop_tiny_samples() {
    let (_dir, store) = store();
    let everyone = store
        .leaders(LeaderRequest {
            season: 2025,
            level: Level::Mlb,
            stat: LeaderStat::Era,
            limit: 5,
            min_pa: 0,
            min_outs: 0,
        })
        .unwrap();
    assert_eq!(everyone[0].player_id, 670_183);
    assert_eq!(everyone[0].value, 0.0);

    let qualified = store
        .leaders(LeaderRequest {
            season: 2025,
            level: Level::Mlb,
            stat: LeaderStat::Era,
            limit: 5,
            min_pa: 0,
            min_outs: 486,
        })
        .unwrap();
    assert_eq!(qualified.len(), 1);
    assert_eq!(qualified[0].player_id, 694_973);
    assert!((qualified[0].value - 1.97).abs() < 0.01);
    assert_eq!(qualified[0].innings_pitched.as_deref(), Some("187.2"));
}

#[test]
fn player_seasons_show_a_callup_and_a_trade() {
    let (_dir, store) = store();
    let anthony = store.player_seasons(701_350).unwrap();
    assert_eq!(anthony.player_name.as_deref(), Some("Roman Anthony"));
    assert_eq!(anthony.batting.len(), 2);
    assert!(anthony.pitching.is_empty());
    let mlb = anthony
        .batting
        .iter()
        .find(|row| row.level == Level::Mlb)
        .unwrap();
    let aaa = anthony
        .batting
        .iter()
        .find(|row| row.level == Level::Aaa)
        .unwrap();
    assert_eq!(mlb.team_name, "Boston Red Sox");
    assert_eq!(mlb.home_runs, 8);
    assert_eq!(aaa.team_name, "Worcester Red Sox");
    assert_eq!(aaa.home_runs, 10);

    let naylor = store.player_seasons(647_304).unwrap();
    assert_eq!(naylor.batting.len(), 2);
    let games: u32 = naylor.batting.iter().map(|row| row.games).sum();
    assert_eq!(games, 147);

    let season = store
        .player_seasons_filtered(
            701_350,
            SeasonFilter {
                season: Some(2025),
                level: None,
            },
        )
        .unwrap();
    let levels: Vec<_> = season.batting.iter().map(|row| row.level).collect();
    assert_eq!(levels, vec![Level::Aaa, Level::Mlb]);
    let only_mlb = store
        .player_seasons_filtered(
            701_350,
            SeasonFilter {
                season: Some(2025),
                level: Some(Level::Mlb),
            },
        )
        .unwrap();
    assert_eq!(only_mlb.batting.len(), 1);
    assert_eq!(only_mlb.batting[0].level, Level::Mlb);
    let no_aa = store
        .player_seasons_filtered(
            701_350,
            SeasonFilter {
                season: Some(2025),
                level: Some(Level::Aa),
            },
        )
        .unwrap();
    assert!(no_aa.is_empty());
}

#[test]
fn roster_uses_the_team_stint_not_the_combined_line() {
    let (_dir, store) = store();
    let mariners = store.team_roster(136, 2025, Level::Mlb).unwrap();
    assert_eq!(mariners.team_name.as_deref(), Some("Seattle Mariners"));
    let naylor = mariners
        .batting
        .iter()
        .find(|row| row.player_id == 647_304)
        .unwrap();
    assert_eq!(naylor.games, 54);
    assert_eq!(naylor.home_runs, 9);
    let raleigh = mariners
        .batting
        .iter()
        .find(|row| row.player_id == 663_728)
        .unwrap();
    assert_eq!(raleigh.home_runs, 60);

    let arizona = store.team_roster(109, 2025, Level::Mlb).unwrap();
    let naylor = arizona
        .batting
        .iter()
        .find(|row| row.player_id == 647_304)
        .unwrap();
    assert_eq!(naylor.games, 93);
    assert_eq!(naylor.home_runs, 11);
}

#[test]
fn missing_partition_is_not_an_empty_leaderboard() {
    let (_dir, store) = store();
    let error = store
        .leaders(LeaderRequest {
            season: 2024,
            level: Level::Mlb,
            stat: LeaderStat::HomeRuns,
            limit: 5,
            min_pa: 0,
            min_outs: 0,
        })
        .unwrap_err();
    assert!(matches!(error, QueryError::DataNotLoaded { .. }));
}

#[test]
fn game_log_roundtrip_preserves_recorded_games() {
    let (dir, store) = store();
    assert!(matches!(
        store
            .game_log(592_450, 2025, Level::Mlb, StatGroup::Hitting)
            .unwrap(),
        GameLogLoad::NotMaterialized
    ));
    let doc = stats("judge_gamelog_two_games.json");
    let games = diamond_stats_core::normalize::game_log(
        &doc,
        NormalizeCtx {
            season: 2025,
            level: Level::Mlb,
            group: StatGroup::Hitting,
        },
        592_450,
    )
    .unwrap();
    diamond_stats_ingest::write_batting_gamelog(
        &diamond_stats_core::paths::gamelog_file(
            dir.path(),
            StatGroup::Hitting,
            2025,
            Level::Mlb,
            592_450,
        ),
        &games,
    )
    .unwrap();
    let GameLogLoad::Games(loaded) = store
        .game_log(592_450, 2025, Level::Mlb, StatGroup::Hitting)
        .unwrap()
    else {
        panic!("game log should be materialized");
    };
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].game_pk, 778_557);
    assert_eq!(loaded[0].game_date, "2025-03-27");
    assert_eq!(loaded[0].hits, 1);
    assert_eq!(loaded[0].at_bats, 4);
    assert_eq!(loaded[1].home_runs, 3);
}

#[test]
fn stat_names_are_not_sql() {
    assert!(LeaderStat::parse(StatGroup::Hitting, "hr; drop table batting").is_none());
    assert!(
        LeaderStat::parse(StatGroup::Pitching, "era")
            .unwrap()
            .lower_is_better()
    );
}
