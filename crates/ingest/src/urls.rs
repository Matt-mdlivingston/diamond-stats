//! Stats API URLs. `playerPool=all` is required: the default pool is qualified
//! players only. `gameType=R` keeps the regular season.

use diamond_stats_core::StatGroup;

/// Page size for `/api/v1/stats`. The historical cap cited by MLB is 1000.
pub const PAGE_LIMIT: u32 = 500;

/// Descriptive User-Agent. The service is individual and non-commercial.
pub const USER_AGENT: &str = "diamond-stats/0.1 (+https://github.com/Matt-mdlivingston/diamond-stats; individual non-commercial research)";

/// Query for one page of season stats.
#[derive(Debug, Clone, Copy)]
pub struct SeasonPageQuery {
    /// Season year.
    pub season: u16,
    /// `sportId`.
    pub sport_id: u32,
    /// Hitting or pitching.
    pub group: StatGroup,
    /// Page size.
    pub limit: u32,
    /// Zero-based offset.
    pub offset: u32,
}

/// Query for one player's season line or game log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerQuery {
    /// Season year.
    pub season: u16,
    /// `sportId`.
    pub sport_id: u32,
    /// Hitting or pitching.
    pub group: StatGroup,
}

/// `GET /api/v1/stats` for a season page.
pub fn season_stats_url(base: &str, query: SeasonPageQuery) -> String {
    format!(
        "{}/api/v1/stats?stats=season&group={}&season={}&sportId={}&playerPool=all&gameType=R&limit={}&offset={}",
        trim_base(base),
        query.group,
        query.season,
        query.sport_id,
        query.limit,
        query.offset
    )
}

/// `GET /api/v1/people/{id}/stats` for a season, which includes per-team stints.
pub fn player_season_url(base: &str, player_id: u32, query: PlayerQuery) -> String {
    format!(
        "{}/api/v1/people/{player_id}/stats?stats=season&group={}&season={}&sportId={}&gameType=R",
        trim_base(base),
        query.group,
        query.season,
        query.sport_id
    )
}

/// `GET /api/v1/people/{id}/stats` for a game log.
pub fn game_log_url(base: &str, player_id: u32, query: PlayerQuery) -> String {
    format!(
        "{}/api/v1/people/{player_id}/stats?stats=gameLog&group={}&season={}&sportId={}&gameType=R",
        trim_base(base),
        query.group,
        query.season,
        query.sport_id
    )
}

/// `GET /api/v1/schedule` for one sport and official schedule date.
pub fn schedule_date_url(base: &str, sport_id: u32, date: &str) -> String {
    format!(
        "{}/api/v1/schedule?sportId={sport_id}&date={date}&gameType=R",
        trim_base(base)
    )
}

/// `GET /api/v1/schedule?gamePk=` for one game. The payload does not include `sportId`.
pub fn schedule_game_url(base: &str, game_pk: u64) -> String {
    format!("{}/api/v1/schedule?gamePk={game_pk}", trim_base(base))
}

/// `GET /api/v1/game/{gamePk}/boxscore`.
pub fn boxscore_url(base: &str, game_pk: u64) -> String {
    format!("{}/api/v1/game/{game_pk}/boxscore", trim_base(base))
}

/// `GET /api/v1/game/{gamePk}/playByPlay`.
pub fn play_by_play_url(base: &str, game_pk: u64) -> String {
    format!("{}/api/v1/game/{game_pk}/playByPlay", trim_base(base))
}

/// `GET /api/v1/teams?teamId=` so a gamePk lookup can learn the club's sport.
pub fn team_url(base: &str, team_id: u32, season: u16) -> String {
    format!(
        "{}/api/v1/teams?teamId={team_id}&season={season}",
        trim_base(base)
    )
}

/// `GET /api/v1/teams`.
pub fn teams_url(base: &str, sport_id: u32, season: u16) -> String {
    format!(
        "{}/api/v1/teams?sportId={sport_id}&season={season}",
        trim_base(base)
    )
}

fn trim_base(base: &str) -> &str {
    base.trim_end_matches('/')
}

/// Whether an HTTP status is worth another try.
pub fn should_retry_status(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

/// Exponential backoff for attempt `n` (0-based), capped at 8 seconds.
pub fn backoff(attempt: u32) -> std::time::Duration {
    let millis = 250_u64.saturating_mul(1_u64 << attempt.min(5));
    std::time::Duration::from_millis(millis.min(8_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn season_url_requests_every_player_in_the_regular_season() {
        let url = season_stats_url(
            "https://statsapi.mlb.com/",
            SeasonPageQuery {
                season: 2025,
                sport_id: 11,
                group: StatGroup::Hitting,
                limit: PAGE_LIMIT,
                offset: 500,
            },
        );
        assert!(url.contains("playerPool=all"));
        assert!(url.contains("gameType=R"));
        assert!(url.contains("sportId=11"));
        assert!(url.contains("season=2025"));
        assert!(url.contains("group=hitting"));
        assert!(url.contains("limit=500"));
        assert!(url.contains("offset=500"));
        assert!(!url.contains(".com//"));
    }

    #[test]
    fn retry_policy_is_limited_to_throttle_and_server_errors() {
        assert!(should_retry_status(429));
        assert!(should_retry_status(503));
        assert!(!should_retry_status(404));
        assert!(!should_retry_status(400));
        assert!(backoff(0).as_millis() >= 250);
        assert!(backoff(10).as_millis() <= 8_000);
    }

    #[test]
    fn game_urls_name_the_schedule_box_and_plays() {
        assert_eq!(
            schedule_date_url("https://statsapi.mlb.com", 12, "2025-06-21"),
            "https://statsapi.mlb.com/api/v1/schedule?sportId=12&date=2025-06-21&gameType=R"
        );
        assert!(schedule_game_url("https://statsapi.mlb.com", 777_420).ends_with("gamePk=777420"));
        assert!(
            boxscore_url("https://statsapi.mlb.com/", 777_420).ends_with("/game/777420/boxscore")
        );
        assert!(
            play_by_play_url("https://statsapi.mlb.com", 782_551)
                .ends_with("/game/782551/playByPlay")
        );
        assert!(team_url("https://statsapi.mlb.com", 116, 2025).contains("teamId=116"));
        assert!(team_url("https://statsapi.mlb.com", 116, 2025).contains("season=2025"));
    }
}
