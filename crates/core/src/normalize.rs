//! Turn API models into domain rows.
//!
//! The sport-wide season endpoint returns one row per player. When `numTeams`
//! is greater than 1, the counting stats are the whole season and `team` is
//! only one of the clubs (observed: Josh Naylor's 147-game 2025 line is labeled
//! Seattle). Those rows are not stints. The people endpoint returns the combined
//! row with no team, plus one row per club. This module keeps only rows that
//! have a team and are not flagged as multi-team aggregates.

use crate::api_model::{StatSplit, StatsEnvelope, TeamsEnvelope};
use crate::domain::{BattingStint, GameLogEntry, PitchingStint, Team};
use crate::error::CoreError;
use crate::group::StatGroup;
use crate::level::Level;
use crate::rates::Innings;

/// What to do with one split from a season-stats response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDisposition {
    /// A single-team counting line. Safe to store and to put on a roster.
    Stint,
    /// Sport-wide row covering more than one team. Fetch the people endpoint.
    Hydrate,
    /// Combined season total with no team. Drop it; the stints are separate rows.
    Ignore,
}

/// Season and level the caller asked for. Used to reject mixed payloads.
#[derive(Debug, Clone, Copy)]
pub struct NormalizeCtx {
    /// Season that was requested.
    pub season: u16,
    /// Level that was requested.
    pub level: Level,
    /// Hitting or pitching.
    pub group: StatGroup,
}

/// Classify a season split.
pub fn classify_split(split: &StatSplit) -> Result<SplitDisposition, CoreError> {
    let player_id = player_id(split)?;
    let teams = split.num_teams.unwrap_or(1);
    if teams > 1 && split.team.is_some() {
        return Ok(SplitDisposition::Hydrate);
    }
    if split.team.is_none() {
        let _ = player_id;
        return Ok(SplitDisposition::Ignore);
    }
    Ok(SplitDisposition::Stint)
}

/// Player ids whose sport-wide row is a multi-team aggregate.
pub fn hydration_ids(splits: &[StatSplit]) -> Result<Vec<u32>, CoreError> {
    let mut ids = Vec::new();
    for split in splits {
        if classify_split(split)? == SplitDisposition::Hydrate {
            let id = player_id(split)?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    Ok(ids)
}

/// Batting stints in `splits`. Aggregates are skipped, not converted.
pub fn batting_stints(
    splits: &[StatSplit],
    ctx: NormalizeCtx,
) -> Result<Vec<BattingStint>, CoreError> {
    let mut rows = Vec::new();
    for split in splits {
        if classify_split(split)? == SplitDisposition::Stint {
            rows.push(batting_stint(split, ctx)?);
        }
    }
    Ok(rows)
}

/// Pitching stints in `splits`. Aggregates are skipped, not converted.
pub fn pitching_stints(
    splits: &[StatSplit],
    ctx: NormalizeCtx,
) -> Result<Vec<PitchingStint>, CoreError> {
    let mut rows = Vec::new();
    for split in splits {
        if classify_split(split)? == SplitDisposition::Stint {
            rows.push(pitching_stint(split, ctx)?);
        }
    }
    Ok(rows)
}

/// Game-log rows from a people game-log envelope.
///
/// Counting stats are stored. The API rate fields are ignored because on this
/// endpoint they are season-to-date through the game.
pub fn game_log(
    envelope: &StatsEnvelope,
    ctx: NormalizeCtx,
    player_id_expected: u32,
) -> Result<Vec<GameLogEntry>, CoreError> {
    let splits = only_block(envelope)?;
    if let Some(total) = splits.total_splits
        && (splits.splits.len() as u32) < total
    {
        return Err(CoreError::Message(format!(
            "game log for player {player_id_expected} was truncated ({}/{})",
            splits.splits.len(),
            total
        )));
    }
    let mut rows = Vec::with_capacity(splits.splits.len());
    for split in &splits.splits {
        rows.push(game_log_entry(split, ctx, player_id_expected)?);
    }
    Ok(rows)
}

/// Teams for one season and level.
pub fn teams(envelope: &TeamsEnvelope, season: u16, level: Level) -> Result<Vec<Team>, CoreError> {
    envelope
        .teams
        .iter()
        .map(|team| {
            if let Some(team_season) = team.season
                && team_season != season
            {
                return Err(CoreError::Message(format!(
                    "team {} season {team_season} does not match requested {season}",
                    team.id
                )));
            }
            if let Some(sport_id) = team.sport.as_ref().and_then(|sport| sport.id)
                && sport_id != level.sport_id()
            {
                return Err(CoreError::Message(format!(
                    "team {} sportId {sport_id} does not match {}",
                    team.id,
                    level.sport_id()
                )));
            }
            Ok(Team {
                team_id: team.id,
                season,
                level,
                name: team.name.clone(),
                abbreviation: team.abbreviation.clone(),
                team_name: team.team_name.clone(),
                location_name: team.location_name.clone(),
                league_name: team.league.as_ref().and_then(|league| league.name.clone()),
                division_name: team
                    .division
                    .as_ref()
                    .and_then(|division| division.name.clone()),
                parent_org_id: team.parent_org_id,
                parent_org_name: team.parent_org_name.clone(),
            })
        })
        .collect()
}

fn batting_stint(split: &StatSplit, ctx: NormalizeCtx) -> Result<BattingStint, CoreError> {
    check_context(split, ctx)?;
    let stat = &split.stat;
    let (team_id, team_name) = team_identity(split)?;
    let mut row = BattingStint {
        season: ctx.season,
        level: ctx.level,
        player_id: player_id(split)?,
        player_name: player_name(split)?,
        team_id,
        team_name,
        position: position_abbr(split),
        games: stat.games_played.0,
        at_bats: stat.at_bats.0,
        plate_appearances: stat.plate_appearances.0,
        runs: stat.runs.0,
        hits: stat.hits.0,
        doubles: stat.doubles.0,
        triples: stat.triples.0,
        home_runs: stat.home_runs.0,
        rbi: stat.rbi.0,
        walks: stat.base_on_balls.0,
        intentional_walks: stat.intentional_walks.0,
        strikeouts: stat.strike_outs.0,
        hit_by_pitch: stat.hit_by_pitch.0,
        stolen_bases: stat.stolen_bases.0,
        caught_stealing: stat.caught_stealing.0,
        sac_flies: stat.sac_flies.0,
        sac_bunts: stat.sac_bunts.0,
        total_bases: stat.total_bases.0,
        gidp: stat.ground_into_double_play.0,
        avg: None,
        obp: None,
        slg: None,
        ops: None,
        api_avg: stat.avg.0,
        api_obp: stat.obp.0,
        api_slg: stat.slg.0,
        api_ops: stat.ops.0,
    };
    row.recompute_rates();
    Ok(row)
}

fn pitching_stint(split: &StatSplit, ctx: NormalizeCtx) -> Result<PitchingStint, CoreError> {
    check_context(split, ctx)?;
    let stat = &split.stat;
    let (team_id, team_name) = team_identity(split)?;
    let outs = resolve_outs(stat)?;
    let games = if stat.games_played.0 > 0 {
        stat.games_played.0
    } else {
        stat.games_pitched.0
    };
    let mut row = PitchingStint {
        season: ctx.season,
        level: ctx.level,
        player_id: player_id(split)?,
        player_name: player_name(split)?,
        team_id,
        team_name,
        position: position_abbr(split),
        games,
        games_started: stat.games_started.0,
        wins: stat.wins.0,
        losses: stat.losses.0,
        saves: stat.saves.0,
        holds: stat.holds.0,
        blown_saves: stat.blown_saves.0,
        save_opportunities: stat.save_opportunities.0,
        complete_games: stat.complete_games.0,
        shutouts: stat.shutouts.0,
        outs,
        innings_pitched: stat
            .innings_pitched
            .clone()
            .unwrap_or_else(|| Innings::from_outs(outs).notation()),
        hits: stat.hits.0,
        runs: stat.runs.0,
        earned_runs: stat.earned_runs.0,
        home_runs: stat.home_runs.0,
        walks: stat.base_on_balls.0,
        intentional_walks: stat.intentional_walks.0,
        strikeouts: stat.strike_outs.0,
        hit_batters: stat.hit_batsmen.0,
        balks: stat.balks.0,
        wild_pitches: stat.wild_pitches.0,
        batters_faced: stat.batters_faced.0,
        strikes: stat.strikes.0,
        number_of_pitches: stat.number_of_pitches.0,
        era: None,
        whip: None,
        api_era: stat.era.0,
        api_whip: stat.whip.0,
    };
    row.recompute_rates();
    Ok(row)
}

fn game_log_entry(
    split: &StatSplit,
    ctx: NormalizeCtx,
    expected_player: u32,
) -> Result<GameLogEntry, CoreError> {
    check_context(split, ctx)?;
    let player = player_id(split)?;
    if player != expected_player {
        return Err(CoreError::Message(format!(
            "game log player {player} does not match requested {expected_player}"
        )));
    }
    let game_pk = split
        .game
        .as_ref()
        .map(|game| game.game_pk)
        .ok_or_else(|| CoreError::Message(format!("game log for {player} is missing gamePk")))?;
    let game_date = split
        .date
        .clone()
        .ok_or_else(|| CoreError::Message(format!("game log {game_pk} is missing a date")))?;
    let (team_id, team_name) = team_identity(split)?;
    let stat = &split.stat;
    let outs = if ctx.group == StatGroup::Pitching {
        resolve_outs(stat)?
    } else {
        0
    };
    let mut row = GameLogEntry {
        season: ctx.season,
        level: ctx.level,
        group: ctx.group,
        player_id: player,
        player_name: player_name(split)?,
        game_pk,
        game_date,
        team_id,
        team_name,
        opponent_id: split.opponent.as_ref().map(|team| team.id),
        opponent_name: split.opponent.as_ref().and_then(|team| team.name.clone()),
        is_home: split.is_home,
        is_win: split.is_win,
        position: position_abbr(split),
        summary: stat.summary.clone(),
        at_bats: stat.at_bats.0,
        plate_appearances: stat.plate_appearances.0,
        runs: stat.runs.0,
        hits: stat.hits.0,
        doubles: stat.doubles.0,
        triples: stat.triples.0,
        home_runs: stat.home_runs.0,
        rbi: stat.rbi.0,
        walks: stat.base_on_balls.0,
        strikeouts: stat.strike_outs.0,
        hit_by_pitch: if ctx.group == StatGroup::Pitching {
            stat.hit_batsmen.0
        } else {
            stat.hit_by_pitch.0
        },
        total_bases: stat.total_bases.0,
        sac_flies: stat.sac_flies.0,
        outs,
        innings_pitched: if ctx.group == StatGroup::Pitching {
            stat.innings_pitched
                .clone()
                .unwrap_or_else(|| Innings::from_outs(outs).notation())
        } else {
            String::new()
        },
        earned_runs: stat.earned_runs.0,
        avg: None,
        era: None,
        whip: None,
    };
    row.recompute_rates();
    Ok(row)
}

fn only_block(envelope: &StatsEnvelope) -> Result<&crate::api_model::StatsBlock, CoreError> {
    envelope
        .stats
        .first()
        .ok_or_else(|| CoreError::Message("stats response had no stats group".into()))
}

fn player_id(split: &StatSplit) -> Result<u32, CoreError> {
    split
        .player
        .as_ref()
        .map(|player| player.id)
        .ok_or_else(|| CoreError::Message("stat split is missing a player id".into()))
}

fn player_name(split: &StatSplit) -> Result<String, CoreError> {
    split
        .player
        .as_ref()
        .map(crate::api_model::PlayerRef::display_name)
        .ok_or_else(|| CoreError::Message("stat split is missing a player".into()))
}

fn team_identity(split: &StatSplit) -> Result<(u32, String), CoreError> {
    let team = split
        .team
        .as_ref()
        .ok_or_else(|| CoreError::Message("stint is missing a team".into()))?;
    let name = team
        .name
        .clone()
        .unwrap_or_else(|| format!("team {}", team.id));
    Ok((team.id, name))
}

fn position_abbr(split: &StatSplit) -> Option<String> {
    split
        .positions_played
        .iter()
        .find_map(|position| position.abbreviation.clone())
        .or_else(|| {
            split
                .position
                .as_ref()
                .and_then(|position| position.abbreviation.clone())
        })
}

fn check_context(split: &StatSplit, ctx: NormalizeCtx) -> Result<(), CoreError> {
    if let Some(season) = split.season.as_deref() {
        let parsed: u16 = season.parse().map_err(|_| {
            CoreError::Message(format!("could not parse season '{season}' as a year"))
        })?;
        if parsed != ctx.season {
            return Err(CoreError::Message(format!(
                "split season {parsed} does not match requested {}",
                ctx.season
            )));
        }
    }
    if let Some(sport_id) = split.sport.as_ref().and_then(|sport| sport.id)
        && sport_id != ctx.level.sport_id()
    {
        return Err(CoreError::Message(format!(
            "split sportId {sport_id} does not match requested {}",
            ctx.level.sport_id()
        )));
    }
    Ok(())
}

fn resolve_outs(stat: &crate::api_model::RawStat) -> Result<u32, CoreError> {
    let from_field = stat.outs.0;
    let from_notation = match &stat.innings_pitched {
        Some(raw) => Some(Innings::parse(raw)?.outs()),
        None => None,
    };
    match (from_field, from_notation) {
        (outs, Some(parsed)) if outs != 0 && outs != parsed => Err(CoreError::Message(format!(
            "outs {outs} does not match innings pitched '{}'",
            stat.innings_pitched.as_deref().unwrap_or("")
        ))),
        (0, Some(parsed)) => Ok(parsed),
        (outs, _) => Ok(outs),
    }
}

/// Absolute difference allowed when comparing a recomputed rate to a rounded API value.
pub fn rate_tolerance(decimal_places: u32) -> f64 {
    10_f64.powi(-(decimal_places as i32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_model::{SportsEnvelope, StatsEnvelope, TeamsEnvelope};
    use crate::rates::{self, Innings};
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
    }

    fn envelope(name: &str) -> StatsEnvelope {
        serde_json::from_str(&fixture(name)).unwrap_or_else(|err| panic!("parse {name}: {err}"))
    }

    fn mlb_hitting() -> NormalizeCtx {
        NormalizeCtx {
            season: 2025,
            level: Level::Mlb,
            group: StatGroup::Hitting,
        }
    }

    fn mlb_pitching() -> NormalizeCtx {
        NormalizeCtx {
            season: 2025,
            level: Level::Mlb,
            group: StatGroup::Pitching,
        }
    }

    fn assert_close(actual: f64, expected: f64, places: u32) {
        let tol = rate_tolerance(places);
        assert!(
            (actual - expected).abs() <= tol + 1e-9,
            "{actual} vs {expected} (tol {tol})"
        );
    }

    #[test]
    fn sports_fixture_matches_the_verified_level_map() {
        let sports: SportsEnvelope = serde_json::from_str(&fixture("sports_trimmed.json")).unwrap();
        let ids: Vec<u32> = sports.sports.iter().map(|sport| sport.id).collect();
        assert!(sports.copyright.contains("MLB Advanced Media"));
        assert!(!ids.contains(&15), "sportId 15 was not active");
        assert!(ids.contains(&21));
        let minors = sports.sports.iter().find(|sport| sport.id == 21).unwrap();
        assert_eq!(minors.code, "min");
        for level in Level::ALL {
            let sport = sports
                .sports
                .iter()
                .find(|sport| sport.id == level.sport_id())
                .unwrap_or_else(|| panic!("missing {}", level.slug()));
            assert_eq!(sport.code, level.info().api_code);
            assert!(sport.active_status);
        }
    }

    #[test]
    fn empty_sport_21_and_15_have_no_hitting_splits() {
        for name in ["sport21_hitting_empty.json", "sport15_hitting_empty.json"] {
            let doc = envelope(name);
            assert_eq!(doc.stats[0].total_splits, Some(0));
            assert!(doc.stats[0].splits.is_empty());
        }
    }

    #[test]
    fn raleigh_rates_match_the_api_within_rounding() {
        // Recorded season line: 147 H, 596 AB, 97 BB, 9 HBP, 3 SF, 351 TB.
        let doc = envelope("hitting_raleigh.json");
        let split = &doc.stats[0].splits[0];
        assert_eq!(classify_split(split).unwrap(), SplitDisposition::Stint);
        let row = batting_stint(split, mlb_hitting()).unwrap();
        assert_eq!(row.player_id, 663_728);
        assert_eq!(row.player_name, "Cal Raleigh");
        assert_eq!(row.team_id, 136);
        assert_eq!(row.home_runs, 60);
        assert_eq!(row.plate_appearances, 705);
        assert_eq!(
            rates::total_bases(row.hits, row.doubles, row.triples, row.home_runs),
            Some(row.total_bases)
        );
        assert_close(row.avg.unwrap(), row.api_avg.unwrap(), 3);
        assert_close(row.obp.unwrap(), row.api_obp.unwrap(), 3);
        assert_close(row.slg.unwrap(), row.api_slg.unwrap(), 3);
        assert_close(row.ops.unwrap(), row.api_ops.unwrap(), 3);
        assert_eq!(row.api_avg, Some(0.247));
    }

    #[test]
    fn nelson_leading_dot_average_and_undefined_percentage() {
        let doc = envelope("hitting_nelson.json");
        let split = &doc.stats[0].splits[0];
        assert_eq!(split.stat.avg.0, Some(1.0));
        assert_eq!(split.stat.stolen_base_percentage.0, None);
        let row = batting_stint(split, mlb_hitting()).unwrap();
        assert_eq!(row.at_bats, 1);
        assert_eq!(row.hits, 1);
        assert_eq!(row.avg, Some(1.0));
        assert_eq!(row.position.as_deref(), Some("P"));
    }

    #[test]
    fn naylor_sportwide_row_is_not_a_mariners_stint() {
        let doc = envelope("hitting_naylor_sportwide.json");
        let split = &doc.stats[0].splits[0];
        assert_eq!(split.num_teams, Some(2));
        assert_eq!(
            split.team.as_ref().unwrap().name.as_deref(),
            Some("Seattle Mariners")
        );
        assert_eq!(split.stat.games_played.0, 147);
        assert_eq!(classify_split(split).unwrap(), SplitDisposition::Hydrate);
        assert!(
            batting_stints(&doc.stats[0].splits, mlb_hitting())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn naylor_people_payload_is_two_stints_that_sum_to_the_season() {
        let doc = envelope("naylor_people_hitting.json");
        assert_eq!(doc.stats[0].splits.len(), 3);
        assert_eq!(
            classify_split(&doc.stats[0].splits[0]).unwrap(),
            SplitDisposition::Ignore
        );
        let rows = batting_stints(&doc.stats[0].splits, mlb_hitting()).unwrap();
        assert_eq!(rows.len(), 2);
        let games: u32 = rows.iter().map(|row| row.games).sum();
        let hr: u32 = rows.iter().map(|row| row.home_runs).sum();
        let pa: u32 = rows.iter().map(|row| row.plate_appearances).sum();
        let hits: u32 = rows.iter().map(|row| row.hits).sum();
        let ab: u32 = rows.iter().map(|row| row.at_bats).sum();
        assert_eq!(games, 147);
        assert_eq!(hr, 20);
        assert_eq!(pa, 604);
        assert_close(rates::batting_average(hits, ab).unwrap(), 0.295, 3);
        let names: Vec<_> = rows.iter().map(|row| row.team_name.as_str()).collect();
        assert!(names.contains(&"Arizona Diamondbacks"));
        assert!(names.contains(&"Seattle Mariners"));
        let seattle = rows
            .iter()
            .find(|row| row.team_name == "Seattle Mariners")
            .unwrap();
        assert_eq!(seattle.games, 54);
        assert_eq!(seattle.home_runs, 9);
    }

    #[test]
    fn skenes_era_and_whip_use_outs() {
        let doc = envelope("pitching_skenes.json");
        let row = pitching_stint(&doc.stats[0].splits[0], mlb_pitching()).unwrap();
        assert_eq!(row.player_id, 694_973);
        assert_eq!(row.outs, 563);
        assert_eq!(row.innings_pitched, "187.2");
        assert_eq!(Innings::from_outs(row.outs).notation(), "187.2");
        assert_close(row.era.unwrap(), 1.97, 2);
        assert_close(row.whip.unwrap(), 0.95, 2);
        assert_close(row.era.unwrap(), row.api_era.unwrap(), 2);
        assert_close(row.whip.unwrap(), row.api_whip.unwrap(), 2);
    }

    #[test]
    fn acton_zero_era_and_one_inning() {
        let doc = envelope("pitching_acton.json");
        let row = pitching_stint(&doc.stats[0].splits[0], mlb_pitching()).unwrap();
        assert_eq!(row.outs, 3);
        assert_eq!(row.earned_runs, 0);
        assert_eq!(row.era, Some(0.0));
        assert_close(row.whip.unwrap(), 2.0, 2);
    }

    #[test]
    fn rogers_people_stints_sum_to_the_combined_line() {
        let doc = envelope("rogers_people_pitching.json");
        let rows = pitching_stints(&doc.stats[0].splits, mlb_pitching()).unwrap();
        assert_eq!(rows.len(), 2);
        let outs: u32 = rows.iter().map(|row| row.outs).sum();
        let er: u32 = rows.iter().map(|row| row.earned_runs).sum();
        assert_eq!(outs, 232);
        assert_eq!(er, 17);
        assert_close(
            rates::earned_run_average(er, Innings::from_outs(outs)).unwrap(),
            1.98,
            2,
        );
    }

    #[test]
    fn judge_gamelog_keeps_per_game_counts() {
        let doc = envelope("judge_gamelog_two_games.json");
        let rows = game_log(&doc, mlb_hitting(), 592_450).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].game_pk, 778_557);
        assert_eq!(rows[0].game_date, "2025-03-27");
        assert_eq!(rows[0].hits, 1);
        assert_eq!(rows[0].at_bats, 4);
        assert_eq!(rows[0].opponent_name.as_deref(), Some("Milwaukee Brewers"));
        assert_eq!(rows[0].is_home, Some(true));
        assert_eq!(rows[0].position.as_deref(), Some("RF"));
        assert_eq!(rows[0].summary.as_deref(), Some("1-4 | 2B, 2 K, RBI"));
        assert_close(rows[0].avg.unwrap(), 0.250, 3);
        assert_eq!(rows[1].home_runs, 3);
        assert_eq!(rows[1].hits, 4);
    }

    #[test]
    fn skenes_second_start_does_not_keep_the_running_era() {
        // Second recorded start: 0 ER in 7.0 IP. The API `era` on that split is
        // "1.46", the season ERA through the game, not the game ERA of 0.00.
        let doc = envelope("skenes_gamelog_two_games.json");
        assert_eq!(doc.stats[0].splits[1].stat.era.0, Some(1.46));
        let rows = game_log(&doc, mlb_pitching(), 694_973).unwrap();
        assert_eq!(rows[0].innings_pitched, "5.1");
        assert_eq!(rows[0].outs, 16);
        assert_eq!(rows[0].earned_runs, 2);
        assert_close(rows[0].era.unwrap(), 3.38, 2);
        assert_eq!(rows[1].earned_runs, 0);
        assert_eq!(rows[1].outs, 21);
        assert_eq!(rows[1].era, Some(0.0));
        assert!((rows[1].era.unwrap() - 1.46).abs() > 1.0);
    }

    #[test]
    fn anthony_lines_are_different_levels() {
        let mlb = envelope("hitting_anthony_mlb.json");
        let aaa = envelope("hitting_anthony_aaa.json");
        let mlb_row = batting_stint(&mlb.stats[0].splits[0], mlb_hitting()).unwrap();
        let aaa_row = batting_stint(
            &aaa.stats[0].splits[0],
            NormalizeCtx {
                season: 2025,
                level: Level::Aaa,
                group: StatGroup::Hitting,
            },
        )
        .unwrap();
        assert_eq!(mlb_row.player_id, 701_350);
        assert_eq!(aaa_row.player_id, 701_350);
        assert_eq!(mlb_row.level, Level::Mlb);
        assert_eq!(aaa_row.level, Level::Aaa);
        assert_eq!(mlb_row.team_name, "Boston Red Sox");
        assert_eq!(aaa_row.team_name, "Worcester Red Sox");
        assert_eq!(mlb_row.home_runs, 8);
        assert_eq!(aaa_row.home_runs, 10);
        assert_close(mlb_row.avg.unwrap(), 0.292, 3);
        assert_close(aaa_row.avg.unwrap(), 0.288, 3);
    }

    #[test]
    fn trimmed_team_objects_keep_parent_clubs_for_aaa() {
        let mlb: TeamsEnvelope = serde_json::from_str(&fixture("teams_mlb_trimmed.json")).unwrap();
        let aaa: TeamsEnvelope = serde_json::from_str(&fixture("teams_aaa_trimmed.json")).unwrap();
        let mlb_teams = teams(&mlb, 2025, Level::Mlb).unwrap();
        let aaa_teams = teams(&aaa, 2025, Level::Aaa).unwrap();
        let mariners = mlb_teams.iter().find(|team| team.team_id == 136).unwrap();
        assert_eq!(mariners.abbreviation.as_deref(), Some("SEA"));
        assert!(mariners.parent_org_id.is_none());
        let worcester = aaa_teams.iter().find(|team| team.team_id == 533).unwrap();
        assert_eq!(worcester.parent_org_name.as_deref(), Some("Boston Red Sox"));
        assert_eq!(
            worcester.league_name.as_deref(),
            Some("International League")
        );
    }
}
