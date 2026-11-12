//! Pliage de texte : minuscules, sans accents, apostrophes droites,
//! espaces (dont insécables) ramenées à une espace simple.

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// Version pliée d'un caractère (0, 1 ou plusieurs caractères).
pub(crate) fn fold_char(c: char) -> Vec<char> {
    if c.is_whitespace() {
        return vec![' '];
    }
    let c = if c == '’' { '\'' } else { c };
    std::iter::once(c)
        .nfd()
        .filter(|m| !is_combining_mark(*m))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Texte plié.
pub(crate) fn fold(s: &str) -> String {
    s.chars().flat_map(fold_char).collect()
}

/// Mots alphanumériques d'un texte plié.
pub(crate) fn tokens(s: &str) -> Vec<String> {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}
