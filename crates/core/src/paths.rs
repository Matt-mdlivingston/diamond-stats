//! On-disk layout shared by the writer and the query layer.
//!
//! Raw API cache and Parquet output both live under the data directory and
//! must stay gitignored. Nothing here publishes a bulk dataset.

use std::path::{Path, PathBuf};

use crate::group::StatGroup;
use crate::level::Level;

/// Root of the raw JSON cache.
pub fn cache_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("cache").join("statsapi")
}

/// Root of normalized Parquet datasets.
pub fn parquet_root(data_dir: &Path) -> PathBuf {
    data_dir.join("parquet")
}

/// Dataset directory name for a stat group.
pub fn dataset_name(group: StatGroup) -> &'static str {
    match group {
        StatGroup::Hitting => "batting",
        StatGroup::Pitching => "pitching",
    }
}

/// Dataset directory name for game logs.
pub fn gamelog_dataset_name(group: StatGroup) -> &'static str {
    match group {
        StatGroup::Hitting => "gamelog_batting",
        StatGroup::Pitching => "gamelog_pitching",
    }
}

/// Partition directory: `{dataset}/season={year}/level={slug}`.
pub fn partition_dir(data_dir: &Path, dataset: &str, season: u16, level: Level) -> PathBuf {
    parquet_root(data_dir)
        .join(dataset)
        .join(format!("season={season}"))
        .join(format!("level={}", level.slug()))
}

/// Single file written for a season/level partition.
pub fn partition_file(data_dir: &Path, dataset: &str, season: u16, level: Level) -> PathBuf {
    partition_dir(data_dir, dataset, season, level).join("part-0.parquet")
}

/// One player's lazily materialized game log.
pub fn gamelog_file(
    data_dir: &Path,
    group: StatGroup,
    season: u16,
    level: Level,
    player_id: u32,
) -> PathBuf {
    partition_dir(data_dir, gamelog_dataset_name(group), season, level)
        .join(format!("player={player_id}.parquet"))
}

/// DuckDB glob for every file of a dataset. The path is produced from the
/// configured data directory, never from a request string.
pub fn dataset_glob(data_dir: &Path, dataset: &str) -> PathBuf {
    parquet_root(data_dir)
        .join(dataset)
        .join("season=*")
        .join("level=*")
        .join("*.parquet")
}

/// True when a path can be inlined as a SQL string literal.
pub fn is_safe_sql_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\'')
        && !path.contains('"')
        && !path.contains(';')
        && !path.contains('\0')
        && !path.contains("--")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_partitioned_by_season_and_level() {
        let root = Path::new("/data");
        let file = partition_file(root, "batting", 2025, Level::Mlb);
        assert_eq!(
            file,
            PathBuf::from("/data/parquet/batting/season=2025/level=mlb/part-0.parquet")
        );
        let gamelog = gamelog_file(root, StatGroup::Pitching, 2025, Level::Aaa, 694973);
        assert_eq!(
            gamelog,
            PathBuf::from(
                "/data/parquet/gamelog_pitching/season=2025/level=aaa/player=694973.parquet"
            )
        );
    }

    #[test]
    fn quotes_are_rejected_for_sql_paths() {
        assert!(is_safe_sql_path(
            "/data/parquet/batting/season=*/level=*/*.parquet"
        ));
        assert!(!is_safe_sql_path("/tmp/it's.parquet"));
    }
}
