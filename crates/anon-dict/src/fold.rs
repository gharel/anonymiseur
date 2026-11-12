//! Pliage des formes : minuscules, sans accents, apostrophes unifiées, tirets et
//! espaces ramenés à une espace unique. DOIT rester identique à `fold()` de
//! `tools/prepare_dicts.py` (le test `data_lists_are_already_folded` le vérifie sur
//! chaque entrée des listes).

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

/// Plie `text` pour la recherche dans les dictionnaires.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.nfd() {
        if canonical_combining_class(ch) != 0 {
            continue;
        }
        for low in ch.to_lowercase() {
            match low {
                '-' | '\u{2010}'..='\u{2015}' | '\u{2212}' => pending_space = true,
                c if c.is_whitespace() => pending_space = true,
                c => {
                    if pending_space && !out.is_empty() {
                        out.push(' ');
                    }
                    pending_space = false;
                    push_letter(&mut out, c);
                }
            }
        }
    }
    out
}

fn push_letter(out: &mut String, c: char) {
    match c {
        'œ' => out.push_str("oe"),
        'æ' => out.push_str("ae"),
        'ß' => out.push_str("ss"),
        'ø' => out.push('o'),
        'đ' => out.push('d'),
        'ł' => out.push('l'),
        '\u{2019}' | '\u{2018}' | '\u{02bc}' | '`' | '\u{00b4}' => out.push('\''),
        c => out.push(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_case_accents_and_punctuation() {
        assert_eq!(fold("Élodie"), "elodie");
        assert_eq!(fold("JEAN-PIERRE"), "jean pierre");
        assert_eq!(fold("Jean–Pierre"), "jean pierre");
        assert_eq!(fold("L’Haÿ-les-Roses"), "l'hay les roses");
        assert_eq!(fold("  Le   Mont-Dore "), "le mont dore");
        assert_eq!(fold("Bœuf Æon Straße"), "boeuf aeon strasse");
        assert_eq!(fold("Nouméa"), "noumea");
    }

    #[test]
    fn empty_and_separators_only() {
        assert_eq!(fold(""), "");
        assert_eq!(fold(" - - "), "");
    }

    #[test]
    fn exotic_input_does_not_panic() {
        for s in [
            "\u{0301}\u{0301}",
            "😀 Jean",
            "𝔘𝔫𝔦𝔠𝔬𝔡𝔢",
            "İstanbul",
            "ǅ",
            "\0",
        ] {
            let _ = fold(s);
        }
    }
}
