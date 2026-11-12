//! Contrôles de clé : fonctions pures, sans état, sans panique.
//!
//! Toutes tolèrent les séparateurs usuels (espace, espace insécable, point,
//! tiret) et sont insensibles à la casse quand des lettres sont attendues.

/// Retire les séparateurs d'écriture courants.
fn compact(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '\u{a0}' | '\u{202f}' | '.' | '-'))
        .collect()
}

/// Chiffres d'une chaîne qui ne contient que des chiffres (après nettoyage).
fn digits(s: &str) -> Option<Vec<u32>> {
    let c = compact(s);
    if c.is_empty() {
        return None;
    }
    c.chars().map(|ch| ch.to_digit(10)).collect()
}

/// Valeur numérique d'une chaîne de chiffres ASCII (≤ 18 chiffres).
fn to_u64(ds: &[u32]) -> Option<u64> {
    if ds.len() > 18 {
        return None;
    }
    Some(ds.iter().fold(0u64, |acc, d| acc * 10 + u64::from(*d)))
}

/// Algorithme de Luhn sur une suite de chiffres.
pub fn luhn(s: &str) -> bool {
    let Some(ds) = digits(s) else {
        return false;
    };
    let sum: u32 = ds
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                *d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Longueur d'un IBAN selon le pays (code ISO sur deux lettres majuscules).
pub fn iban_length(country: &str) -> Option<usize> {
    let len = match country {
        // France et territoires (NC, PF, WF) : même structure que FR.
        "FR" | "NC" | "PF" | "WF" | "MC" | "GP" | "MQ" | "RE" | "GF" | "YT" | "PM" | "BL"
        | "MF" | "TF" | "IT" => 27,
        "BE" => 16,
        "DE" | "GB" | "IE" => 22,
        "ES" => 24,
        "LU" | "AT" => 20,
        "CH" | "LI" => 21,
        "NL" => 18,
        "PT" => 25,
        "PL" => 28,
        _ => return None,
    };
    Some(len)
}

/// IBAN valide : pays connu, longueur du pays, mod 97 égal à 1.
pub fn iban_valid(s: &str) -> bool {
    let c: String = compact_spaces(s).to_ascii_uppercase();
    if !c.is_ascii() {
        return false;
    }
    let (Some(cc), Some(check)) = (c.get(0..2), c.get(2..4)) else {
        return false;
    };
    if !cc.chars().all(|ch| ch.is_ascii_uppercase()) || !check.chars().all(|ch| ch.is_ascii_digit())
    {
        return false;
    }
    if iban_length(cc) != Some(c.len()) {
        return false;
    }
    let (Some(head), Some(tail)) = (c.get(..4), c.get(4..)) else {
        return false;
    };
    let mut rem: u32 = 0;
    for ch in tail.chars().chain(head.chars()) {
        match ch {
            '0'..='9' => rem = (rem * 10 + (u32::from(ch) - u32::from('0'))) % 97,
            'A'..='Z' => rem = (rem * 100 + (u32::from(ch) - u32::from('A')) + 10) % 97,
            _ => return false,
        }
    }
    rem == 1
}

/// Comme [`compact`] mais ne retire que les espaces (un IBAN n'a pas de tiret).
fn compact_spaces(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '\u{a0}' | '\u{202f}'))
        .collect()
}

/// N° de sécurité sociale (15 caractères) : clé 97, Corse 2A (19) et 2B (18).
pub fn nir_valid(s: &str) -> bool {
    let c = compact_spaces(s).to_ascii_uppercase();
    if c.len() != 15 || !c.is_ascii() {
        return false;
    }
    let (Some(base), Some(key)) = (c.get(..13), c.get(13..)) else {
        return false;
    };
    if !base.starts_with(['1', '2', '3', '4', '7', '8']) {
        return false;
    }
    let numeric = match base.get(5..7) {
        Some("2A") => format!(
            "{}19{}",
            base.get(..5).unwrap_or(""),
            base.get(7..).unwrap_or("")
        ),
        Some("2B") => format!(
            "{}18{}",
            base.get(..5).unwrap_or(""),
            base.get(7..).unwrap_or("")
        ),
        _ => base.to_string(),
    };
    let (Some(n), Ok(k)) = (
        numeric
            .chars()
            .map(|ch| ch.to_digit(10))
            .collect::<Option<Vec<u32>>>()
            .as_deref()
            .and_then(to_u64),
        key.parse::<u64>(),
    ) else {
        return false;
    };
    97 - n % 97 == k
}

/// SIREN : 9 chiffres, Luhn.
pub fn siren_valid(s: &str) -> bool {
    digits(s).is_some_and(|d| d.len() == 9) && luhn(s)
}

/// SIRET : 14 chiffres, Luhn (exception La Poste : somme des chiffres multiple de 5).
pub fn siret_valid(s: &str) -> bool {
    let Some(d) = digits(s) else {
        return false;
    };
    if d.len() != 14 {
        return false;
    }
    if d.iter().take(9).copied().eq([3, 5, 6, 0, 0, 0, 0, 0, 0]) {
        return d.iter().sum::<u32>() % 5 == 0;
    }
    luhn(s)
}

const DNI_LETTERS: &[u8; 23] = b"TRWAGMYFPDXBNJZSQVHLCKE";

fn dni_letter(n: u64) -> char {
    usize::try_from(n % 23)
        .ok()
        .and_then(|i| DNI_LETTERS.get(i))
        .map_or('?', |b| char::from(*b))
}

/// DNI espagnol : 8 chiffres et lettre de contrôle.
pub fn dni_es_valid(s: &str) -> bool {
    let c = compact(s).to_ascii_uppercase();
    if c.len() != 9 || !c.is_ascii() {
        return false;
    }
    let (Some(num), Some(letter)) = (c.get(..8), c.get(8..)) else {
        return false;
    };
    let Ok(n) = num.parse::<u64>() else {
        return false;
    };
    num.chars().all(|ch| ch.is_ascii_digit()) && letter.starts_with(dni_letter(n))
}

/// NIE espagnol : X, Y ou Z, 7 chiffres et lettre de contrôle.
pub fn nie_es_valid(s: &str) -> bool {
    let c = compact(s).to_ascii_uppercase();
    if c.len() != 9 || !c.is_ascii() {
        return false;
    }
    let (Some(first), Some(num), Some(letter)) = (c.get(..1), c.get(1..8), c.get(8..)) else {
        return false;
    };
    let prefix = match first {
        "X" => 0u64,
        "Y" => 1,
        "Z" => 2,
        _ => return false,
    };
    let Ok(n) = num.parse::<u64>() else {
        return false;
    };
    num.chars().all(|ch| ch.is_ascii_digit())
        && letter.starts_with(dni_letter(prefix * 10_000_000 + n))
}

const CF_ODD: [u32; 26] = [
    1, 0, 5, 7, 9, 13, 15, 17, 19, 21, 2, 4, 18, 20, 11, 3, 6, 8, 12, 14, 16, 10, 22, 25, 24, 23,
];
const CF_ODD_DIGITS: [u32; 10] = [1, 0, 5, 7, 9, 13, 15, 17, 19, 21];

/// Codice fiscale italien : 16 caractères, caractère de contrôle.
pub fn codice_fiscale_valid(s: &str) -> bool {
    let c = compact_spaces(s).to_ascii_uppercase();
    if c.len() != 16 || !c.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return false;
    }
    let mut sum = 0u32;
    for (i, ch) in c.chars().take(15).enumerate() {
        let value = if ch.is_ascii_digit() {
            let d = ch.to_digit(10).unwrap_or(0);
            if i % 2 == 0 {
                CF_ODD_DIGITS.get(d as usize).copied()
            } else {
                Some(d)
            }
        } else {
            let idx = u32::from(ch) - u32::from('A');
            if i % 2 == 0 {
                CF_ODD.get(idx as usize).copied()
            } else {
                Some(idx)
            }
        };
        match value {
            Some(v) => sum += v,
            None => return false,
        }
    }
    let expected = char::from_u32(u32::from('A') + sum % 26);
    c.chars().nth(15) == expected
}

/// Numéro de registre national belge (NISS) : 11 chiffres, clé 97
/// (préfixe « 2 » ajouté aux 9 premiers chiffres pour les naissances ≥ 2000).
pub fn niss_be_valid(s: &str) -> bool {
    let Some(d) = digits(s) else {
        return false;
    };
    if d.len() != 11 {
        return false;
    }
    let (Some(base), Some(key)) = (d.get(..9).and_then(to_u64), d.get(9..).and_then(to_u64)) else {
        return false;
    };
    97 - base % 97 == key || 97 - (2_000_000_000 + base) % 97 == key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luhn_cases() {
        assert!(luhn("4111111111111111"));
        assert!(luhn("4111 1111 1111 1111"));
        assert!(luhn("378282246310005"));
        assert!(!luhn("4111111111111112"));
        assert!(!luhn(""));
        assert!(!luhn("41a1"));
    }

    #[test]
    fn iban_cases() {
        for ok in [
            "FR1420041010050500013M02606",
            "FR76 3000 6000 0112 3456 7890 189",
            "FR76\u{a0}3000\u{a0}6000\u{a0}0112\u{a0}3456\u{a0}7890\u{a0}189",
            "NC49 3000 6000 0112 3456 7890 189",
            "PF22 3000 6000 0112 3456 7890 189",
            "WF56 3000 6000 0112 3456 7890 189",
            "GB82 WEST 1234 5698 7654 32",
            "DE89 3704 0044 0532 0130 00",
            "BE68 5390 0754 7034",
            "ES91 2100 0418 4502 0005 1332",
            "IT60 X054 2811 1010 0000 0123 456",
            "LU28 0019 4006 4475 0000",
            "CH93 0076 2011 6238 5295 7",
            "NL91 ABNA 0417 1643 00",
            "PT50 0002 0123 1234 5678 9015 4",
            "MC58 1122 2000 0101 2345 6789 030",
        ] {
            assert!(iban_valid(ok), "{ok}");
        }
        for bad in [
            "FR77 3000 6000 0112 3456 7890 189",
            "FR76 3000 6000 0112 3456 7890 18",
            "XX76 3000 6000 0112 3456 7890 189",
            "",
            "FR",
            "FR76é000600001123456789018",
        ] {
            assert!(!iban_valid(bad), "{bad}");
        }
    }

    #[test]
    fn nir_cases() {
        assert!(nir_valid("184037512000549"));
        assert!(nir_valid("1 84 03 75 120 005 49"));
        assert!(nir_valid("291072A00401249"));
        assert!(nir_valid("190062B00123427"));
        assert!(!nir_valid("184037512000548"));
        // la clé 2A se calcule avec 19 : en 20 elle serait fausse
        assert!(!nir_valid("291072A00401250"));
        assert!(!nir_valid("084037512000549"));
        assert!(!nir_valid("18403751200054"));
    }

    #[test]
    fn siren_siret_cases() {
        assert!(siren_valid("732 829 320"));
        assert!(siren_valid("123456782"));
        assert!(!siren_valid("123456789"));
        assert!(siret_valid("732 829 320 00074"));
        assert!(siret_valid("73282932000009"));
        assert!(!siret_valid("73282932000075"));
        assert!(!siret_valid("7328293200007"));
        // exception La Poste (356 000 000) : somme des chiffres multiple de 5
        assert!(siret_valid("35600000000001"));
        assert!(!siret_valid("35600000000002"));
    }

    #[test]
    fn spanish_ids() {
        assert!(dni_es_valid("12345678Z"));
        assert!(dni_es_valid("12345678-z"));
        assert!(!dni_es_valid("12345678A"));
        assert!(!dni_es_valid("1234567Z"));
        assert!(nie_es_valid("X1234567L"));
        assert!(nie_es_valid("x-1234567-l"));
        assert!(!nie_es_valid("X1234567A"));
        assert!(!nie_es_valid("A1234567L"));
    }

    #[test]
    fn codice_fiscale_cases() {
        assert!(codice_fiscale_valid("RSSMRA85T10A562S"));
        assert!(codice_fiscale_valid("rssmra85t10a562s"));
        assert!(!codice_fiscale_valid("RSSMRA85T10A562T"));
        assert!(!codice_fiscale_valid("RSSMRA85T10A562"));
        assert!(!codice_fiscale_valid("RSSMRA85T10A56?S"));
    }

    #[test]
    fn niss_cases() {
        assert!(niss_be_valid("85.07.30-033.28"));
        assert!(niss_be_valid("85073003328"));
        assert!(niss_be_valid("05.07.30-033.26"));
        assert!(!niss_be_valid("85.07.30-033.29"));
        assert!(!niss_be_valid("8507300332"));
    }
}
