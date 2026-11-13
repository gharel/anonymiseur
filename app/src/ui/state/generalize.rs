//! Généralisation (profil Renforcé) : une date devient son année, un âge sa
//! tranche de dix ans, un montant son ordre de grandeur. Irréversible : la
//! restauration ne remet pas la valeur exacte.

use anon_core::EntityKind;

/// Largeur d'une tranche d'âge, en années.
const AGE_BAND: u32 = 10;
/// Âge maximal plausible ; au-delà, la valeur n'est pas un âge.
const AGE_MAX: u32 = 130;
/// Années acceptées pour une date.
const YEAR_MIN: u32 = 1000;
const YEAR_MAX: u32 = 2999;
/// Nombre maximal de chiffres après la virgule d'un montant.
const MAX_DECIMALS: usize = 2;

/// Types pour lesquels la généralisation est proposée.
pub fn is_generalizable(kind: EntityKind) -> bool {
    matches!(
        kind,
        EntityKind::Date | EntityKind::DateNaiss | EntityKind::Age | EntityKind::Montant
    )
}

/// Forme généralisée de `value`, ou `None` si le type ne s'y prête pas ou si
/// la valeur n'a pas pu être lue.
pub fn generalize(kind: EntityKind, value: &str) -> Option<String> {
    match kind {
        EntityKind::Date | EntityKind::DateNaiss => year_of(value).map(|y| y.to_string()),
        EntityKind::Age => age_band(value),
        EntityKind::Montant => amount_magnitude(value),
        _ => None,
    }
}

/// Explication courte de la généralisation d'un type, pour l'interface.
pub fn rule_label(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Date | EntityKind::DateNaiss => "année seule",
        EntityKind::Age => "tranche de dix ans",
        EntityKind::Montant => "ordre de grandeur",
        _ => "",
    }
}

/// Suites de chiffres consécutifs, avec leur position (octets).
fn digit_runs(value: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in value.char_indices() {
        match (c.is_ascii_digit(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if let Some(run) = value.get(s..i) {
                    out.push((s, run));
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start
        && let Some(run) = value.get(s..)
    {
        out.push((s, run));
    }
    out
}

/// Année d'une date : un nombre de quatre chiffres entre 1000 et 2999.
fn year_of(value: &str) -> Option<u32> {
    digit_runs(value)
        .into_iter()
        .filter(|(_, r)| r.len() == 4)
        .filter_map(|(_, r)| r.parse::<u32>().ok())
        .find(|y| (YEAR_MIN..=YEAR_MAX).contains(y))
}

/// « 44 ans » → « 40-49 ans » ; l'unité suit la langue de la valeur.
fn age_band(value: &str) -> Option<String> {
    let (_, run) = digit_runs(value).into_iter().next()?;
    let n: u32 = run.parse().ok()?;
    if n > AGE_MAX {
        return None;
    }
    let low = n / AGE_BAND * AGE_BAND;
    let high = low + AGE_BAND - 1;
    let lower = value.to_lowercase();
    let unit = if lower.contains("year") {
        "years"
    } else if lower.contains("año") || lower.contains("ano") {
        "años"
    } else {
        "ans"
    };
    Some(format!("{low}-{high} {unit}"))
}

/// Groupe les chiffres par milliers avec une espace : « 1 000 000 ».
fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    let len = digits.len();
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
    // Dernier séparateur suivi d'une ou deux décimales : la virgule décimale.
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

/// « 105 000,00 EUR » → « entre 100 000 et 1 000 000 EUR ».
fn amount_magnitude(value: &str) -> Option<String> {
    let start = value.find(|c: char| c.is_ascii_digit())?;
    let rest = value.get(start..)?;
    let len = rest
        .find(|c: char| !is_amount_char(c))
        .unwrap_or(rest.len());
    let number = rest.get(..len)?;
    let n = integer_part(number)?;
    if n == 0 {
        return None;
    }
    let low = 10u64.checked_pow(n.ilog10())?;
    let high = low.checked_mul(10)?;
    // Devise : ce qui entoure le nombre (« EUR », « € », « USD », « XPF »).
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

#[cfg(test)]
mod tests {
    use super::*;
    use EntityKind::*;

    #[test]
    fn dates_become_years() {
        assert_eq!(generalize(Date, "14 mars 2026").as_deref(), Some("2026"));
        assert_eq!(generalize(DateNaiss, "12/05/1981").as_deref(), Some("1981"));
        assert_eq!(generalize(Date, "2024-03-14").as_deref(), Some("2024"));
        assert_eq!(generalize(Date, "March 14, 2026").as_deref(), Some("2026"));
        assert_eq!(
            generalize(Date, "14/03/26"),
            None,
            "année sur deux chiffres"
        );
        assert_eq!(generalize(Date, "12345"), None);
    }

    #[test]
    fn ages_become_bands() {
        assert_eq!(generalize(Age, "44 ans").as_deref(), Some("40-49 ans"));
        assert_eq!(generalize(Age, "62 ans").as_deref(), Some("60-69 ans"));
        assert_eq!(generalize(Age, "8 ans").as_deref(), Some("0-9 ans"));
        assert_eq!(
            generalize(Age, "41 years old").as_deref(),
            Some("40-49 years")
        );
        assert_eq!(generalize(Age, "52 años").as_deref(), Some("50-59 años"));
        assert_eq!(generalize(Age, "âgé de"), None);
        assert_eq!(generalize(Age, "250 ans"), None);
    }

    #[test]
    fn amounts_become_magnitudes() {
        assert_eq!(
            generalize(Montant, "105 000,00 EUR").as_deref(),
            Some("entre 100 000 et 1 000 000 EUR")
        );
        assert_eq!(
            generalize(Montant, "1250000").as_deref(),
            Some("entre 1 000 000 et 10 000 000")
        );
        assert_eq!(
            generalize(Montant, "USD 48,500.00").as_deref(),
            Some("entre 10 000 et 100 000 USD")
        );
        assert_eq!(
            generalize(Montant, "48.500,00 EUR").as_deref(),
            Some("entre 10 000 et 100 000 EUR")
        );
        assert_eq!(
            generalize(Montant, "12 500 000 XPF").as_deref(),
            Some("entre 10 000 000 et 100 000 000 XPF")
        );
        assert_eq!(
            generalize(Montant, "1.250 €").as_deref(),
            Some("entre 1 000 et 10 000 €"),
            "point des milliers"
        );
        assert_eq!(
            generalize(Montant, "9,99 €").as_deref(),
            Some("entre 1 et 10 €")
        );
        assert_eq!(generalize(Montant, "0,50 €"), None);
        assert_eq!(generalize(Montant, "gratuit"), None);
    }

    #[test]
    fn other_kinds_are_not_generalized() {
        assert!(!is_generalizable(Personne));
        assert_eq!(generalize(Personne, "Jean Dupont 1984"), None);
        for k in [Date, DateNaiss, Age, Montant] {
            assert!(is_generalizable(k));
            assert!(!rule_label(k).is_empty());
        }
    }

    #[test]
    fn thousands() {
        assert_eq!(group_thousands(1), "1");
        assert_eq!(group_thousands(1000), "1 000");
        assert_eq!(group_thousands(100_000_000), "100 000 000");
    }
}
