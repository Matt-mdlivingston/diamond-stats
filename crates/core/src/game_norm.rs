//! Turn schedule, box score, and play-by-play documents into domain rows.
//!
//! A postponed game stays on the schedule and does not become a box score.
//! Pitch type and velocity are copied through only when the feed sent them.

use std::collections::HashMap;

use crate::api_model::RawStat;
use crate::error::CoreError;
use crate::game::{BoxBattingLine, BoxPitchingLine, Pitch, PlateAppearance, ScheduledGame};
use crate::game_api::{
    BoxscoreEnvelope, BoxscorePlayer, BoxscoreTeam, PlayByPlayEnvelope, ScheduleEnvelope,
};
use crate::level::Level;
use crate::normalize::resolve_outs;
use crate::rates::Innings;

/// Games returned for one schedule date.
///
/// `schedule_date` is the date that was requested. A game whose `officialDate`
/// points at a makeup date is kept, with both dates stored.
pub fn schedule_games(
    envelope: &ScheduleEnvelope,
    season: u16,
    level: Level,
    schedule_date: &str,
) -> Result<Vec<ScheduledGame>, CoreError> {
    let mut games = Vec::new();
    for date in &envelope.dates {
        for game in &date.games {
            games.push(scheduled_game(
                game,
                season,
                level,
                schedule_date,
                date.date.as_deref(),
            )?);
        }
    }
    Ok(games)
}

/// The single game inside `GET /api/v1/schedule?gamePk=`.
///
/// The schedule team object has no `sport`. The caller has already resolved
/// `level` (from a box score or from `GET /api/v1/teams/{id}`).
pub fn one_schedule_game(
    envelope: &ScheduleEnvelope,
    level: Level,
) -> Result<ScheduledGame, CoreError> {
    let games: Vec<_> = envelope
        .dates
        .iter()
        .flat_map(|date| date.games.iter().map(move |game| (date, game)))
        .collect();
    let Some((date, game)) = games.first() else {
        return Err(CoreError::Message(
            "schedule response did not contain a game".into(),
        ));
    };
    if games.len() != 1 {
        return Err(CoreError::Message(format!(
            "schedule response for one gamePk contained {} games",
            games.len()
        )));
    }
    let official = game
        .official_date
        .clone()
        .or_else(|| date.date.clone())
        .ok_or_else(|| CoreError::Message("schedule game is missing officialDate".into()))?;
    let season = parse_season(game.season.as_deref(), game.game_pk)?;
    scheduled_game(game, season, level, &official, date.date.as_deref())
}

/// Batting and pitching lines for a final game.
pub fn box_score(
    envelope: &BoxscoreEnvelope,
    game: &ScheduledGame,
) -> Result<(Vec<BoxBattingLine>, Vec<BoxPitchingLine>), CoreError> {
    let mut batting = Vec::new();
    let mut pitching = Vec::new();
    for (is_home, side) in [(false, &envelope.teams.away), (true, &envelope.teams.home)] {
        check_box_side(side, game, is_home)?;
        for player_id in &side.batters {
            let player = find_player(&side.players, *player_id, game.game_pk)?;
            batting.push(batting_line(game, side, is_home, *player_id, player)?);
        }
        for player_id in &side.pitchers {
            let player = find_player(&side.players, *player_id, game.game_pk)?;
            pitching.push(pitching_line(game, side, is_home, *player_id, player)?);
        }
    }
    Ok((batting, pitching))
}

/// Plate appearances and the pitches the feed actually included.
///
/// Incomplete plate appearances are skipped so a captured in-progress feed is
/// not stored as a finished game. Callers only request this for final games.
pub fn play_by_play(
    envelope: &PlayByPlayEnvelope,
    game: &ScheduledGame,
) -> Result<(Vec<PlateAppearance>, Vec<Pitch>), CoreError> {
    let mut plays = Vec::new();
    let mut pitches = Vec::new();
    for play in &envelope.all_plays {
        if play.about.is_complete == Some(false) {
            continue;
        }
        let batter = play.matchup.batter.as_ref().ok_or_else(|| {
            CoreError::Message(format!(
                "game {} plate appearance {} is missing a batter",
                game.game_pk, play.about.at_bat_index
            ))
        })?;
        let pitcher = play.matchup.pitcher.as_ref().ok_or_else(|| {
            CoreError::Message(format!(
                "game {} plate appearance {} is missing a pitcher",
                game.game_pk, play.about.at_bat_index
            ))
        })?;
        let half = half_inning(&play.about, game.game_pk)?;
        plays.push(PlateAppearance {
            game_pk: game.game_pk,
            season: game.season,
            level: game.level,
            schedule_date: game.schedule_date.clone(),
            official_date: game.official_date.clone(),
            at_bat_index: play.about.at_bat_index,
            inning: play.about.inning,
            half_inning: half,
            batter_id: batter.id,
            batter_name: batter.display_name(),
            pitcher_id: pitcher.id,
            pitcher_name: pitcher.display_name(),
            bat_side: play
                .matchup
                .bat_side
                .as_ref()
                .and_then(|side| side.code.clone()),
            pitch_hand: play
                .matchup
                .pitch_hand
                .as_ref()
                .and_then(|side| side.code.clone()),
            event: play.result.event.clone(),
            event_type: play.result.event_type.clone(),
            description: play.result.description.clone(),
            rbi: play.result.rbi.unwrap_or(0),
            is_scoring_play: play.about.is_scoring_play.unwrap_or(false),
            away_score: play.result.away_score.unwrap_or(0),
            home_score: play.result.home_score.unwrap_or(0),
        });
        let mut number = 0_u32;
        for event in &play.play_events {
            let is_pitch = event.is_pitch || event.kind.as_deref() == Some("pitch");
            if !is_pitch {
                continue;
            }
            number = number.saturating_add(1);
            let details = event.details.as_ref();
            let pitch_type = details.and_then(|details| details.pitch_type.as_ref());
            pitches.push(Pitch {
                game_pk: game.game_pk,
                season: game.season,
                level: game.level,
                schedule_date: game.schedule_date.clone(),
                official_date: game.official_date.clone(),
                at_bat_index: play.about.at_bat_index,
                pitch_number: event.pitch_number.unwrap_or(number),
                call_code: details
                    .and_then(|details| details.call.as_ref())
                    .and_then(|call| call.code.clone()),
                call_description: details.and_then(|details| {
                    details
                        .call
                        .as_ref()
                        .and_then(|call| call.description.clone())
                        .or_else(|| details.description.clone())
                }),
                pitch_type_code: pitch_type.and_then(|kind| kind.code.clone()),
                pitch_type: pitch_type.and_then(|kind| kind.description.clone()),
                start_speed: event.pitch_data.as_ref().and_then(|data| data.start_speed),
                is_ball: details.and_then(|details| details.is_ball).unwrap_or(false),
                is_strike: details
                    .and_then(|details| details.is_strike)
                    .unwrap_or(false),
                is_in_play: details
                    .and_then(|details| details.is_in_play)
                    .unwrap_or(false),
            });
        }
    }
    Ok((plays, pitches))
}

fn scheduled_game(
    game: &crate::game_api::ScheduleGame,
    season: u16,
    level: Level,
    schedule_date: &str,
    group_date: Option<&str>,
) -> Result<ScheduledGame, CoreError> {
    let parsed = parse_season(game.season.as_deref(), game.game_pk)?;
    if parsed != season {
        return Err(CoreError::Message(format!(
            "game {} season {parsed} does not match requested {season}",
            game.game_pk
        )));
    }
    let official_date = game
        .official_date
        .clone()
        .or_else(|| group_date.map(str::to_string))
        .ok_or_else(|| {
            CoreError::Message(format!("game {} is missing officialDate", game.game_pk))
        })?;
    let status = game.status.as_ref();
    Ok(ScheduledGame {
        game_pk: game.game_pk,
        season,
        level,
        schedule_date: schedule_date.to_string(),
        official_date,
        game_type: game.game_type.clone().unwrap_or_else(|| "R".to_string()),
        detailed_state: status
            .and_then(|status| status.detailed_state.clone())
            .unwrap_or_default(),
        abstract_state: status
            .and_then(|status| status.abstract_game_state.clone())
            .unwrap_or_default(),
        coded_state: status
            .and_then(|status| status.coded_game_state.clone())
            .unwrap_or_default(),
        away_team_id: game.teams.away.team.id,
        away_team_name: game
            .teams
            .away
            .team
            .name
            .clone()
            .unwrap_or_else(|| format!("team {}", game.teams.away.team.id)),
        away_score: game.teams.away.score,
        home_team_id: game.teams.home.team.id,
        home_team_name: game
            .teams
            .home
            .team
            .name
            .clone()
            .unwrap_or_else(|| format!("team {}", game.teams.home.team.id)),
        home_score: game.teams.home.score,
        venue_name: game.venue.as_ref().and_then(|venue| venue.name.clone()),
        double_header: game
            .double_header
            .clone()
            .unwrap_or_else(|| "N".to_string()),
        game_number: game.game_number.unwrap_or(1),
    })
}

fn parse_season(raw: Option<&str>, game_pk: u64) -> Result<u16, CoreError> {
    let Some(raw) = raw else {
        return Err(CoreError::Message(format!(
            "game {game_pk} is missing a season"
        )));
    };
    raw.parse::<u16>()
        .map_err(|_| CoreError::Message(format!("game {game_pk} has season '{raw}'")))
}

fn check_box_side(
    side: &BoxscoreTeam,
    game: &ScheduledGame,
    is_home: bool,
) -> Result<(), CoreError> {
    let expected = if is_home {
        game.home_team_id
    } else {
        game.away_team_id
    };
    if side.team.id != expected {
        return Err(CoreError::Message(format!(
            "game {} box score {} team {} does not match schedule team {expected}",
            game.game_pk,
            if is_home { "home" } else { "away" },
            side.team.id
        )));
    }
    if let Some(sport_id) = side.team.sport.as_ref().and_then(|sport| sport.id)
        && sport_id != game.level.sport_id()
    {
        return Err(CoreError::Message(format!(
            "game {} box score sportId {sport_id} does not match {}",
            game.game_pk,
            game.level.sport_id()
        )));
    }
    Ok(())
}

fn find_player(
    players: &HashMap<String, BoxscorePlayer>,
    player_id: u32,
    game_pk: u64,
) -> Result<&BoxscorePlayer, CoreError> {
    if let Some(player) = players.get(&format!("ID{player_id}")) {
        return Ok(player);
    }
    players
        .values()
        .find(|player| player.person.id == player_id)
        .ok_or_else(|| {
            CoreError::Message(format!(
                "game {game_pk} box score lists player {player_id} but has no matching player object"
            ))
        })
}

fn batting_line(
    game: &ScheduledGame,
    side: &BoxscoreTeam,
    is_home: bool,
    player_id: u32,
    player: &BoxscorePlayer,
) -> Result<BoxBattingLine, CoreError> {
    let stat = &player.stats.batting;
    Ok(BoxBattingLine {
        season: game.season,
        level: game.level,
        game_pk: game.game_pk,
        schedule_date: game.schedule_date.clone(),
        official_date: game.official_date.clone(),
        team_id: side.team.id,
        team_name: side.team.name.clone(),
        is_home,
        player_id,
        player_name: player.person.display_name(),
        position: player
            .position
            .as_ref()
            .and_then(|pos| pos.abbreviation.clone()),
        batting_order: player.batting_order,
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
        hit_by_pitch: stat.hit_by_pitch.0,
        sac_flies: stat.sac_flies.0,
        total_bases: stat.total_bases.0,
        stolen_bases: stat.stolen_bases.0,
        summary: stat.summary.clone(),
        avg: None,
    })
}

fn pitching_line(
    game: &ScheduledGame,
    side: &BoxscoreTeam,
    is_home: bool,
    player_id: u32,
    player: &BoxscorePlayer,
) -> Result<BoxPitchingLine, CoreError> {
    let stat: &RawStat = &player.stats.pitching;
    let outs = resolve_outs(stat)?;
    Ok(BoxPitchingLine {
        season: game.season,
        level: game.level,
        game_pk: game.game_pk,
        schedule_date: game.schedule_date.clone(),
        official_date: game.official_date.clone(),
        team_id: side.team.id,
        team_name: side.team.name.clone(),
        is_home,
        player_id,
        player_name: player.person.display_name(),
        position: player
            .position
            .as_ref()
            .and_then(|pos| pos.abbreviation.clone()),
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
        strikeouts: stat.strike_outs.0,
        hit_batters: stat.hit_batsmen.0,
        batters_faced: stat.batters_faced.0,
        number_of_pitches: stat.number_of_pitches.0,
        wins: stat.wins.0,
        losses: stat.losses.0,
        saves: stat.saves.0,
        summary: stat.summary.clone(),
        era: None,
        whip: None,
    })
}

fn half_inning(about: &crate::game_api::PlayAbout, game_pk: u64) -> Result<String, CoreError> {
    if let Some(half) = about.half_inning.as_deref() {
        let token = half.trim().to_ascii_lowercase();
        if token == "top" || token == "bottom" {
            return Ok(token);
        }
    }
    match about.is_top_inning {
        Some(true) => Ok("top".to_string()),
        Some(false) => Ok("bottom".to_string()),
        None => Err(CoreError::Message(format!(
            "game {game_pk} plate appearance {} is missing a half inning",
            about.at_bat_index
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_model::{StatsEnvelope, TeamsEnvelope};
    use crate::game::PitchTracking;
    use crate::normalize::{batting_stints, teams};
    use crate::{NormalizeCtx, StatGroup};
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    }

    #[test]
    fn minor_league_season_pages_use_the_same_stat_shape() {
        let cases = [
            ("hitting_aa_one.json", Level::Aa, "Juan Sulbaran", 1, 1),
            (
                "hitting_high_a_one.json",
                Level::HighA,
                "Jesse Winker",
                1,
                1,
            ),
            (
                "hitting_single_a_one.json",
                Level::SingleA,
                "Jose D. Hernandez",
                3,
                10,
            ),
            ("hitting_rookie_one.json", Level::Rookie, "Mike Boeve", 1, 2),
        ];
        for (name, level, player, games, at_bats) in cases {
            let doc: StatsEnvelope = serde_json::from_str(&fixture(name)).unwrap();
            assert!(
                doc.copyright.contains("MLB Advanced Media"),
                "{name} lost the copyright notice"
            );
            let rows = batting_stints(
                &doc.stats[0].splits,
                NormalizeCtx {
                    season: 2025,
                    level,
                    group: StatGroup::Hitting,
                },
            )
            .unwrap();
            assert_eq!(rows.len(), 1, "{name}");
            assert_eq!(rows[0].player_name, player);
            assert_eq!(rows[0].level, level);
            assert_eq!(rows[0].games, games);
            assert_eq!(rows[0].at_bats, at_bats);
            assert_eq!(rows[0].at_bats, doc.stats[0].splits[0].stat.at_bats.0);
            if at_bats > 0 {
                assert!(rows[0].avg.is_some());
            }
        }
    }

    #[test]
    fn rookie_teams_cover_complex_and_dsl_under_one_sport() {
        let doc: TeamsEnvelope =
            serde_json::from_str(&fixture("teams_rookie_trimmed.json")).unwrap();
        let clubs = teams(&doc, 2025, Level::Rookie).unwrap();
        assert_eq!(clubs.len(), 2);
        assert!(clubs.iter().all(|club| club.level == Level::Rookie));
        let leagues: Vec<_> = clubs
            .iter()
            .filter_map(|club| club.league_name.clone())
            .collect();
        assert!(leagues.iter().any(|name| name.contains("Florida Complex")));
        assert!(leagues.iter().any(|name| name.contains("Dominican Summer")));
        assert!(clubs.iter().all(|club| club.parent_org_name.is_some()));
    }

    #[test]
    fn mlb_schedule_keeps_final_scores_and_a_postponement_stays_unplayed() {
        let mlb: ScheduleEnvelope =
            serde_json::from_str(&fixture("schedule_mlb_2025-06-21_two.json")).unwrap();
        assert!(mlb.copyright.contains("MLB Advanced Media"));
        let games = schedule_games(&mlb, 2025, Level::Mlb, "2025-06-21").unwrap();
        assert_eq!(games.len(), 2);
        let tigers = games.iter().find(|game| game.game_pk == 777_420).unwrap();
        assert!(tigers.boxscore_ready());
        assert_eq!(tigers.away_team_name, "Detroit Tigers");
        assert_eq!(tigers.away_score, Some(3));
        assert_eq!(tigers.home_team_name, "Tampa Bay Rays");
        assert_eq!(tigers.home_score, Some(8));
        assert_eq!(
            tigers.venue_name.as_deref(),
            Some("George M. Steinbrenner Field")
        );
        assert_eq!(tigers.double_header, "N");
        assert_eq!(tigers.game_number, 1);
        assert_eq!(tigers.official_date, "2025-06-21");
        assert_eq!(tigers.schedule_date, "2025-06-21");

        let high_a: ScheduleEnvelope = serde_json::from_str(&fixture(
            "schedule_high_a_2025-06-21_final_and_postponed.json",
        ))
        .unwrap();
        let games = schedule_games(&high_a, 2025, Level::HighA, "2025-06-21").unwrap();
        let played = games.iter().find(|game| game.game_pk == 783_928).unwrap();
        assert!(played.boxscore_ready());
        assert_eq!(played.away_score, Some(4));
        assert_eq!(played.home_score, Some(3));
        let postponed = games.iter().find(|game| game.game_pk == 789_134).unwrap();
        assert_eq!(postponed.abstract_state, "Final");
        assert_eq!(postponed.coded_state, "D");
        assert_eq!(postponed.detailed_state, "Postponed");
        assert!(!postponed.boxscore_ready());
        assert_eq!(postponed.away_score, None);
        assert_eq!(postponed.home_score, None);
        assert_eq!(postponed.schedule_date, "2025-06-21");
        assert_eq!(postponed.official_date, "2025-07-24");
        assert_eq!(postponed.away_team_name, "Vancouver Canadians");
    }

    #[test]
    fn box_scores_keep_the_game_line_and_baseball_innings() {
        let game = mlb_game();
        let envelope: BoxscoreEnvelope =
            serde_json::from_str(&fixture("boxscore_mlb_777420_trimmed.json")).unwrap();
        let (batting, pitching) = box_score(&envelope, &game).unwrap();
        let keith = batting.iter().find(|row| row.player_id == 690_993).unwrap();
        assert_eq!(keith.player_name, "Colt Keith");
        assert_eq!(keith.position.as_deref(), Some("1B"));
        assert_eq!(keith.batting_order, Some(100));
        assert_eq!(keith.at_bats, 4);
        assert_eq!(keith.hits, 1);
        assert!(!keith.is_home);
        let hurter = pitching
            .iter()
            .find(|row| row.player_id == 676_428)
            .unwrap();
        assert_eq!(hurter.player_name, "Brant Hurter");
        assert_eq!(hurter.innings_pitched, "0.2");
        assert_eq!(hurter.outs, 2);
        assert_eq!(hurter.earned_runs, 0);
        assert_eq!(hurter.strikeouts, 2);
        assert_eq!(hurter.walks, 2);
        assert_eq!(hurter.hits, 2);
        let pepiot = pitching
            .iter()
            .find(|row| row.player_id == 686_752)
            .unwrap();
        assert!(pepiot.is_home);
        assert_eq!(pepiot.outs, 15);
        assert_eq!(pepiot.innings_pitched, "5.0");
    }

    #[test]
    fn play_by_play_keeps_velocity_only_when_the_feed_sent_it() {
        let mlb = mlb_game();
        let doc: PlayByPlayEnvelope =
            serde_json::from_str(&fixture("playbyplay_mlb_777420_one_pa.json")).unwrap();
        let (plays, pitches) = play_by_play(&doc, &mlb).unwrap();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].batter_name, "Colt Keith");
        assert_eq!(plays[0].batter_id, 690_993);
        assert_eq!(plays[0].pitcher_name, "Ryan Pepiot");
        assert_eq!(plays[0].event.as_deref(), Some("Lineout"));
        assert_eq!(plays[0].event_type.as_deref(), Some("field_out"));
        assert_eq!(plays[0].half_inning, "top");
        assert_eq!(plays[0].inning, 1);
        assert!(!plays[0].is_scoring_play);
        assert_eq!(pitches.len(), 3);
        assert_eq!(pitches[0].call_code.as_deref(), Some("C"));
        assert_eq!(pitches[0].pitch_type_code.as_deref(), Some("FF"));
        assert_eq!(pitches[0].start_speed, Some(94.0));
        assert!(pitches[0].is_strike);
        let tracking = PitchTracking::from_rows(&plays, &pitches);
        assert_eq!(tracking.pitches_with_velocity, 3);
        assert_eq!(tracking.pitches_with_pitch_type, 3);

        let mut aa = mlb_game();
        aa.level = Level::Aa;
        aa.game_pk = 782_551;
        aa.away_team_id = 498;
        aa.home_team_id = 559;
        let doc: PlayByPlayEnvelope =
            serde_json::from_str(&fixture("playbyplay_aa_782551_one_pa.json")).unwrap();
        let (plays, pitches) = play_by_play(&doc, &aa).unwrap();
        assert_eq!(plays[0].batter_name, "Héctor Rodríguez");
        assert_eq!(plays[0].pitcher_name, "Sam Aldegheri");
        assert_eq!(plays[0].event.as_deref(), Some("Triple"));
        assert_eq!(plays[0].event_type.as_deref(), Some("triple"));
        assert_eq!(pitches[0].call_code.as_deref(), Some("F"));
        assert_eq!(pitches[0].pitch_type_code, None);
        assert_eq!(pitches[0].start_speed, None);
        assert!(pitches[0].is_strike);
        let tracking = PitchTracking::from_rows(&plays, &pitches);
        assert_eq!(tracking.pitches_with_velocity, 0);
        assert_eq!(tracking.pitches_with_pitch_type, 0);
        assert!(tracking.pitches >= 1);
    }

    fn mlb_game() -> ScheduledGame {
        let doc: ScheduleEnvelope =
            serde_json::from_str(&fixture("schedule_mlb_2025-06-21_two.json")).unwrap();
        schedule_games(&doc, 2025, Level::Mlb, "2025-06-21")
            .unwrap()
            .into_iter()
            .find(|game| game.game_pk == 777_420)
            .unwrap()
    }
}
