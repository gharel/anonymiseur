//! Découpage d'un bloc de texte en mots, avec leur casse et leur position.
//! Les mots à l'intérieur d'un email ou d'une URL ne produisent aucun jeton.

use crate::fold::fold;

/// Casse d'un mot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Case {
    /// Majuscule initiale suivie d'au moins une minuscule : `Dupont`, `Jean-Pierre`.
    Cap,
    /// Capitales uniquement, au moins deux lettres : `DUPONT`.
    Upper,
    /// Une seule lettre majuscule : `J`.
    Initial,
    /// Tout le reste : `dupont`, `日本`.
    Lower,
}

/// Nature d'un jeton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Lettres, avec traits d'union et apostrophes internes (`Jean-Pierre`, `O'Brien`).
    Word,
    /// Élision `d'` ou `l'`, apostrophe comprise, collée au mot suivant.
    Elision,
}

#[derive(Debug, Clone)]
pub(crate) struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
    pub case: Case,
    /// Forme pliée du jeton.
    pub folded: String,
}

impl Token {
    pub(crate) fn text<'a>(&self, text: &'a str) -> &'a str {
        text.get(self.start..self.end).unwrap_or("")
    }

    pub(crate) fn is_capitalized(&self) -> bool {
        matches!(self.case, Case::Cap | Case::Upper)
    }
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '\u{02bc}')
}

fn is_hyphen(c: char) -> bool {
    matches!(c, '-' | '\u{2011}')
}

/// Classe un mot selon sa casse.
fn classify(word: &str) -> Case {
    let mut letters = word.chars().filter(|c| c.is_alphabetic());
    let Some(first) = letters.next() else {
        return Case::Lower;
    };
    if !first.is_uppercase() {
        return Case::Lower;
    }
    let mut count = 1usize;
    let mut has_lower = false;
    for c in letters {
        count += 1;
        if c.is_lowercase() {
            has_lower = true;
        }
    }
    match (count, has_lower) {
        (1, _) => Case::Initial,
        (_, true) => Case::Cap,
        (_, false) => Case::Upper,
    }
}

/// Intervalles `(début, fin)` triés occupés par un email ou une URL.
pub(crate) fn protected_ranges(text: &str) -> Vec<(usize, usize)> {
    const LEADING: &[char] = &['(', '[', '<', '{', '"', '\'', '«', '“'];
    const TRAILING: &[char] = &[
        ')', ']', '>', '}', ',', ';', ':', '.', '!', '?', '"', '\'', '»', '”',
    ];
    let mut ranges = Vec::new();
    let mut chunk_start: Option<usize> = None;
    let mut push_chunk = |start: usize, end: usize| {
        let Some(chunk) = text.get(start..end) else {
            return;
        };
        let trimmed_start = chunk.trim_start_matches(LEADING);
        let trimmed = trimmed_start.trim_end_matches(TRAILING);
        let lead = chunk.len() - trimmed_start.len();
        let lower = trimmed.to_lowercase();
        let has_at = trimmed
            .find('@')
            .is_some_and(|i| i > 0 && i + 1 < trimmed.len());
        if has_at
            || trimmed.contains("://")
            || lower.starts_with("www.")
            || lower.starts_with("mailto:")
        {
            ranges.push((start + lead, start + lead + trimmed.len()));
        }
    };
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = chunk_start.take() {
                push_chunk(s, i);
            }
        } else if chunk_start.is_none() {
            chunk_start = Some(i);
        }
    }
    if let Some(s) = chunk_start {
        push_chunk(s, text.len());
    }
    ranges
}

/// Découpe `text` en jetons, hors emails et URL.
pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let protected = protected_ranges(text);
    let mut cursor = 0usize;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    while let Some(&(start, c)) = chars.get(i) {
        if !c.is_alphabetic() {
            i += 1;
            continue;
        }
        // Mot : lettres, avec traits d'union et apostrophes entre deux lettres.
        let mut j = i + 1;
        let mut elision_end: Option<usize> = None;
        while let Some(&(_, cj)) = chars.get(j) {
            if cj.is_alphabetic() {
                j += 1;
                continue;
            }
            let next_is_letter = chars.get(j + 1).is_some_and(|&(_, n)| n.is_alphabetic());
            if next_is_letter && is_hyphen(cj) {
                j += 1;
            } else if next_is_letter && is_apostrophe(cj) {
                if j == i + 1 && matches!(c, 'd' | 'D' | 'l' | 'L') {
                    elision_end = Some(j + 1);
                    break;
                }
                j += 1;
            } else {
                break;
            }
        }
        let end_of = |idx: usize| chars.get(idx).map_or(text.len(), |&(b, _)| b);
        let (end, next_i, kind) = match elision_end {
            Some(e) => (end_of(e), e, Kind::Elision),
            None => (end_of(j), j, Kind::Word),
        };
        i = next_i;
        while let Some(&(_, range_end)) = protected.get(cursor) {
            if range_end <= start {
                cursor += 1;
            } else {
                break;
            }
        }
        if protected
            .get(cursor)
            .is_some_and(|&(range_start, _)| range_start < end)
        {
            continue;
        }
        let word = text.get(start..end).unwrap_or("");
        let case = if kind == Kind::Elision {
            if c.is_uppercase() {
                Case::Initial
            } else {
                Case::Lower
            }
        } else {
            classify(word)
        };
        tokens.push(Token {
            start,
            end,
            kind,
            case,
            folded: fold(word),
        });
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        tokenize(text)
            .iter()
            .map(|t| t.text(text).to_string())
            .collect()
    }

    #[test]
    fn splits_words_hyphens_and_elisions() {
        assert_eq!(
            words("Jean-Pierre d'Artagnan, O'Brien"),
            ["Jean-Pierre", "d'", "Artagnan", "O'Brien"]
        );
        assert_eq!(words("L’Haÿ-les-Roses"), ["L’", "Haÿ-les-Roses"]);
        assert_eq!(words("aujourd'hui l'homme"), ["aujourd'hui", "l'", "homme"]);
    }

    #[test]
    fn classifies_case() {
        let toks = tokenize("Dupont DUPONT dupont J Jean-pierre 日本");
        let cases: Vec<Case> = toks.iter().map(|t| t.case).collect();
        assert_eq!(
            cases,
            [
                Case::Cap,
                Case::Upper,
                Case::Lower,
                Case::Initial,
                Case::Cap,
                Case::Lower
            ]
        );
    }

    #[test]
    fn emails_and_urls_yield_no_tokens() {
        assert_eq!(
            words("Écrire à Jean.Dupont@example.com ou voir https://example.com/Jean-Dupont Marie"),
            ["Écrire", "à", "ou", "voir", "Marie"]
        );
        assert_eq!(words("(www.example.com/Marie)"), Vec::<String>::new());
    }

    #[test]
    fn offsets_are_char_boundaries() {
        let text = "é Éric 😀 Zoë 日本語";
        for t in tokenize(text) {
            assert!(text.is_char_boundary(t.start) && text.is_char_boundary(t.end));
        }
    }
}
