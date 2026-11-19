# Test fixtures

These files are **trimmed excerpts of real MLB Stats API responses**, saved so unit tests can normalize and query without the network. They are not a data product and they are not a bulk download.

Retrieved 2026-11-19 with `User-Agent: diamond-stats/0.1` from `https://statsapi.mlb.com`. Every file still carries the MLBAM copyright string that the API returned.

| File | What was kept |
| --- | --- |
| `sports_trimmed.json` | `/api/v1/sports`, filtered to sport ids 1, 11, 12, 13, 14, 16, and 21. Objects are unmodified. |
| `sport15_hitting_empty.json` | Season hitting for sportId 15 (inactive). `totalSplits` is 0. Recorded in full. |
| `sport21_hitting_empty.json` | Season hitting for sportId 21 ("Minor League Baseball"). `totalSplits` is 0. Recorded in full. |
| `teams_mlb_trimmed.json` | `/api/v1/teams?sportId=1&season=2025`, four clubs. |
| `teams_aaa_trimmed.json` | `/api/v1/teams?sportId=11&season=2025`, two clubs. |
| `hitting_*.json`, `pitching_*.json` | `/api/v1/stats?stats=season&...&playerPool=all&gameType=R`, one split each. |
| `naylor_people_hitting.json` | `/api/v1/people/647304/stats?stats=season&group=hitting&season=2025&sportId=1&gameType=R`. One player, recorded in full. |
| `rogers_people_pitching.json` | `/api/v1/people/643511/stats?stats=season&group=pitching&season=2025&sportId=1&gameType=R`. One player, recorded in full. |
| `judge_gamelog_two_games.json` | Aaron Judge's 2025 MLB hitting game log, first two games of 152. |
| `skenes_gamelog_two_games.json` | Paul Skenes's 2025 MLB pitching game log, first two starts of 32. |

`crates/ingest/fixtures/hit-limit5.json`, `hit-off5.json`, and `hit-off10.json` are three recorded pages (`limit=5`) of 2025 MLB hitting. Tests use them to show that `splitsTiedWithLimit` / `splitsTiedWithOffset` repeat the tie group and must not be appended.
