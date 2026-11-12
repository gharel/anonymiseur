//! Reconnaissance tolérante des balises dans un texte (formes entre crochets,
//! formes URI, forme nue). Partagé par `Replacer` (protection des balises déjà
//! présentes) et `Restorer`.

use crate::normalize;
use anon_core::{EntityKind, Tag};
use regex::{Captures, Regex};
use std::ops::Range;
use std::sync::LazyLock;

/// `[PERSONNE_1]` et ses variantes : casse quelconque, espace ou tiret au lieu
/// du souligné, souligné ou crochets échappés en Markdown, accents, préfixe en
/// plusieurs mots, crochets pleine chasse.
const BRACKET: &str = r"(?:\\?\[|【|［)[ \t\x{A0}\x{202F}]*(?P<bp>[\p{L}\p{M}]+(?:(?:\\?_|[ \-])+[\p{L}\p{M}]+)*)(?:\\?_|[ \-])+(?P<bn>[0-9]{1,9})[ \t\x{A0}\x{202F}]*(?:\\?\]|】|］)";
/// `https://anon.invalid/URL_1`.
const URL: &str = r"(?i:https?://anon\.invalid/(?P<up>[a-z]+(?:_[a-z]+)*)_(?P<un>[0-9]{1,9}))";
/// `EMAIL_1@anon.invalid`.
const EMAIL: &str = r"(?i:\b(?P<ep>[a-z]+(?:_[a-z]+)*)_(?P<en>[0-9]{1,9})@anon\.invalid)";
/// `PERSONNE_1`, seulement dans une cible d'URI.
const BARE: &str = r"\b(?P<np>[A-Za-z]+(?:_[A-Za-z]+)*)_(?P<nn>[0-9]{1,9})\b";

static TEXT_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(&format!("{BRACKET}|{URL}|{EMAIL}")).ok());
static URI_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(&format!("{BRACKET}|{URL}|{EMAIL}|{BARE}")).ok());

/// Balise trouvée : intervalle d'octets dans le texte et balise de type valide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoundTag {
    pub range: Range<usize>,
    pub tag: Tag,
}

/// Balises de type valide (indice connu ou non), triées, sans chevauchement.
/// `uri` ajoute la forme nue `PERSONNE_1`.
pub(crate) fn find_tags(text: &str, uri: bool) -> Vec<FoundTag> {
    let re = if uri { &*URI_RE } else { &*TEXT_RE };
    let Some(re) = re else {
        return Vec::new();
    };
    re.captures_iter(text)
        .filter_map(|caps| {
            let whole = caps.get(0)?;
            let tag = parse(&caps)?;
            Some(FoundTag {
                range: whole.range(),
                tag,
            })
        })
        .collect()
}

fn parse(caps: &Captures<'_>) -> Option<Tag> {
    let (p, n) = [("bp", "bn"), ("up", "un"), ("ep", "en"), ("np", "nn")]
        .into_iter()
        .find_map(|(p, n)| Some((caps.name(p)?, caps.name(n)?)))?;
    let kind = parse_prefix(p.as_str())?;
    let index = n.as_str().parse::<u32>().ok()?;
    Some(Tag::new(kind, index))
}

/// `Prénom`, `date naiss`, `DATE\_NAISS`, `societe` → type correspondant.
pub(crate) fn parse_prefix(prefix: &str) -> Option<EntityKind> {
    let cleaned: String = prefix.chars().filter(|c| *c != '\\').collect();
    let folded = normalize::fold(&cleaned).text;
    let mut out = String::with_capacity(folded.len());
    for c in folded.chars() {
        let c = if c == ' ' || c == '-' { '_' } else { c };
        if c == '_' && (out.is_empty() || out.ends_with('_')) {
            continue;
        }
        out.push(c.to_ascii_uppercase());
    }
    while out.ends_with('_') {
        out.pop();
    }
    EntityKind::from_prefix(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regexes_compile() {
        assert!(TEXT_RE.is_some());
        assert!(URI_RE.is_some());
    }

    #[test]
    fn prefixes() {
        assert_eq!(parse_prefix("Prénom"), Some(EntityKind::Prenom));
        assert_eq!(parse_prefix("PRE\u{301}NOM"), Some(EntityKind::Prenom));
        assert_eq!(parse_prefix("date naiss"), Some(EntityKind::DateNaiss));
        assert_eq!(parse_prefix("DATE\\_NAISS"), Some(EntityKind::DateNaiss));
        assert_eq!(parse_prefix("Date-Naiss"), Some(EntityKind::DateNaiss));
        assert_eq!(parse_prefix("NOTE"), None);
    }

    fn one(text: &str, uri: bool) -> Option<(Tag, &str)> {
        let found = find_tags(text, uri);
        let f = found.first()?;
        Some((f.tag, &text[f.range.clone()]))
    }

    #[test]
    fn tolerant_bracket_forms() {
        let p1 = Tag::new(EntityKind::Personne, 1);
        for form in [
            "[PERSONNE_1]",
            "[personne_1]",
            "[Personne 1]",
            "[PERSONNE-1]",
            "[PERSONNE\\_1]",
            "\\[PERSONNE_1\\]",
            "【PERSONNE_1】",
            "［PERSONNE_1］",
            "[ PERSONNE_1 ]",
        ] {
            assert_eq!(one(form, false), Some((p1, form)), "{form}");
        }
        assert_eq!(
            one("[PRÉNOM_1]", false).map(|t| t.0),
            Some(Tag::new(EntityKind::Prenom, 1))
        );
        assert_eq!(
            one("[SOCIÉTÉ_2]", false).map(|t| t.0),
            Some(Tag::new(EntityKind::Societe, 2))
        );
        assert_eq!(
            one("[DATE NAISS 3]", false).map(|t| t.0),
            Some(Tag::new(EntityKind::DateNaiss, 3))
        );
        assert_eq!(
            one("**[SOCIETE_1]**", false),
            Some((Tag::new(EntityKind::Societe, 1), "[SOCIETE_1]"))
        );
    }

    #[test]
    fn uri_forms() {
        assert_eq!(
            one("écrire à EMAIL_1@anon.invalid.", false),
            Some((Tag::new(EntityKind::Email, 1), "EMAIL_1@anon.invalid"))
        );
        assert_eq!(
            one("voir https://anon.invalid/URL_2 ici", false),
            Some((Tag::new(EntityKind::Url, 2), "https://anon.invalid/URL_2"))
        );
        assert_eq!(one("PERSONNE_1", false), None);
        assert_eq!(
            one("https://example.com/?q=PERSONNE_1", true),
            Some((Tag::new(EntityKind::Personne, 1), "PERSONNE_1"))
        );
    }

    #[test]
    fn not_tags() {
        assert!(find_tags("[1] [NOTE_1] [PERSONNE] [PERSONNE_]", false).is_empty());
    }
}
