//! Génération d'une phrase de passe : mots français courants tirés au hasard.
//!
//! Format : `mot-mot-mot-mot-mot-NN` (mots en minuscules sans accents, nombre de 10 à 99).
//! Tirage par `getrandom` (générateur du système) avec rejet pour éviter le biais de modulo.

use crate::VaultError;

/// Nombre de mots par défaut. Avec 4 096 mots, 5 mots donnent 60 bits, plus 6,5 bits
/// pour le nombre final : environ 66 bits, hors de portée d'une attaque par force
/// brute sur Argon2id.
pub const DEFAULT_PASSPHRASE_WORDS: usize = 5;
/// Bornes du nombre de mots demandé (hors bornes, la valeur est ramenée dans les bornes).
pub const MIN_PASSPHRASE_WORDS: usize = 4;
pub const MAX_PASSPHRASE_WORDS: usize = 12;
const NUMBER_MIN: u32 = 10;
const NUMBER_COUNT: u32 = 90;
const SEPARATOR: char = '-';

static WORDS: &str = include_str!("../data/mots.txt");

fn words() -> impl Iterator<Item = &'static str> + Clone {
    WORDS.lines().filter(|w| !w.is_empty())
}

/// Entier uniforme dans `0..n` (`n > 0`), par rejet.
fn uniform_below(n: u32) -> Result<u32, VaultError> {
    if n == 0 {
        return Err(VaultError::Crypto);
    }
    // Plus grand multiple de `n` représentable sur 32 bits : au-delà, on rejette.
    let zone = (u64::from(u32::MAX) + 1) / u64::from(n) * u64::from(n);
    loop {
        let mut buf = [0u8; 4];
        getrandom::fill(&mut buf).map_err(|_| VaultError::Crypto)?;
        let r = u32::from_le_bytes(buf);
        if u64::from(r) < zone {
            return Ok(r % n);
        }
    }
}

/// Entropie, en bits, d'une phrase de `words_count` mots (borné comme à la génération).
pub fn passphrase_entropy_bits(words_count: usize) -> f64 {
    let n = words().count() as f64;
    let words_count = words_count.clamp(MIN_PASSPHRASE_WORDS, MAX_PASSPHRASE_WORDS) as f64;
    words_count * n.log2() + f64::from(NUMBER_COUNT).log2()
}

/// Génère une phrase de passe de `words_count` mots (entre 4 et 12), séparés par des
/// tirets, suivis d'un nombre à deux chiffres.
pub fn try_generate_passphrase(words_count: usize) -> Result<String, VaultError> {
    let list: Vec<&'static str> = words().collect();
    let count = u32::try_from(list.len()).map_err(|_| VaultError::Crypto)?;
    let words_count = words_count.clamp(MIN_PASSPHRASE_WORDS, MAX_PASSPHRASE_WORDS);
    let mut out = String::new();
    for _ in 0..words_count {
        let i = usize::try_from(uniform_below(count)?).map_err(|_| VaultError::Crypto)?;
        out.push_str(list.get(i).ok_or(VaultError::Crypto)?);
        out.push(SEPARATOR);
    }
    out.push_str(&(NUMBER_MIN + uniform_below(NUMBER_COUNT)?).to_string());
    Ok(out)
}

/// Comme [`try_generate_passphrase`], mais renvoie une chaîne **vide** si le générateur
/// du système échoue (cas pratiquement impossible) : l'appelant doit alors refuser
/// de continuer, jamais utiliser une valeur par défaut.
pub fn generate_passphrase(words_count: usize) -> String {
    try_generate_passphrase(words_count).unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::{PasswordStrength, password_strength};
    use std::collections::HashSet;

    #[test]
    fn word_list_is_valid() {
        let all: Vec<&str> = words().collect();
        assert!(all.len() >= 2048, "{}", all.len());
        let unique: HashSet<&&str> = all.iter().collect();
        assert_eq!(unique.len(), all.len(), "doublons");
        for w in &all {
            assert!((4..=8).contains(&w.len()), "{w}");
            assert!(w.bytes().all(|b| b.is_ascii_lowercase()), "{w}");
        }
    }

    #[test]
    fn shape_and_alphabet() {
        let p = generate_passphrase(DEFAULT_PASSPHRASE_WORDS);
        let parts: Vec<&str> = p.split('-').collect();
        assert_eq!(parts.len(), DEFAULT_PASSPHRASE_WORDS + 1);
        let all: HashSet<&str> = words().collect();
        for w in &parts[..DEFAULT_PASSPHRASE_WORDS] {
            assert!(all.contains(w), "{w}");
        }
        let n: u32 = parts[DEFAULT_PASSPHRASE_WORDS].parse().unwrap();
        assert!((10..=99).contains(&n));
        assert!(
            p.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        );
        assert!(p.len() >= DEFAULT_PASSPHRASE_WORDS * 5 + 2);
    }

    #[test]
    fn words_count_is_clamped() {
        assert_eq!(
            generate_passphrase(0).split('-').count(),
            MIN_PASSPHRASE_WORDS + 1
        );
        assert_eq!(
            generate_passphrase(99).split('-').count(),
            MAX_PASSPHRASE_WORDS + 1
        );
        assert_eq!(generate_passphrase(7).split('-').count(), 8);
    }

    #[test]
    fn generated_phrase_is_strong() {
        for _ in 0..50 {
            assert_eq!(
                password_strength(&generate_passphrase(DEFAULT_PASSPHRASE_WORDS)),
                PasswordStrength::Fort
            );
        }
    }

    #[test]
    fn documented_minimum_entropy() {
        // 4 096 mots : 12 bits par mot ; 5 mots + nombre de 10 à 99.
        assert!(passphrase_entropy_bits(DEFAULT_PASSPHRASE_WORDS) >= 66.0);
        assert!(passphrase_entropy_bits(MIN_PASSPHRASE_WORDS) >= 54.0);
    }

    #[test]
    fn draws_differ() {
        let set: HashSet<String> = (0..20)
            .map(|_| generate_passphrase(DEFAULT_PASSPHRASE_WORDS))
            .collect();
        assert_eq!(set.len(), 20);
    }

    #[test]
    fn uniform_stays_in_range_and_covers_it() {
        let mut seen = [0u32; 7];
        for _ in 0..7000 {
            seen[uniform_below(7).unwrap() as usize] += 1;
        }
        // Chaque valeur est attendue ~1000 fois ; 700 est très loin de l'aléa pur.
        assert!(seen.iter().all(|&c| c > 700), "{seen:?}");
        assert_eq!(uniform_below(1).unwrap(), 0);
        assert!(uniform_below(0).is_err());
    }
}
