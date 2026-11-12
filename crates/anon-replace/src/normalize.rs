//! Pliage du texte pour la recherche des valeurs sensibles.
//!
//! Le texte plié sert uniquement à chercher : chaque octet du texte plié garde
//! l'offset d'octet du caractère d'origine dont il provient, ce qui permet de
//! ramener une correspondance à un intervalle du texte d'origine couvrant des
//! caractères entiers.

use std::fmt;
use std::ops::Range;
use unicode_normalization::char::{decompose_canonical, is_combining_mark};

/// Texte plié et, pour chaque octet de `text`, l'offset d'octet du caractère
/// d'origine. `offsets` a un élément de plus que `text` : la longueur du texte
/// d'origine (sentinelle de fin).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Folded {
    pub text: String,
    pub offsets: Vec<u32>,
}

impl fmt::Debug for Folded {
    /// N'affiche jamais le texte (contenu utilisateur).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Folded")
            .field("len", &self.text.len())
            .finish_non_exhaustive()
    }
}

impl Folded {
    /// Ramène l'intervalle `start..end` du texte plié à un intervalle de
    /// `original` couvrant des caractères entiers, marques combinantes qui
    /// suivent comprises. `None` si l'intervalle est vide ou invalide.
    pub fn origin_range(&self, original: &str, start: usize, end: usize) -> Option<Range<usize>> {
        if start >= end || end > self.text.len() {
            return None;
        }
        let from = usize::try_from(*self.offsets.get(start)?).ok()?;
        let last = usize::try_from(*self.offsets.get(end.checked_sub(1)?)?).ok()?;
        let last_char = original.get(last..)?.chars().next()?;
        let mut to = last.checked_add(last_char.len_utf8())?;
        for c in original.get(to..)?.chars() {
            if !is_combining_mark(c) {
                break;
            }
            to = to.checked_add(c.len_utf8())?;
        }
        (from <= to && original.is_char_boundary(from) && original.is_char_boundary(to))
            .then_some(from..to)
    }
}

/// Plie le texte : minuscules, accents retirés, ligatures développées,
/// apostrophes et tirets unifiés, espaces fusionnées, caractères invisibles retirés.
pub fn fold(text: &str) -> Folded {
    fold_impl(text, true)
}

/// Comme [`fold`] mais sans passage en minuscules, pour les entités sensibles à la casse.
pub fn fold_keep_case(text: &str) -> Folded {
    fold_impl(text, false)
}

/// Clé de comparaison d'une valeur : pliage puis suppression des espaces aux extrémités.
pub fn key(value: &str) -> String {
    fold(value).text.trim_matches(' ').to_string()
}

/// Clé sans séparateurs (espaces, points, tirets), pour les types à séparateurs variables.
pub fn compact_key(value: &str) -> String {
    key(value).chars().filter(|c| !is_separator(*c)).collect()
}

/// Séparateur ignoré par les types à séparateurs variables, dans le texte plié.
pub(crate) fn is_separator(c: char) -> bool {
    matches!(c, ' ' | '.' | '-')
}

/// Retire les séparateurs d'un texte plié en conservant la correspondance des offsets.
pub(crate) fn compact(folded: &Folded) -> Folded {
    let mut out = Folded {
        text: String::with_capacity(folded.text.len()),
        offsets: Vec::with_capacity(folded.offsets.len()),
    };
    for (i, c) in folded.text.char_indices() {
        if is_separator(c) {
            continue;
        }
        let origin = folded.offsets.get(i).copied().unwrap_or(u32::MAX);
        out.text.push(c);
        out.offsets
            .extend(std::iter::repeat_n(origin, c.len_utf8()));
    }
    out.offsets
        .push(folded.offsets.last().copied().unwrap_or(u32::MAX));
    out
}

/// Caractères supprimés au pliage : trait d'union conditionnel, espaces de
/// largeur nulle, indicateur d'ordre des octets.
pub(crate) fn is_ignorable(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{200B}'..='\u{200D}' | '\u{FEFF}')
}

pub(crate) fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '\u{02BC}' | '\u{2018}' | '`')
}

pub(crate) fn is_dash(c: char) -> bool {
    matches!(c, '-' | '\u{2010}'..='\u{2014}')
}

/// Lettre, chiffre ou marque combinante, hors apostrophes et tirets : ce qui
/// interdit une frontière de mot.
pub(crate) fn is_word_char(c: char) -> bool {
    (c.is_alphanumeric() || is_combining_mark(c)) && !is_apostrophe(c) && !is_dash(c)
}

fn to_u32(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

fn fold_impl(text: &str, lower: bool) -> Folded {
    let mut out = Folded {
        text: String::with_capacity(text.len()),
        offsets: Vec::with_capacity(text.len().saturating_add(1)),
    };
    let mut in_space = false;
    for (i, c) in text.char_indices() {
        let origin = to_u32(i);
        let push = |ch: char, out: &mut Folded| {
            out.text.push(ch);
            out.offsets
                .extend(std::iter::repeat_n(origin, ch.len_utf8()));
        };
        if is_ignorable(c) {
            continue;
        }
        if c.is_whitespace() {
            if !in_space {
                push(' ', &mut out);
                in_space = true;
            }
            continue;
        }
        let before = out.text.len();
        if is_apostrophe(c) {
            push('\'', &mut out);
        } else if is_dash(c) {
            push('-', &mut out);
        } else if lower {
            for l in c.to_lowercase() {
                push_letter(l, &mut |ch| push(ch, &mut out));
            }
        } else {
            push_letter(c, &mut |ch| push(ch, &mut out));
        }
        if out.text.len() != before {
            in_space = false;
        }
    }
    out.offsets.push(to_u32(text.len()));
    out
}

fn push_letter(c: char, push: &mut impl FnMut(char)) {
    let expansion: Option<[char; 2]> = match c {
        'œ' => Some(['o', 'e']),
        'Œ' => Some(['O', 'E']),
        'æ' => Some(['a', 'e']),
        'Æ' => Some(['A', 'E']),
        'ß' => Some(['s', 's']),
        'ẞ' => Some(['S', 'S']),
        _ => None,
    };
    match expansion {
        Some(chars) => chars.into_iter().for_each(&mut *push),
        None => decompose_canonical(c, |d| {
            if !is_combining_mark(d) {
                push(d);
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercase_and_accents() {
        assert_eq!(fold("Élodie ÇA").text, "elodie ca");
        assert_eq!(fold_keep_case("Élodie ÇA").text, "Elodie CA");
    }

    #[test]
    fn ligatures() {
        assert_eq!(fold("Œuvre Æther Straße").text, "oeuvre aether strasse");
        assert_eq!(fold_keep_case("Œ ß").text, "OE ss");
    }

    #[test]
    fn apostrophes_and_dashes() {
        assert_eq!(fold("d’Alice dʼun l‘x `y").text, "d'alice d'un l'x 'y");
        assert_eq!(fold("a‐b‑c‒d–e—f").text, "a-b-c-d-e-f");
    }

    #[test]
    fn whitespace_runs_collapse() {
        assert_eq!(fold("a \u{00A0}\u{202F}\u{2009}\t\r\nb").text, "a b");
        assert_eq!(fold("a \u{200B} b").text, "a b");
    }

    #[test]
    fn invisible_chars_removed() {
        assert_eq!(
            fold("Mar\u{00AD}tin\u{200B}\u{200C}\u{200D}\u{FEFF}").text,
            "martin"
        );
    }

    #[test]
    fn nfd_input_folds_like_nfc() {
        assert_eq!(fold("E\u{0301}lodie").text, fold("Élodie").text);
    }

    #[test]
    fn offsets_have_sentinel_and_point_to_char_starts() {
        let src = "É œ";
        let f = fold(src);
        assert_eq!(f.text, "e oe");
        assert_eq!(f.offsets.len(), f.text.len() + 1);
        assert_eq!(f.offsets, vec![0, 2, 3, 3, 5]);
    }

    #[test]
    fn origin_range_covers_whole_chars() {
        let src = "xE\u{0301}y";
        let f = fold(src);
        assert_eq!(f.text, "xey");
        let r = f.origin_range(src, 1, 2);
        assert_eq!(r, Some(1..4));
        let src = "Straße";
        let f = fold(src);
        // Correspondance qui commence au milieu de « ss » : on couvre tout le « ß ».
        assert_eq!(f.origin_range(src, 5, 7), Some(4..7));
        assert_eq!(f.origin_range(src, 3, 3), None);
        assert_eq!(f.origin_range(src, 3, 99), None);
    }

    #[test]
    fn key_trims() {
        assert_eq!(key("  Jean\u{00A0}Dupont \n"), "jean dupont");
        assert_eq!(key(" \u{200B} "), "");
        assert_eq!(compact_key("06 12.34-56\u{00A0}78"), "0612345678");
    }

    #[test]
    fn compact_keeps_offsets() {
        let src = "06 12";
        let c = compact(&fold(src));
        assert_eq!(c.text, "0612");
        assert_eq!(c.offsets, vec![0, 1, 3, 4, 5]);
    }

    #[test]
    fn emoji_and_non_latin_do_not_panic() {
        let src = "🙂 東京 Москва";
        let f = fold(src);
        assert_eq!(f.text, "🙂 東京 москва");
        for (i, _) in f.text.char_indices() {
            let o = f.offsets[i] as usize;
            assert!(src.is_char_boundary(o));
        }
    }

    #[test]
    fn word_chars() {
        assert!(is_word_char('é'));
        assert!(is_word_char('7'));
        assert!(!is_word_char('\u{02BC}'));
        assert!(!is_word_char('-'));
        assert!(!is_word_char(' '));
    }
}
