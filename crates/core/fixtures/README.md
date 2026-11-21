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
| `hitting_aa_one.json`, `hitting_high_a_one.json`, `hitting_single_a_one.json`, `hitting_rookie_one.json` | `limit=1` season hitting pages for sportIds 12, 13, 14, and 16, recorded in full. `totalSplits` is the season population, not the number of rows in the file. |
| `teams_rookie_trimmed.json` | Two 2025 Rookie clubs, one Florida Complex League and one Dominican Summer League. Objects are unmodified. |
| `schedule_mlb_2025-06-21_two.json` | 2025-06-21 MLB schedule, first two games. Game objects are unmodified. The totals were rewritten to 2. |
| `schedule_high_a_2025-06-21_final_and_postponed.json` | One final High-A game and postponed game 789134 from 2025-06-21. Game objects are unmodified. Totals were rewritten to 2. |
| `boxscore_mlb_777420_trimmed.json`, `boxscore_aa_782551_trimmed.json` | One batter and one pitcher per club. `seasonStats` and the other players were removed. The kept game lines are unmodified. |
| `playbyplay_mlb_777420_one_pa.json`, `playbyplay_aa_782551_one_pa.json` | The first plate appearance and its first three pitches. `pitchData` keeps speed and the strike-zone edges; coordinates and break charts were removed. The Double-A pitches had no pitch type and no `startSpeed` in the original feed. |

`crates/ingest/fixtures/hit-limit5.json`, `hit-off5.json`, and `hit-off10.json` are three recorded pages (`limit=5`) of 2025 MLB hitting. Tests use them to show that `splitsTiedWithLimit` / `splitsTiedWithOffset` repeat the tie group and must not be appended.
