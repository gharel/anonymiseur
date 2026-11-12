//! Estimation de la robustesse d'un mot de passe, pour la jauge.
//!
//! Estimation volontairement simple (longueur × variété des caractères, avec
//! pénalités pour les répétitions et les suites) : elle guide l'utilisateur,
//! elle ne remplace pas la dérivation de clé du coffre.

/// Longueur minimale acceptée pour chiffrer la table.
pub const MIN_LENGTH: usize = 10;

/// Seuils d'entropie estimée (bits).
const BITS_WEAK: f32 = 40.0;
const BITS_GOOD: f32 = 60.0;
const BITS_STRONG: f32 = 80.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrengthLevel {
    Empty,
    TooWeak,
    Weak,
    Good,
    Strong,
}

impl StrengthLevel {
    pub fn label(self) -> &'static str {
        match self {
            StrengthLevel::Empty => "à saisir",
            StrengthLevel::TooWeak => "trop faible",
            StrengthLevel::Weak => "moyenne",
            StrengthLevel::Good => "bonne",
            StrengthLevel::Strong => "très bonne",
        }
    }

    /// Nombre de segments allumés sur quatre.
    pub fn segments(self) -> u8 {
        match self {
            StrengthLevel::Empty => 0,
            StrengthLevel::TooWeak => 1,
            StrengthLevel::Weak => 2,
            StrengthLevel::Good => 3,
            StrengthLevel::Strong => 4,
        }
    }

    /// Niveau suffisant pour chiffrer la table.
    pub fn is_acceptable(self) -> bool {
        self >= StrengthLevel::Weak
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strength {
    pub level: StrengthLevel,
    pub bits: f32,
}

/// Évalue un mot de passe.
pub fn evaluate(password: &str) -> Strength {
    let chars: Vec<char> = password.chars().collect();
    if chars.is_empty() {
        return Strength {
            level: StrengthLevel::Empty,
            bits: 0.0,
        };
    }
    let mut pool = 0u32;
    if chars.iter().any(|c| c.is_ascii_lowercase()) {
        pool += 26;
    }
    if chars.iter().any(|c| c.is_ascii_uppercase()) {
        pool += 26;
    }
    if chars.iter().any(|c| c.is_ascii_digit()) {
        pool += 10;
    }
    if chars.iter().any(|c| c.is_ascii_punctuation() || *c == ' ') {
        pool += 33;
    }
    if chars.iter().any(|c| !c.is_ascii()) {
        pool += 64;
    }
    // Les caractères répétés ou en suite (« aaaa », « 1234 », « abcd ») comptent peu.
    let mut effective = 0.0f32;
    for (i, c) in chars.iter().enumerate() {
        let weak = i > 0
            && chars.get(i - 1).is_some_and(|prev| {
                let (a, b) = (u32::from(*prev), u32::from(*c));
                a == b || a + 1 == b || b + 1 == a
            });
        effective += if weak { 0.25 } else { 1.0 };
    }
    let bits = effective * (pool.max(1) as f32).log2();
    let level = if chars.len() < MIN_LENGTH || bits < BITS_WEAK {
        StrengthLevel::TooWeak
    } else if bits < BITS_GOOD {
        StrengthLevel::Weak
    } else if bits < BITS_STRONG {
        StrengthLevel::Good
    } else {
        StrengthLevel::Strong
    };
    Strength { level, bits }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_password() {
        assert_eq!(evaluate("").level, StrengthLevel::Empty);
    }

    #[test]
    fn short_passwords_are_too_weak() {
        assert_eq!(evaluate("Ab1!").level, StrengthLevel::TooWeak);
        assert_eq!(evaluate("motdepas").level, StrengthLevel::TooWeak);
    }

    #[test]
    fn repetitions_and_sequences_are_penalised() {
        assert_eq!(evaluate("aaaaaaaaaaaa").level, StrengthLevel::TooWeak);
        assert_eq!(evaluate("123456789012").level, StrengthLevel::TooWeak);
        assert!(evaluate("abcdefghijkl").bits < evaluate("qmzrtxwpvkly").bits);
    }

    #[test]
    fn levels_grow_with_length_and_variety() {
        assert_eq!(evaluate("qmzrtxwpvk").level, StrengthLevel::Weak);
        assert_eq!(evaluate("Qmzrtx-Wpvk7").level, StrengthLevel::Good);
        assert_eq!(
            evaluate("Cheval-Agrafe-Batterie-47").level,
            StrengthLevel::Strong
        );
    }

    #[test]
    fn acceptable_threshold() {
        assert!(!StrengthLevel::TooWeak.is_acceptable());
        assert!(StrengthLevel::Weak.is_acceptable());
        assert!(StrengthLevel::Strong.is_acceptable());
    }

    #[test]
    fn non_ascii_counts() {
        assert!(evaluate("éléphant-déçu").bits > evaluate("elephant-decu").bits);
    }
}
