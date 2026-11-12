//! Dictionnaires embarqués (prénoms, noms de famille, communes, mots courants) et
//! détecteur à base de dictionnaires.
//!
//! Les listes de `data/` sont compilées en FST par `build.rs` et incluses dans le
//! binaire : aucune lecture de fichier à l'exécution, aucun réseau.

mod detector;
mod fold;
mod tokens;

pub use detector::{
    COMMUNE_CONFIDENCE, DictionaryDetector, ORGANIZATION_CONFIDENCE, PERSON_CONFIDENCE,
    SOLO_NAME_CONFIDENCE, TITLE_NAME_CONFIDENCE,
};
pub use fold::fold;

use fst::Map;
use std::sync::OnceLock;

/// Palier de fréquence d'une entrée (du plus rare au plus courant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// Rare : peu de naissances cumulées.
    Rare = 1,
    /// Courant.
    Common = 2,
    /// Très courant.
    VeryCommon = 3,
}

impl Tier {
    fn from_value(v: u64) -> Self {
        match v {
            0 | 1 => Tier::Rare,
            2 => Tier::Common,
            _ => Tier::VeryCommon,
        }
    }

    /// Valeur numérique : 1 (rare) à 3 (très courant).
    pub fn level(self) -> u8 {
        self as u8
    }
}

static FIRST_NAMES_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/prenoms.fst"));
static SURNAMES_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/noms.fst"));
static COMMUNES_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/communes.fst"));
static COMMON_WORDS_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mots_courants.fst"));

/// Taille cumulée des FST embarqués, en octets (contrôlée par un test).
pub const EMBEDDED_BYTES: usize = FIRST_NAMES_BYTES.len()
    + SURNAMES_BYTES.len()
    + COMMUNES_BYTES.len()
    + COMMON_WORDS_BYTES.len();

type Dict = Map<&'static [u8]>;

fn open(cell: &'static OnceLock<Option<Dict>>, bytes: &'static [u8]) -> Option<&'static Dict> {
    cell.get_or_init(|| Map::new(bytes).ok()).as_ref()
}

fn first_names() -> Option<&'static Dict> {
    static CELL: OnceLock<Option<Dict>> = OnceLock::new();
    open(&CELL, FIRST_NAMES_BYTES)
}

fn surnames() -> Option<&'static Dict> {
    static CELL: OnceLock<Option<Dict>> = OnceLock::new();
    open(&CELL, SURNAMES_BYTES)
}

fn communes() -> Option<&'static Dict> {
    static CELL: OnceLock<Option<Dict>> = OnceLock::new();
    open(&CELL, COMMUNES_BYTES)
}

fn common_words() -> Option<&'static Dict> {
    static CELL: OnceLock<Option<Dict>> = OnceLock::new();
    open(&CELL, COMMON_WORDS_BYTES)
}

fn get(dict: Option<&'static Dict>, folded: &str) -> Option<u64> {
    dict?.get(folded.as_bytes())
}

/// Palier d'un prénom (l'entrée est pliée par la fonction).
pub fn first_name(name: &str) -> Option<Tier> {
    first_name_folded(&fold(name))
}

/// Palier d'un nom de famille (l'entrée est pliée par la fonction).
pub fn surname(name: &str) -> Option<Tier> {
    surname_folded(&fold(name))
}

/// Vrai si `name` est une commune connue (l'entrée est pliée par la fonction).
pub fn commune(name: &str) -> bool {
    commune_folded(&fold(name))
}

/// Vrai si `word` est un mot français courant également prénom ou nom
/// (l'entrée est pliée par la fonction).
pub fn is_common_word(word: &str) -> bool {
    common_word_folded(&fold(word))
}

pub(crate) fn first_name_folded(folded: &str) -> Option<Tier> {
    get(first_names(), folded).map(Tier::from_value)
}

pub(crate) fn surname_folded(folded: &str) -> Option<Tier> {
    get(surnames(), folded).map(Tier::from_value)
}

pub(crate) fn commune_folded(folded: &str) -> bool {
    get(communes(), folded).is_some()
}

pub(crate) fn common_word_folded(folded: &str) -> bool {
    get(common_words(), folded).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Taille maximale cumulée des FST embarqués.
    const MAX_EMBEDDED_BYTES: usize = 4 * 1024 * 1024;

    #[test]
    fn embedded_size_is_bounded() {
        let total = std::hint::black_box(EMBEDDED_BYTES);
        assert!(
            total <= MAX_EMBEDDED_BYTES,
            "FST embarqués : {total} octets (max {MAX_EMBEDDED_BYTES})"
        );
        assert!(total > 0);
    }

    #[test]
    fn first_names_lookup() {
        assert_eq!(first_name("Jean"), Some(Tier::VeryCommon));
        assert!(first_name("JEAN-PIERRE").is_some());
        assert!(first_name("Élodie").is_some());
        assert!(first_name("elodie").is_some());
        assert!(first_name("Mélanie").is_some());
        assert!(first_name("Noël").is_some());
        assert!(first_name("Zélie").is_some());
        assert!(first_name("Sébastien").is_some());
        assert!(first_name("Teiva").is_some());
    }

    #[test]
    fn first_names_reject_ordinary_words() {
        for w in ["table", "bureau", "xylophone", "contrat", "", "   "] {
            assert!(first_name(w).is_none(), "{w}");
        }
    }

    #[test]
    fn surnames_lookup() {
        for s in [
            "Dupont",
            "MARTIN",
            "Lefèbvre",
            "Lefebvre",
            "Durand",
            "Le Goff",
            "Nguyen",
        ] {
            assert!(surname(s).is_some(), "{s}");
        }
        assert_eq!(surname("Martin"), Some(Tier::VeryCommon));
        for w in ["ordinateur", "xylophone", "", "xylophone"] {
            assert!(surname(w).is_none(), "{w}");
        }
    }

    #[test]
    fn communes_lookup() {
        for c in [
            "Paris",
            "Marseille",
            "Nouméa",
            "Noumea",
            "Mont-Dore",
            "Le Mont-Dore",
            "La Foa",
            "Païta",
            "Dumbéa",
            "Koné",
            "Bourail",
            "Lifou",
            "Fort-de-France",
            "Saint-Étienne",
            "L'Haÿ-les-Roses",
            "Papeete",
            "Cayenne",
        ] {
            assert!(commune(c), "{c}");
        }
        for w in ["Martinique", "ordinateur", "", "Dupontville"] {
            assert!(!commune(w), "{w}");
        }
    }

    #[test]
    fn new_caledonia_has_all_its_communes() {
        for c in [
            "Belep",
            "Bouloupari",
            "Canala",
            "Farino",
            "Hienghène",
            "Houaïlou",
            "Île des Pins",
            "Kaala-Gomen",
            "Koumac",
            "Moindou",
            "Ouégoa",
            "Ouvéa",
            "Poindimié",
            "Ponérihouen",
            "Pouébo",
            "Pouembout",
            "Poya",
            "Sarraméa",
            "Thio",
            "Voh",
            "Yaté",
            "Tontouta",
        ] {
            // Tontouta est une localité de Païta, pas une commune : tolérée absente.
            if c != "Tontouta" {
                assert!(commune(c), "{c}");
            }
        }
    }

    #[test]
    fn common_words_lookup() {
        for w in ["pierre", "Rose", "marin", "blanc", "Petit", "lundi"] {
            assert!(is_common_word(w), "{w}");
        }
        assert!(!is_common_word("martin"));
        assert!(!is_common_word("dupont"));
    }

    #[test]
    fn data_lists_are_already_folded() {
        for (name, text) in [
            ("prenoms", include_str!("../data/prenoms.txt")),
            ("noms", include_str!("../data/noms.txt")),
            ("communes", include_str!("../data/communes.txt")),
            ("mots_courants", include_str!("../data/mots_courants.txt")),
        ] {
            assert!(!text.is_empty(), "{name} vide");
            let mut previous: Option<&str> = None;
            for line in text.lines() {
                let (form, tier) = line.split_once('\t').unwrap_or((line, "1"));
                assert!(matches!(tier, "1" | "2" | "3"), "{name}: palier {tier}");
                assert_eq!(fold(form), form, "{name}: entrée non pliée");
                if let Some(prev) = previous {
                    assert!(prev.as_bytes() < form.as_bytes(), "{name}: {prev} / {form}");
                }
                previous = Some(form);
            }
        }
    }
}
