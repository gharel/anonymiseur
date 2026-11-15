//! Dates de conservation de la table de correspondance (`created_on`, `keep_until`).
//!
//! Format `AAAA-MM-JJ` (calendrier grégorien, années 0001 à 9999). Aucune dépendance :
//! la conversion jours depuis l'époque / date civile suit l'algorithme de Howard Hinnant.
//! Aucune fonction ne panique : une date invalide donne `None` ou `false`.

use std::time::{SystemTime, UNIX_EPOCH};

/// Durée de conservation proposée par défaut, en mois.
pub const DEFAULT_RETENTION_MONTHS: u32 = 12;

const SECONDS_PER_DAY: u64 = 86_400;
const MONTHS_PER_YEAR: u32 = 12;
const MIN_YEAR: i64 = 1;
const MAX_YEAR: i64 = 9999;
/// Jours entre le 0000-03-01 (début de l'ère de l'algorithme) et le 1970-01-01.
const DAYS_TO_EPOCH: i64 = 719_468;
/// Jours par cycle grégorien de 400 ans.
const DAYS_PER_ERA: i64 = 146_097;

/// Date du jour, **en UTC**, au format `AAAA-MM-JJ`.
///
/// Choix : l'UTC évite toute dépendance à un fuseau et reste identique d'une machine
/// à l'autre ; l'écart avec la date locale n'excède pas un jour, sans effet sur une
/// durée de conservation exprimée en mois. Si l'horloge est antérieure à 1970,
/// renvoie `1970-01-01`.
pub fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = i64::try_from(secs / SECONDS_PER_DAY).unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    format_date(y, m, d)
}

/// Ajoute `months` mois à `date` ; si le jour n'existe pas dans le mois d'arrivée,
/// on prend le dernier jour du mois (31 janvier + 1 mois = 28 ou 29 février).
/// `None` si `date` est invalide ou si le résultat dépasse l'an 9999.
pub fn add_months(date: &str, months: u32) -> Option<String> {
    let (y, m, d) = parse_date(date)?;
    let index = y
        .checked_mul(i64::from(MONTHS_PER_YEAR))?
        .checked_add(i64::from(m) - 1)?
        .checked_add(i64::from(months))?;
    let year = index.div_euclid(i64::from(MONTHS_PER_YEAR));
    let month = u32::try_from(index.rem_euclid(i64::from(MONTHS_PER_YEAR))).ok()? + 1;
    if year > MAX_YEAR {
        return None;
    }
    let day = d.min(days_in_month(year, month));
    Some(format_date(year, month, day))
}

/// `true` si `date` est strictement antérieure à `today`. Une date invalide (l'une
/// ou l'autre) donne `false` : on ne propose jamais de supprimer sur une donnée douteuse.
pub fn is_past(date: &str, today: &str) -> bool {
    match (parse_date(date), parse_date(today)) {
        (Some(a), Some(b)) => a < b,
        _ => false,
    }
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

fn format_date(y: i64, m: u32, d: u32) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

/// Analyse stricte de `AAAA-MM-JJ` (10 caractères ASCII, chiffres et deux tirets).
fn parse_date(date: &str) -> Option<(i64, u32, u32)> {
    let b = date.as_bytes();
    if b.len() != 10 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return None;
    }
    let num = |range: std::ops::Range<usize>| -> Option<u32> {
        let digits = b.get(range)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        digits.iter().try_fold(0u32, |acc, d| {
            acc.checked_mul(10)?.checked_add(u32::from(d - b'0'))
        })
    };
    let year = i64::from(num(0..4)?);
    let month = num(5..7)?;
    let day = num(8..10)?;
    if !(MIN_YEAR..=MAX_YEAR).contains(&year)
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
    {
        return None;
    }
    Some((year, month, day))
}

/// Jours depuis 1970-01-01 vers (année, mois, jour).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + DAYS_TO_EPOCH;
    let era = z.div_euclid(DAYS_PER_ERA);
    let doe = z.rem_euclid(DAYS_PER_ERA);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    // Bornés par construction : jour 1..=31, mois 1..=12.
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn civil_conversion_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(365), (1971, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_407), (2025, 11, 15));
    }

    #[test]
    fn today_is_a_valid_recent_date() {
        let t = today();
        assert!(parse_date(&t).is_some(), "{t}");
        assert!(t.as_str() >= "2026-01-01");
    }

    #[test]
    fn add_months_simple_and_year_wrap() {
        assert_eq!(add_months("2026-11-15", 12).unwrap(), "2027-11-15");
        assert_eq!(add_months("2026-11-15", 1).unwrap(), "2026-12-15");
        assert_eq!(add_months("2026-11-15", 2).unwrap(), "2027-01-15");
        assert_eq!(add_months("2026-12-31", 0).unwrap(), "2026-12-31");
        assert_eq!(add_months("2026-01-10", 25).unwrap(), "2028-02-10");
    }

    #[test]
    fn add_months_end_of_month() {
        assert_eq!(add_months("2027-01-31", 1).unwrap(), "2027-02-28");
        assert_eq!(add_months("2028-01-31", 1).unwrap(), "2028-02-29");
        assert_eq!(add_months("2026-03-31", 1).unwrap(), "2026-04-30");
        assert_eq!(add_months("2024-02-29", 12).unwrap(), "2025-02-28");
        assert_eq!(add_months("2024-02-29", 48).unwrap(), "2028-02-29");
    }

    #[test]
    fn leap_years() {
        assert!(is_leap(2024) && is_leap(2000));
        assert!(!is_leap(1900) && !is_leap(2100) && !is_leap(2026));
        assert!(parse_date("2024-02-29").is_some());
        assert!(parse_date("2026-02-29").is_none());
        assert!(parse_date("1900-02-29").is_none());
    }

    #[test]
    fn invalid_dates_give_none_never_panic() {
        for bad in [
            "",
            "2026",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "2026-11-00",
            "2026/11/15",
            "26-11-15",
            "2026-1-15",
            "2026-11-5 ",
            "0000-01-01",
            "abcd-ef-gh",
            "２０２６-11-15",
            "2026-11-15T00:00",
            "-026-11-15",
            "+026-11-15",
        ] {
            assert_eq!(add_months(bad, 1), None, "{bad:?}");
            assert!(!is_past(bad, "2026-11-15"), "{bad:?}");
            assert!(!is_past("2020-01-01", bad), "{bad:?}");
        }
    }

    #[test]
    fn add_months_overflow_is_none() {
        assert_eq!(add_months("9999-12-31", 1), None);
        assert_eq!(add_months("2026-01-01", u32::MAX), None);
        assert_eq!(add_months("9999-01-31", 11).unwrap(), "9999-12-31");
    }

    #[test]
    fn past_is_strict() {
        assert!(is_past("2026-11-14", "2026-11-15"));
        assert!(!is_past("2026-11-15", "2026-11-15"));
        assert!(!is_past("2027-01-01", "2026-11-15"));
        assert!(is_past("2025-12-31", "2026-01-01"));
    }

    #[test]
    fn default_retention_is_twelve_months() {
        assert_eq!(DEFAULT_RETENTION_MONTHS, 12);
    }
}
