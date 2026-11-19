//! Pagination over `/api/v1/stats`.
//!
//! Only the `splits` array is kept. `splitsTiedWithOffset` and
//! `splitsTiedWithLimit` repeat players who sit on a tie at the page boundary;
//! the next or previous page already includes them. After the walk, the number
//! of unique player rows must equal `totalSplits`.

use std::collections::HashSet;

use diamond_stats_core::StatGroup;
use diamond_stats_core::api_model::StatSplit;

use crate::client::StatsApi;
use crate::error::IngestError;
use crate::urls::SeasonPageQuery;

/// Every season split for one group, sport, and season.
pub async fn fetch_all_splits(
    api: &impl StatsApi,
    season: u16,
    sport_id: u32,
    group: StatGroup,
    page_limit: u32,
) -> Result<Vec<StatSplit>, IngestError> {
    let mut offset = 0_u32;
    let mut rows = Vec::new();
    let mut seen: HashSet<(u32, u32, u32)> = HashSet::new();
    let mut expected: Option<u32> = None;

    loop {
        let envelope = api
            .season_page(SeasonPageQuery {
                season,
                sport_id,
                group,
                limit: page_limit,
                offset,
            })
            .await?;
        let block = envelope.stats.into_iter().next().ok_or_else(|| {
            IngestError::Message(format!(
                "season stats response had no group (sport {sport_id} {group} offset {offset})"
            ))
        })?;
        if expected.is_none() {
            expected = block.total_splits;
        }
        let page_len = block.splits.len();
        if page_len == 0 {
            break;
        }
        for split in block.splits {
            let player_id = split
                .player
                .as_ref()
                .map(|player| player.id)
                .ok_or_else(|| IngestError::Message("season split missing player id".into()))?;
            let team_id = split.team.as_ref().map(|team| team.id).unwrap_or(0);
            let teams = split.num_teams.unwrap_or(0);
            if seen.insert((player_id, team_id, teams)) {
                rows.push(split);
            }
        }
        offset = offset.saturating_add(page_limit);
        if page_len < page_limit as usize {
            break;
        }
        if expected.is_some_and(|total| offset >= total || rows.len() as u32 >= total) {
            break;
        }
        if offset > 50_000 {
            return Err(IngestError::Message(format!(
                "refusing to page past offset {offset} for sport {sport_id} {group}"
            )));
        }
    }

    if let Some(total) = expected
        && rows.len() as u32 != total
    {
        return Err(IngestError::Pagination {
            group: group.to_string(),
            sport_id,
            season,
            expected: total,
            got: rows.len(),
        });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::FixtureClient;
    use diamond_stats_core::Level;
    use diamond_stats_core::api_model::StatsEnvelope;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn load_page(name: &str) -> StatsEnvelope {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        let text = std::fs::read_to_string(&path).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[tokio::test]
    async fn pages_keep_splits_only_and_do_not_duplicate_ties() {
        // Recorded pages of the 2025 MLB hitting pool, limit 5.
        // `totalSplits` on the wire is 765. The test rewrites it to 15 so the
        // paginator treats these three recorded pages as the whole population.
        let mut pages = [
            load_page("hit-limit5.json"),
            load_page("hit-off5.json"),
            load_page("hit-off10.json"),
        ];
        for (index, page) in pages.iter_mut().enumerate() {
            page.stats[0].total_splits = Some(15);
            if index == 2 {
                // The third recorded page is also a full limit, which would
                // make a real crawl continue. Mark it short via totalSplits.
            }
        }
        // The third page returns 5 rows and limit is 500 in production, so
        // `page_len < PAGE_LIMIT` stops the loop. totalSplits is also 15.
        let client = FixtureClient::new();
        for (offset, page) in [0_u32, 5, 10].into_iter().zip(pages) {
            client.insert_season_page(
                SeasonPageQuery {
                    season: 2025,
                    sport_id: Level::Mlb.sport_id(),
                    group: StatGroup::Hitting,
                    limit: 5,
                    offset,
                },
                page,
            );
        }

        let rows = fetch_all_splits(&client, 2025, Level::Mlb.sport_id(), StatGroup::Hitting, 5)
            .await
            .unwrap();
        let ids: Vec<u32> = rows
            .iter()
            .map(|split| split.player.as_ref().unwrap().id)
            .collect();
        assert_eq!(ids.len(), 15);
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 15);
        // Loperfido is in page 2's splitsTiedWithLimit and page 3's splits.
        assert_eq!(ids.iter().filter(|id| **id == 694_388).count(), 1);
        // Cook is in page 2's splits and page 3's splitsTiedWithOffset.
        assert_eq!(ids.iter().filter(|id| **id == 695_257).count(), 1);
        let calls = client.calls();
        assert_eq!(calls.len(), 3, "{calls:?}");
    }
}
