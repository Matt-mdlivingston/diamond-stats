//! Rate stats derived from counting stats.
//!
//! MLB prints innings as `whole.partial` where the partial is outs, not a
//! decimal tenth: `5.1` is five innings and one out (16 outs, 5.333...).
//! ERA and WHIP divide by outs/3. Formulas:
//!
//! - AVG = H / AB
//! - OBP = (H + BB + HBP) / (AB + BB + HBP + SF)
//! - SLG = TB / AB
//! - OPS = OBP + SLG
//! - TB = H + 2B + 2*3B + 3*HR
//! - ERA = 9 * ER / (outs / 3)
//! - WHIP = (BB + H) / (outs / 3)
//!
//! Intentional walks are already included in walks. Hit batsmen are not part of WHIP.

use crate::error::CoreError;

/// Innings stored as outs so `.1` and `.2` cannot be confused with decimals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Innings {
    outs: u32,
}

impl Innings {
    /// Build innings from a number of outs.
    pub fn from_outs(outs: u32) -> Self {
        Self { outs }
    }

    /// Outs, the source of truth.
    pub fn outs(self) -> u32 {
        self.outs
    }

    /// True innings as a decimal (`5.1` baseball notation → `5.333...`).
    pub fn as_decimal(self) -> f64 {
        f64::from(self.outs) / 3.0
    }

    /// Baseball notation: whole innings, a dot, and the leftover outs (`0`, `1`, or `2`).
    pub fn notation(self) -> String {
        let whole = self.outs / 3;
        let rem = self.outs % 3;
        format!("{whole}.{rem}")
    }

    /// Parse baseball notation (`187.2`, `5.1`, `7.0`, `7`).
    pub fn parse(raw: &str) -> Result<Self, CoreError> {
        let s = raw.trim();
        if s.is_empty() || !s.chars().any(|c| c.is_ascii_digit()) {
            return Err(CoreError::InvalidInnings(raw.to_string()));
        }
        let (whole_str, tenth_str) = match s.split_once('.') {
            Some((whole, tenth)) => (whole, tenth),
            None => (s, "0"),
        };
        if tenth_str.len() != 1 || !tenth_str.chars().all(|c| c.is_ascii_digit()) {
            return Err(CoreError::InvalidInnings(raw.to_string()));
        }
        let whole: u32 = if whole_str.is_empty() {
            0
        } else {
            whole_str
                .parse()
                .map_err(|_| CoreError::InvalidInnings(raw.to_string()))?
        };
        let tenth: u32 = tenth_str
            .parse()
            .map_err(|_| CoreError::InvalidInnings(raw.to_string()))?;
        if tenth > 2 {
            return Err(CoreError::InvalidInnings(raw.to_string()));
        }
        Ok(Self {
            outs: whole
                .checked_mul(3)
                .and_then(|v| v.checked_add(tenth))
                .ok_or_else(|| CoreError::InvalidInnings(raw.to_string()))?,
        })
    }
}

/// Parse a rate the Stats API sent as a string.
///
/// The API uses `".---"` and `"-.--"` for undefined rates and often omits the
/// leading zero (`".667"`). Those sentinels become `None`.
pub fn parse_api_decimal(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if s.is_empty() || !s.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let normalized = if let Some(rest) = s.strip_prefix('.') {
        format!("0.{rest}")
    } else if let Some(rest) = s.strip_prefix("-.") {
        format!("-0.{rest}")
    } else {
        s.to_string()
    };
    normalized
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

/// Hits divided by at-bats. `None` when there are no at-bats.
pub fn batting_average(hits: u32, at_bats: u32) -> Option<f64> {
    ratio(hits, at_bats)
}

/// Official on-base percentage. Sac bunts are not in the denominator.
pub fn on_base_percentage(
    hits: u32,
    walks: u32,
    hit_by_pitch: u32,
    at_bats: u32,
    sac_flies: u32,
) -> Option<f64> {
    let numerator = hits.saturating_add(walks).saturating_add(hit_by_pitch);
    let denominator = at_bats
        .saturating_add(walks)
        .saturating_add(hit_by_pitch)
        .saturating_add(sac_flies);
    ratio(numerator, denominator)
}

/// Total bases divided by at-bats.
pub fn slugging(total_bases: u32, at_bats: u32) -> Option<f64> {
    ratio(total_bases, at_bats)
}

/// On-base plus slugging. `None` when either component is undefined.
pub fn ops(on_base: Option<f64>, slugging_pct: Option<f64>) -> Option<f64> {
    Some(on_base? + slugging_pct?)
}

/// `H + 2B + 2*3B + 3*HR`, which is the usual total-bases identity.
pub fn total_bases(hits: u32, doubles: u32, triples: u32, home_runs: u32) -> Option<u32> {
    hits.checked_add(doubles)?
        .checked_add(triples.checked_mul(2)?)?
        .checked_add(home_runs.checked_mul(3)?)
}

/// Earned runs times nine, divided by true innings. `None` when there are no outs.
pub fn earned_run_average(earned_runs: u32, innings: Innings) -> Option<f64> {
    if innings.outs() == 0 {
        return None;
    }
    Some(9.0 * f64::from(earned_runs) / innings.as_decimal())
}

/// Walks plus hits, divided by true innings. Hit batsmen are not included.
pub fn whip(walks: u32, hits: u32, innings: Innings) -> Option<f64> {
    if innings.outs() == 0 {
        return None;
    }
    let baserunners = walks.saturating_add(hits);
    Some(f64::from(baserunners) / innings.as_decimal())
}

fn ratio(numerator: u32, denominator: u32) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(f64::from(numerator) / f64::from(denominator))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn api_sentinels_and_leading_dots() {
        assert_eq!(parse_api_decimal(".---"), None);
        assert_eq!(parse_api_decimal("-.--"), None);
        assert_eq!(parse_api_decimal(".667"), Some(0.667));
        assert_eq!(parse_api_decimal("1.000"), Some(1.0));
        assert_eq!(parse_api_decimal(".000"), Some(0.0));
        assert_eq!(parse_api_decimal("1.97"), Some(1.97));
    }

    #[test]
    fn baseball_innings_notation() {
        assert_eq!(Innings::parse("187.2").unwrap().outs(), 563);
        assert_eq!(Innings::parse("5.1").unwrap().outs(), 16);
        assert_eq!(Innings::parse("1.0").unwrap().outs(), 3);
        assert_eq!(Innings::parse("0.1").unwrap().outs(), 1);
        assert_eq!(Innings::from_outs(563).notation(), "187.2");
        assert!(Innings::parse("1.3").is_err());
        assert!(Innings::parse(".---").is_err());
        assert!(Innings::parse("5.10").is_err());
    }

    /// Skenes' first 2025 start: 5.1 IP, 2 ER. The API prints ERA `3.38`, which
    /// is `9 * 2 / (16/3)`, not `18 / 5.1`.
    #[test]
    fn game_era_uses_outs_not_decimal_tenths() {
        let innings = Innings::parse("5.1").unwrap();
        let era = earned_run_average(2, innings).unwrap();
        assert!((era - 3.375).abs() < 1e-12);
        assert!((era - 3.38).abs() < 0.01);
        let decimal_tenths: f64 = 18.0 / 5.1;
        assert!((decimal_tenths - 3.53_f64).abs() < 0.01);
        assert!((era - decimal_tenths).abs() > 0.1);
    }

    proptest! {
        #[test]
        fn average_is_bounded_and_exact(hits in 0u32..400, at_bats in 1u32..700) {
            let hits = hits % (at_bats + 1);
            let avg = batting_average(hits, at_bats).unwrap();
            prop_assert!((0.0..=1.0).contains(&avg));
            prop_assert!((avg - f64::from(hits) / f64::from(at_bats)).abs() < 1e-12);
        }

        #[test]
        fn obp_is_bounded_when_hits_cannot_exceed_at_bats(
            hits in 0u32..400,
            walks in 0u32..200,
            hbp in 0u32..40,
            at_bats in 0u32..700,
            sac_flies in 0u32..20,
        ) {
            let hits = if at_bats == 0 { 0 } else { hits % (at_bats + 1) };
            let obp = on_base_percentage(hits, walks, hbp, at_bats, sac_flies);
            match obp {
                None => prop_assert_eq!(at_bats + walks + hbp + sac_flies, 0),
                Some(value) => prop_assert!((0.0..=1.0).contains(&value)),
            }
        }

        #[test]
        fn slugging_matches_total_bases_identity(
            singles in 0u32..250,
            doubles in 0u32..80,
            triples in 0u32..20,
            home_runs in 0u32..80,
        ) {
            let hits = singles + doubles + triples + home_runs;
            let tb = total_bases(hits, doubles, triples, home_runs).unwrap();
            let slg = slugging(tb, hits.max(1)).unwrap();
            prop_assert!((0.0..=4.0).contains(&slg));
            if hits > 0 {
                prop_assert!((slg - f64::from(tb) / f64::from(hits)).abs() < 1e-9);
            }
        }

        #[test]
        fn era_and_whip_are_non_negative(er in 0u32..80, walks in 0u32..80, hits in 0u32..250, outs in 1u32..700) {
            let innings = Innings::from_outs(outs);
            let era = earned_run_average(er, innings).unwrap();
            let whip = whip(walks, hits, innings).unwrap();
            prop_assert!(era >= 0.0 && era.is_finite());
            prop_assert!(whip >= 0.0 && whip.is_finite());
            prop_assert!((era * innings.as_decimal() - 9.0 * f64::from(er)).abs() < 1e-6);
            prop_assert!((whip * innings.as_decimal() - f64::from(walks + hits)).abs() < 1e-6);
        }

        #[test]
        fn innings_notation_roundtrips(outs in 0u32..5_000) {
            let innings = Innings::from_outs(outs);
            let parsed = Innings::parse(&innings.notation()).unwrap();
            prop_assert_eq!(parsed, innings);
        }
    }
}
