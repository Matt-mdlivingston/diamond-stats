//! Season ingest and lazy game-log materialization.
//!
//! Game logs are not crawled up front. A full MLB season is a few hundred
//! season-stat requests after multi-team hydration, and one request per player
//! per group for game logs. Milestone 1 fetches a game log the first time it
//! is queried and writes that player's Parquet file. Later reads hit the file.

use std::collections::HashMap;
use std::path::Path;

use diamond_stats_core::normalize::{
    batting_stints, game_log, hydration_ids, pitching_stints, teams,
};
use diamond_stats_core::paths::{gamelog_file, partition_file};
use diamond_stats_core::{BattingStint, Level, NormalizeCtx, PitchingStint, StatGroup};
use tracing::info;

use crate::client::StatsApi;
use crate::error::IngestError;
use crate::fetch::fetch_all_splits;
use crate::parquet::{
    write_batting, write_batting_gamelog, write_pitching, write_pitching_gamelog, write_teams,
};
use crate::urls::{PAGE_LIMIT, PlayerQuery};

/// What to ingest.
#[derive(Debug, Clone)]
pub struct IngestOptions {
    /// Data directory. Cache and Parquet are created under it.
    pub data_dir: std::path::PathBuf,
    /// Season year.
    pub season: u16,
    /// Levels to pull. MLB through Rookie share this path.
    pub levels: Vec<Level>,
}

/// Rows written for one stat group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSummary {
    /// Hitting or pitching.
    pub group: StatGroup,
    /// Team stints written.
    pub stints: usize,
    /// Players whose sport-wide row was replaced by people-endpoint stints.
    pub hydrated_players: usize,
}

/// Rows written for one level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelSummary {
    /// Level.
    pub level: Level,
    /// Teams written.
    pub teams: usize,
    /// Hitting, then pitching.
    pub groups: Vec<GroupSummary>,
}

/// Result of an ingest run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestSummary {
    /// Season that was requested.
    pub season: u16,
    /// One entry per requested level.
    pub levels: Vec<LevelSummary>,
}

/// Fetch season stats and teams, and write Parquet partitions.
pub async fn ingest(
    api: &impl StatsApi,
    options: &IngestOptions,
) -> Result<IngestSummary, IngestError> {
    let mut levels = Vec::with_capacity(options.levels.len());
    for level in &options.levels {
        info!(season = options.season, %level, "ingesting level");
        let teams_envelope = api.teams(level.sport_id(), options.season).await?;
        let clubs = teams(&teams_envelope, options.season, *level)?;
        let team_path = partition_file(&options.data_dir, "teams", options.season, *level);
        write_teams(&team_path, &clubs)?;
        let hitting = ingest_batting(api, &options.data_dir, options.season, *level).await?;
        let pitching = ingest_pitching(api, &options.data_dir, options.season, *level).await?;
        info!(
            season = options.season,
            %level,
            teams = clubs.len(),
            hitting = hitting.stints,
            pitching = pitching.stints,
            "wrote season partitions"
        );
        levels.push(LevelSummary {
            level: *level,
            teams: clubs.len(),
            groups: vec![hitting, pitching],
        });
    }
    Ok(IngestSummary {
        season: options.season,
        levels,
    })
}

/// Fetch one player's game log and write it. Empty logs are still written.
pub async fn materialize_game_log(
    api: &impl StatsApi,
    data_dir: &Path,
    player_id: u32,
    season: u16,
    level: Level,
    group: StatGroup,
) -> Result<usize, IngestError> {
    let envelope = api
        .game_log(
            player_id,
            PlayerQuery {
                season,
                sport_id: level.sport_id(),
                group,
            },
        )
        .await?;
    let rows = if envelope.stats.is_empty() {
        Vec::new()
    } else {
        game_log(
            &envelope,
            NormalizeCtx {
                season,
                level,
                group,
            },
            player_id,
        )?
    };
    let path = gamelog_file(data_dir, group, season, level, player_id);
    match group {
        StatGroup::Hitting => write_batting_gamelog(&path, &rows)?,
        StatGroup::Pitching => write_pitching_gamelog(&path, &rows)?,
    }
    info!(
        player_id,
        season,
        %level,
        %group,
        games = rows.len(),
        "materialized game log"
    );
    Ok(rows.len())
}

async fn ingest_batting(
    api: &impl StatsApi,
    data_dir: &Path,
    season: u16,
    level: Level,
) -> Result<GroupSummary, IngestError> {
    let group = StatGroup::Hitting;
    let splits = fetch_all_splits(api, season, level.sport_id(), group, PAGE_LIMIT).await?;
    let hydrate = hydration_ids(&splits)?;
    let ctx = NormalizeCtx {
        season,
        level,
        group,
    };
    let mut rows = batting_stints(&splits, ctx)?;
    info!(
        %level,
        season,
        players = splits.len(),
        hydrate = hydrate.len(),
        "hydrating multi-team hitters"
    );
    for player_id in &hydrate {
        let envelope = api
            .player_season(
                *player_id,
                PlayerQuery {
                    season,
                    sport_id: level.sport_id(),
                    group,
                },
            )
            .await?;
        let block = envelope.stats.into_iter().next().ok_or_else(|| {
            IngestError::Message(format!(
                "player {player_id} season response had no hitting group"
            ))
        })?;
        let extra = batting_stints(&block.splits, ctx)?;
        if extra.is_empty() {
            return Err(IngestError::Message(format!(
                "player {player_id} had numTeams > 1 but the people endpoint returned no team stints"
            )));
        }
        rows.extend(extra);
    }
    let rows = dedup_batting(rows)?;
    let path = partition_file(data_dir, "batting", season, level);
    write_batting(&path, &rows)?;
    Ok(GroupSummary {
        group,
        stints: rows.len(),
        hydrated_players: hydrate.len(),
    })
}

async fn ingest_pitching(
    api: &impl StatsApi,
    data_dir: &Path,
    season: u16,
    level: Level,
) -> Result<GroupSummary, IngestError> {
    let group = StatGroup::Pitching;
    let splits = fetch_all_splits(api, season, level.sport_id(), group, PAGE_LIMIT).await?;
    let hydrate = hydration_ids(&splits)?;
    let ctx = NormalizeCtx {
        season,
        level,
        group,
    };
    let mut rows = pitching_stints(&splits, ctx)?;
    info!(
        %level,
        season,
        players = splits.len(),
        hydrate = hydrate.len(),
        "hydrating multi-team pitchers"
    );
    for player_id in &hydrate {
        let envelope = api
            .player_season(
                *player_id,
                PlayerQuery {
                    season,
                    sport_id: level.sport_id(),
                    group,
                },
            )
            .await?;
        let block = envelope.stats.into_iter().next().ok_or_else(|| {
            IngestError::Message(format!(
                "player {player_id} season response had no pitching group"
            ))
        })?;
        let extra = pitching_stints(&block.splits, ctx)?;
        if extra.is_empty() {
            return Err(IngestError::Message(format!(
                "player {player_id} had numTeams > 1 but the people endpoint returned no pitching stints"
            )));
        }
        rows.extend(extra);
    }
    let rows = dedup_pitching(rows)?;
    let path = partition_file(data_dir, "pitching", season, level);
    write_pitching(&path, &rows)?;
    Ok(GroupSummary {
        group,
        stints: rows.len(),
        hydrated_players: hydrate.len(),
    })
}

fn dedup_batting(rows: Vec<BattingStint>) -> Result<Vec<BattingStint>, IngestError> {
    let mut out: Vec<BattingStint> = Vec::new();
    let mut index: HashMap<(u32, u32), usize> = HashMap::new();
    for row in rows {
        let key = (row.player_id, row.team_id);
        if let Some(existing) = index.get(&key).copied() {
            let previous: &BattingStint = &out[existing];
            if previous.hits != row.hits
                || previous.at_bats != row.at_bats
                || previous.home_runs != row.home_runs
            {
                return Err(IngestError::Message(format!(
                    "conflicting batting stints for player {} team {}",
                    row.player_id, row.team_id
                )));
            }
            continue;
        }
        index.insert(key, out.len());
        out.push(row);
    }
    Ok(out)
}

fn dedup_pitching(rows: Vec<PitchingStint>) -> Result<Vec<PitchingStint>, IngestError> {
    let mut out: Vec<PitchingStint> = Vec::new();
    let mut index: HashMap<(u32, u32), usize> = HashMap::new();
    for row in rows {
        let key = (row.player_id, row.team_id);
        if let Some(existing) = index.get(&key).copied() {
            let previous: &PitchingStint = &out[existing];
            if previous.outs != row.outs || previous.earned_runs != row.earned_runs {
                return Err(IngestError::Message(format!(
                    "conflicting pitching stints for player {} team {}",
                    row.player_id, row.team_id
                )));
            }
            continue;
        }
        index.insert(key, out.len());
        out.push(row);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::FixtureClient;
    use crate::urls::SeasonPageQuery;
    use diamond_stats_core::api_model::{StatsEnvelope, TeamsEnvelope};
    use std::path::PathBuf;

    fn core_fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../core/fixtures")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    }

    fn stats(name: &str) -> StatsEnvelope {
        serde_json::from_str(&core_fixture(name)).unwrap()
    }

    #[tokio::test]
    async fn fixture_ingest_hydrates_naylor_and_writes_partitions() {
        let mut hitting = stats("hitting_raleigh.json");
        let naylor = stats("hitting_naylor_sportwide.json");
        hitting.stats[0]
            .splits
            .push(naylor.stats[0].splits[0].clone());
        hitting.stats[0].total_splits = Some(2);

        let mut pitching = stats("pitching_skenes.json");
        let acton = stats("pitching_acton.json");
        pitching.stats[0]
            .splits
            .push(acton.stats[0].splits[0].clone());
        pitching.stats[0].total_splits = Some(2);

        let client = FixtureClient::new();
        client.insert_season_page(
            SeasonPageQuery {
                season: 2025,
                sport_id: Level::Mlb.sport_id(),
                group: StatGroup::Hitting,
                limit: PAGE_LIMIT,
                offset: 0,
            },
            hitting,
        );
        client.insert_season_page(
            SeasonPageQuery {
                season: 2025,
                sport_id: Level::Mlb.sport_id(),
                group: StatGroup::Pitching,
                limit: PAGE_LIMIT,
                offset: 0,
            },
            pitching,
        );
        client.insert_player_season(
            647_304,
            PlayerQuery {
                season: 2025,
                sport_id: Level::Mlb.sport_id(),
                group: StatGroup::Hitting,
            },
            stats("naylor_people_hitting.json"),
        );
        let teams_doc: TeamsEnvelope =
            serde_json::from_str(&core_fixture("teams_mlb_trimmed.json")).unwrap();
        client.insert_teams(Level::Mlb.sport_id(), teams_doc);

        let dir = tempfile::tempdir().unwrap();
        let summary = ingest(
            &client,
            &IngestOptions {
                data_dir: dir.path().to_path_buf(),
                season: 2025,
                levels: vec![Level::Mlb],
            },
        )
        .await
        .unwrap();

        assert_eq!(summary.levels[0].teams, 4);
        assert_eq!(summary.levels[0].groups[0].stints, 3);
        assert_eq!(summary.levels[0].groups[0].hydrated_players, 1);
        assert_eq!(summary.levels[0].groups[1].stints, 2);
        assert_eq!(summary.levels[0].groups[1].hydrated_players, 0);
        assert!(partition_file(dir.path(), "batting", 2025, Level::Mlb).is_file());
        assert!(partition_file(dir.path(), "pitching", 2025, Level::Mlb).is_file());

        client.insert_game_log(
            592_450,
            PlayerQuery {
                season: 2025,
                sport_id: Level::Mlb.sport_id(),
                group: StatGroup::Hitting,
            },
            stats("judge_gamelog_two_games.json"),
        );
        let games = materialize_game_log(
            &client,
            dir.path(),
            592_450,
            2025,
            Level::Mlb,
            StatGroup::Hitting,
        )
        .await
        .unwrap();
        assert_eq!(games, 2);
        assert!(gamelog_file(dir.path(), StatGroup::Hitting, 2025, Level::Mlb, 592_450).is_file());
    }
}
