//! Hiérarchies de généralisation (« rendre moins précis ») pour l'évaluation des
//! trois critères et ses corrections. Chaque niveau est moins précis que le
//! précédent et l'englobe : deux valeurs égales à un niveau le restent aux
//! niveaux supérieurs (propriété dont dépend l'algorithme de type Datafly).
//!
//! | Type | Niveau 1 | Niveau 2 | Niveau 3 |
//! |---|---|---|---|
//! | date, date de naissance | mois et année | année | décennie |
//! | âge | tranche de 5 ans | tranche de 10 ans | |
//! | lieu (commune) | département ou province | région | territoire |
//! | lieu (code postal) | deux premiers chiffres et étoiles | | |
//! | montant | tranche du premier chiffre | ordre de grandeur | deux ordres de grandeur |
//! | fonction | catégorie plus large | « professionnel » | |
//!
//! Reprend la logique de `app/src/ui/state/generalize.rs` (V1) : lecture des
//! dates, âges et montants, unités et devises conservées.

use crate::fold::fold;
use crate::population::{is_feminine, occupation, places};
use anon_core::EntityKind;

/// Âge maximal plausible ; au-delà, la valeur n'est pas un âge.
const AGE_MAX: u32 = 130;
/// Largeur de la tranche d'âge du niveau 1, en années.
const AGE_BAND_FINE: u32 = 5;
/// Largeur de la tranche d'âge du niveau 2, en années.
const AGE_BAND_WIDE: u32 = 10;
/// Années acceptées pour une date.
const YEAR_MIN: u32 = 1000;
const YEAR_MAX: u32 = 2999;
/// Largeur d'une décennie.
const DECADE: u32 = 10;
/// Nombre maximal de chiffres après la virgule d'un montant.
const MAX_DECIMALS: usize = 2;
/// Nombre de chiffres d'un code postal.
const POSTAL_CODE_DIGITS: usize = 5;
/// Chiffres gardés d'un code postal généralisé (« 54*** »).
const POSTAL_CODE_KEPT: usize = 2;
/// Ordres de grandeur regroupés au niveau 3 d'un montant.
const AMOUNT_MAGNITUDES_GROUPED: u32 = 2;

/// Noms de mois pliés (français, anglais, espagnol, abréviations), et numéro.
const MONTHS: &[(&str, u32)] = &[
    ("janvier", 1),
    ("janv", 1),
    ("january", 1),
    ("jan", 1),
    ("enero", 1),
    ("fevrier", 2),
    ("fevr", 2),
    ("fev", 2),
    ("february", 2),
    ("feb", 2),
    ("febrero", 2),
    ("mars", 3),
    ("march", 3),
    ("mar", 3),
    ("marzo", 3),
    ("avril", 4),
    ("avr", 4),
    ("april", 4),
    ("apr", 4),
    ("abril", 4),
    ("mai", 5),
    ("may", 5),
    ("mayo", 5),
    ("juin", 6),
    ("june", 6),
    ("jun", 6),
    ("junio", 6),
    ("juillet", 7),
    ("juil", 7),
    ("july", 7),
    ("jul", 7),
    ("julio", 7),
    ("aout", 8),
    ("august", 8),
    ("aug", 8),
    ("agosto", 8),
    ("septembre", 9),
    ("sept", 9),
    ("september", 9),
    ("sep", 9),
    ("septiembre", 9),
    ("octobre", 10),
    ("oct", 10),
    ("october", 10),
    ("octubre", 10),
    ("novembre", 11),
    ("nov", 11),
    ("november", 11),
    ("noviembre", 11),
    ("decembre", 12),
    ("dec", 12),
    ("december", 12),
    ("diciembre", 12),
];

/// Noms français des mois, pour l'affichage.
const MONTH_NAMES: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

/// Niveau le plus général d'un type (0 : pas de généralisation possible).
pub fn max_level(kind: EntityKind) -> u8 {
    match kind {
        EntityKind::Date | EntityKind::DateNaiss | EntityKind::Lieu | EntityKind::Montant => 3,
        EntityKind::Age | EntityKind::Fonction => 2,
        _ => 0,
    }
}

/// Forme moins précise de `value` au niveau `level` (1 = un cran), `None` si
/// impossible (type sans hiérarchie, valeur illisible, niveau trop élevé pour
/// cette valeur). Le niveau 0 rend la valeur telle quelle.
pub fn generalize(kind: EntityKind, value: &str, level: u8) -> Option<String> {
    if level == 0 {
        return Some(value.to_string());
    }
    if level > max_level(kind) || value.trim().is_empty() {
        return None;
    }
    match kind {
        EntityKind::Date | EntityKind::DateNaiss => date(value, level),
        EntityKind::Age => age(value, level),
        EntityKind::Lieu => place(value, level),
        EntityKind::Montant => amount(value, level),
        EntityKind::Fonction => function(value, level),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Dates

/// Suites de chiffres consécutifs.
fn digit_runs(value: &str) -> Vec<&str> {
    value
        .split(|c: char| !c.is_ascii_digit())
        .filter(|r| !r.is_empty())
        .collect()
}

/// Année et, si elle se lit, mois d'une date.
fn year_month(value: &str) -> Option<(u32, Option<u32>)> {
    let runs = digit_runs(value);
    let year_pos = runs.iter().position(|r| {
        r.len() == 4
            && r.parse::<u32>()
                .is_ok_and(|y| (YEAR_MIN..=YEAR_MAX).contains(&y))
    })?;
    let year: u32 = runs.get(year_pos)?.parse().ok()?;
    let as_month = |i: usize| {
        runs.get(i)
            .and_then(|r| r.parse::<u32>().ok())
            .filter(|m| (1..=12).contains(m))
    };
    // Nom de mois écrit en toutes lettres.
    let folded = fold(value);
    let named = folded
        .split(|c: char| !c.is_alphabetic())
        .find_map(|w| MONTHS.iter().find(|(n, _)| *n == w).map(|(_, m)| *m));
    let numeric = match (year_pos, runs.len()) {
        // JJ/MM/AAAA
        (2, 3) => as_month(1),
        // MM/AAAA
        (1, 2) => as_month(0),
        // AAAA-MM-JJ ou AAAA-MM
        (0, 2..=3) => as_month(1),
        _ => None,
    };
    Some((year, named.or(numeric)))
}

fn date(value: &str, level: u8) -> Option<String> {
    let (year, month) = year_month(value)?;
    match level {
        1 => {
            let name = month
                .and_then(|m| usize::try_from(m.checked_sub(1)?).ok())
                .and_then(|i| MONTH_NAMES.get(i))?;
            Some(format!("{name} {year}"))
        }
        2 => Some(year.to_string()),
        3 => Some(format!("années {}", year / DECADE * DECADE)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Âges

/// Unité d'âge selon la langue de la valeur (français par défaut).
fn age_unit(value: &str) -> &'static str {
    let lower = fold(value);
    if lower.contains("year") {
        "years"
    } else if lower.contains("ano") {
        "años"
    } else {
        "ans"
    }
}

fn age(value: &str, level: u8) -> Option<String> {
    let n: u32 = digit_runs(value).first()?.parse().ok()?;
    if n > AGE_MAX {
        return None;
    }
    let width = match level {
        1 => AGE_BAND_FINE,
        2 => AGE_BAND_WIDE,
        _ => return None,
    };
    let low = n / width * width;
    let high = low + width - 1;
    Some(format!("{low}-{high} {}", age_unit(value)))
}

// ---------------------------------------------------------------------------
// Lieux

/// Code postal : cinq chiffres, ou forme déjà généralisée (« 54*** »).
fn postal_prefix(value: &str) -> Option<&str> {
    let v = value.trim();
    let prefix = v.get(..POSTAL_CODE_KEPT)?;
    let rest = v.get(POSTAL_CODE_KEPT..)?;
    let ok = v.chars().count() == POSTAL_CODE_DIGITS
        && prefix.bytes().all(|b| b.is_ascii_digit())
        && (rest.bytes().all(|b| b.is_ascii_digit()) || rest.bytes().all(|b| b == b'*'));
    ok.then_some(prefix)
}

fn place(value: &str, level: u8) -> Option<String> {
    if let Some(prefix) = postal_prefix(value) {
        return (level == 1).then(|| {
            format!(
                "{prefix}{}",
                "*".repeat(POSTAL_CODE_DIGITS - POSTAL_CODE_KEPT)
            )
        });
    }
    let candidates = places(value);
    if candidates.is_empty() {
        return None;
    }
    // Homonymes : on remonte jusqu'au premier niveau où ils s'accordent, pour ne
    // jamais écrire un département qui n'est pas le bon.
    (level..=3).find_map(|l| {
        let mut labels = candidates.iter().map(|p| match l {
            1 => p.area,
            2 => p.region,
            _ => p.territory.label(),
        });
        let first = labels.next()?;
        labels.all(|x| x == first).then(|| first.to_string())
    })
}

// ---------------------------------------------------------------------------
// Montants

/// Groupe les chiffres par milliers avec une espace : « 1 000 000 ».
fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn is_amount_char(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, ' ' | '.' | ',' | '\'' | '\u{a0}' | '\u{202f}')
}

/// Partie entière d'un montant écrit « 105 000,00 », « 48,500.00 », « 1250000 ».
fn integer_part(number: &str) -> Option<u64> {
    let number = number.trim_matches(|c: char| !c.is_ascii_digit());
    let mut integer = number;
    if let Some(pos) = number.rfind(['.', ',']) {
        let decimals = number.get(pos + 1..).unwrap_or("");
        if (1..=MAX_DECIMALS).contains(&decimals.len())
            && decimals.chars().all(|c| c.is_ascii_digit())
        {
            integer = number.get(..pos).unwrap_or("");
        }
    }
    let digits: String = integer.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

fn amount(value: &str, level: u8) -> Option<String> {
    let start = value.find(|c: char| c.is_ascii_digit())?;
    let rest = value.get(start..)?;
    let len = rest
        .find(|c: char| !is_amount_char(c))
        .unwrap_or(rest.len());
    let n = integer_part(rest.get(..len)?)?;
    if n == 0 {
        return None;
    }
    let k = n.ilog10();
    let unit = 10u64.checked_pow(k)?;
    let (low, high) = match level {
        1 => {
            let d = n / unit;
            (d.checked_mul(unit)?, d.checked_add(1)?.checked_mul(unit)?)
        }
        2 => (unit, unit.checked_mul(10)?),
        3 => {
            let base = k / AMOUNT_MAGNITUDES_GROUPED * AMOUNT_MAGNITUDES_GROUPED;
            (
                10u64.checked_pow(base)?,
                10u64.checked_pow(base + AMOUNT_MAGNITUDES_GROUPED)?,
            )
        }
        _ => return None,
    };
    // Devise : ce qui entoure le nombre (« EUR », « € », « XPF »).
    let before = value.get(..start).unwrap_or("").trim();
    let after = rest.get(len..).unwrap_or("").trim();
    let currency = [before, after]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = format!(
        "entre {} et {}",
        group_thousands(low),
        group_thousands(high)
    );
    if !currency.is_empty() {
        out.push(' ');
        out.push_str(&currency);
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Fonctions

fn function(value: &str, level: u8) -> Option<String> {
    let known = occupation(value);
    match level {
        1 => known.map(|o| o.broader.to_string()),
        2 => Some(
            if known.is_some_and(|o| is_feminine(o.broader)) {
                "professionnelle"
            } else {
                "professionnel"
            }
            .to_string(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use EntityKind::*;

    fn g(kind: EntityKind, v: &str, l: u8) -> Option<String> {
        generalize(kind, v, l)
    }

    #[test]
    fn dates() {
        assert_eq!(g(Date, "14/03/2026", 1).as_deref(), Some("mars 2026"));
        assert_eq!(g(Date, "14 mars 2026", 1).as_deref(), Some("mars 2026"));
        assert_eq!(g(Date, "2024-02-29", 1).as_deref(), Some("février 2024"));
        assert_eq!(g(Date, "March 14, 2026", 1).as_deref(), Some("mars 2026"));
        assert_eq!(g(DateNaiss, "12/08/1946", 1).as_deref(), Some("août 1946"));
        assert_eq!(g(DateNaiss, "12/08/1946", 2).as_deref(), Some("1946"));
        assert_eq!(
            g(DateNaiss, "12/08/1946", 3).as_deref(),
            Some("années 1940")
        );
        // Année seule : pas de mois, mais l'année et la décennie.
        assert_eq!(g(DateNaiss, "1946", 1), None);
        assert_eq!(g(DateNaiss, "1946", 2).as_deref(), Some("1946"));
        assert_eq!(g(Date, "14/03/26", 2), None, "année sur deux chiffres");
        assert_eq!(g(Date, "14/03/2026", 4), None);
        assert_eq!(g(Date, "14/03/2026", 0).as_deref(), Some("14/03/2026"));
    }

    #[test]
    fn ages() {
        assert_eq!(g(Age, "62 ans", 1).as_deref(), Some("60-64 ans"));
        assert_eq!(g(Age, "62 ans", 2).as_deref(), Some("60-69 ans"));
        assert_eq!(g(Age, "67", 1).as_deref(), Some("65-69 ans"));
        assert_eq!(g(Age, "8 ans", 2).as_deref(), Some("0-9 ans"));
        assert_eq!(g(Age, "41 years old", 2).as_deref(), Some("40-49 years"));
        assert_eq!(g(Age, "52 años", 1).as_deref(), Some("50-54 años"));
        assert_eq!(g(Age, "250 ans", 1), None);
        assert_eq!(g(Age, "âgé de", 1), None);
        assert_eq!(g(Age, "62 ans", 3), None);
    }

    #[test]
    fn places_climb_the_hierarchy() {
        assert_eq!(g(Lieu, "Farino", 1).as_deref(), Some("province Sud"));
        assert_eq!(g(Lieu, "Farino", 2).as_deref(), Some("Nouvelle-Calédonie"));
        assert_eq!(g(Lieu, "Farino", 3).as_deref(), Some("Nouvelle-Calédonie"));
        assert_eq!(g(Lieu, "Nancy", 1).as_deref(), Some("Meurthe-et-Moselle"));
        assert_eq!(g(Lieu, "Nancy", 2).as_deref(), Some("Grand Est"));
        assert_eq!(g(Lieu, "Nancy", 3).as_deref(), Some("France"));
        assert_eq!(g(Lieu, "Atlantis", 1), None);
    }

    #[test]
    fn homonyms_never_get_a_wrong_area() {
        // « Saint-Denis » existe dans plusieurs départements : le niveau 1 remonte
        // au territoire, seul niveau commun à tous.
        let sd = g(Lieu, "Saint-Denis", 1).unwrap();
        assert_eq!(sd, "France");
        // « Mont-Dore » : France et Nouvelle-Calédonie, aucun niveau commun.
        assert_eq!(g(Lieu, "Mont-Dore", 1), None);
        assert_eq!(
            g(Lieu, "Le Mont-Dore", 1).as_deref(),
            Some("province Sud"),
            "la forme avec article est propre à la commune calédonienne"
        );
    }

    #[test]
    fn postal_codes() {
        assert_eq!(g(Lieu, "54000", 1).as_deref(), Some("54***"));
        assert_eq!(g(Lieu, "98800", 1).as_deref(), Some("98***"));
        assert_eq!(g(Lieu, "54***", 1).as_deref(), Some("54***"));
        assert_eq!(g(Lieu, "54000", 2), None);
        assert_eq!(g(Lieu, "5400", 1), None);
    }

    #[test]
    fn amounts() {
        assert_eq!(
            g(Montant, "1 250 000 XPF", 1).as_deref(),
            Some("entre 1 000 000 et 2 000 000 XPF")
        );
        assert_eq!(
            g(Montant, "1 250 000 XPF", 2).as_deref(),
            Some("entre 1 000 000 et 10 000 000 XPF")
        );
        assert_eq!(
            g(Montant, "1 250 000 XPF", 3).as_deref(),
            Some("entre 1 000 000 et 100 000 000 XPF")
        );
        assert_eq!(
            g(Montant, "105 000,00 EUR", 2).as_deref(),
            Some("entre 100 000 et 1 000 000 EUR")
        );
        assert_eq!(
            g(Montant, "USD 48,500.00", 1).as_deref(),
            Some("entre 40 000 et 50 000 USD")
        );
        assert_eq!(
            g(Montant, "840000", 3).as_deref(),
            Some("entre 10 000 et 1 000 000")
        );
        assert_eq!(g(Montant, "9,99 €", 1).as_deref(), Some("entre 9 et 10 €"));
        assert_eq!(g(Montant, "0,50 €", 1), None);
        assert_eq!(g(Montant, "gratuit", 1), None);
    }

    #[test]
    fn functions() {
        assert_eq!(
            g(Fonction, "infirmière", 1).as_deref(),
            Some("professionnelle de santé")
        );
        assert_eq!(
            g(Fonction, "infirmière", 2).as_deref(),
            Some("professionnelle")
        );
        assert_eq!(g(Fonction, "gérant", 2).as_deref(), Some("professionnel"));
        assert_eq!(g(Fonction, "astronaute", 1), None);
        assert_eq!(
            g(Fonction, "astronaute", 2).as_deref(),
            Some("professionnel")
        );
    }

    #[test]
    fn other_kinds_have_no_hierarchy() {
        for k in [Personne, Email, Sante, Autre] {
            assert_eq!(max_level(k), 0);
            assert_eq!(g(k, "valeur 1946", 1), None);
        }
        assert_eq!(g(Lieu, "   ", 1), None);
    }

    /// Chaque niveau englobe le précédent : deux valeurs égales au niveau n le
    /// restent au niveau n + 1.
    #[test]
    fn levels_are_nested() {
        let samples: &[(EntityKind, &[&str])] = &[
            (
                Age,
                &["60 ans", "61 ans", "64 ans", "65 ans", "69 ans", "70 ans"],
            ),
            (
                DateNaiss,
                &[
                    "01/01/1946",
                    "31/01/1946",
                    "01/02/1946",
                    "1949-12-31",
                    "1950-01-01",
                ],
            ),
            (
                Montant,
                &[
                    "1 000", "1 999", "2 000", "9 999", "10 000", "99 999", "100 000",
                ],
            ),
            (
                Lieu,
                &["Farino", "Bourail", "Koné", "Nancy", "Metz", "Paris"],
            ),
        ];
        for (kind, values) in samples {
            for l in 1..max_level(*kind) {
                for a in values.iter() {
                    for b in values.iter() {
                        let (ga, gb) = (g(*kind, a, l), g(*kind, b, l));
                        if ga.is_some() && ga == gb {
                            assert_eq!(g(*kind, a, l + 1), g(*kind, b, l + 1), "{a} / {b} ({l})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn exotic_input_does_not_panic() {
        for k in EntityKind::ALL {
            for v in [
                "",
                "😀",
                "99999999999999999999999 €",
                "31/02/9999",
                "\u{0301}",
                "*****",
            ] {
                for l in 0..=4 {
                    let _ = g(*k, v, l);
                }
            }
        }
    }
}
