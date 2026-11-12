//! Remplacement des valeurs validées par leurs balises, en une passe.

use crate::normalize::{
    self, Folded, compact_key, fold, fold_keep_case, is_ignorable, is_separator, is_word_char, key,
};
use crate::session_ext::has_variable_separators;
use crate::{tags, uri};
use aho_corasick::{AhoCorasick, MatchKind};
use anon_core::text::apply_replacements;
use anon_core::{BlockContext, BlockRole, Replacement, Session, Tag, TextTransform, Treatment};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Range;
use unicode_normalization::char::is_combining_mark;

/// Occurrence d'une valeur validée dans un texte. Offsets en octets, sur des
/// frontières de caractères.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Match {
    pub tag: Tag,
    pub start: usize,
    pub end: usize,
}

struct Entry {
    tag: Tag,
    /// Texte de remplacement dans un texte ordinaire.
    with: String,
    /// Texte de remplacement dans une cible d'URI.
    uri_with: String,
    case_sensitive: bool,
    /// Formes de surface pliées sans changement de casse (sans séparateurs si `compact`).
    forms: Vec<String>,
}

/// Automate et, pour chaque motif, les entrées qui le portent (ordre de la session).
#[derive(Default)]
struct Matcher {
    ac: Option<AhoCorasick>,
    owners: Vec<Vec<usize>>,
}

impl Matcher {
    fn build(patterns: Vec<(String, usize)>) -> Self {
        let mut ids: HashMap<String, usize> = HashMap::new();
        let mut pats: Vec<String> = Vec::new();
        let mut owners: Vec<Vec<usize>> = Vec::new();
        for (p, entry) in patterns {
            match ids.get(&p) {
                Some(&id) => {
                    if let Some(o) = owners.get_mut(id)
                        && !o.contains(&entry)
                    {
                        o.push(entry);
                    }
                }
                None => {
                    ids.insert(p.clone(), pats.len());
                    pats.push(p);
                    owners.push(vec![entry]);
                }
            }
        }
        let ac = if pats.is_empty() {
            None
        } else {
            AhoCorasick::builder()
                .match_kind(MatchKind::Standard)
                .build(&pats)
                .ok()
        };
        Self { ac, owners }
    }
}

/// Anonymiseur : remplace toutes les occurrences des valeurs validées et
/// activées de la session. Indépendant de la session après construction.
pub struct Replacer {
    entries: Vec<Entry>,
    plain: Matcher,
    compact: Matcher,
    never_plain: HashSet<String>,
    never_compact: HashSet<String>,
}

impl fmt::Debug for Replacer {
    /// N'affiche aucune valeur.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Replacer")
            .field("entries", &self.entries.len())
            .field("never", &self.never_plain.len())
            .finish_non_exhaustive()
    }
}

fn case_form(s: &str, compact: bool) -> String {
    let folded = fold_keep_case(s).text;
    let trimmed = folded.trim_matches(' ');
    if compact {
        trimmed.chars().filter(|c| !is_separator(*c)).collect()
    } else {
        trimmed.to_string()
    }
}

fn prev_char(text: &str, at: usize) -> Option<char> {
    text.get(..at)?.chars().rev().find(|c| !is_ignorable(*c))
}

fn next_char(text: &str, at: usize) -> Option<char> {
    text.get(at..)?.chars().find(|c| !is_ignorable(*c))
}

/// Frontières de mots : une valeur qui commence (finit) par une lettre ou un
/// chiffre ne peut pas toucher une lettre ou un chiffre de ce côté.
fn boundaries_ok(text: &str, r: &Range<usize>, slice: &str) -> bool {
    let first = slice.chars().next();
    let last = slice.chars().rev().find(|c| !is_combining_mark(*c));
    let left_ok =
        !(first.is_some_and(is_word_char) && prev_char(text, r.start).is_some_and(is_word_char));
    let right_ok =
        !(last.is_some_and(is_word_char) && next_char(text, r.end).is_some_and(is_word_char));
    left_ok && right_ok
}

/// Candidat : intervalle d'origine et indice d'entrée.
type Candidate = (usize, usize, usize);

impl Replacer {
    pub fn new(session: &Session) -> Self {
        let mut entries = Vec::new();
        let mut plain = Vec::new();
        let mut compact = Vec::new();
        for e in session.entities.iter().filter(|e| e.enabled) {
            let is_compact = has_variable_separators(e.tag.kind);
            let idx = entries.len();
            let mut forms = Vec::new();
            for s in std::iter::once(&e.value).chain(e.variants.iter()) {
                let k = if is_compact { compact_key(s) } else { key(s) };
                if k.is_empty() {
                    continue;
                }
                if is_compact {
                    compact.push((k, idx));
                } else {
                    plain.push((k, idx));
                }
                forms.push(case_form(s, is_compact));
            }
            if forms.is_empty() {
                continue;
            }
            let (with, uri_with) = match &e.treatment {
                Treatment::Tag => (e.tag.to_string(), e.tag.uri_token()),
                Treatment::Generalize(g) => (g.clone(), uri::encode(g)),
            };
            entries.push(Entry {
                tag: e.tag,
                with,
                uri_with,
                case_sensitive: e.case_sensitive,
                forms,
            });
        }
        let never_plain = session
            .never
            .iter()
            .map(|n| key(n))
            .filter(|k| !k.is_empty())
            .collect();
        let never_compact = session
            .never
            .iter()
            .map(|n| compact_key(n))
            .filter(|k| !k.is_empty())
            .collect();
        Self {
            entries,
            plain: Matcher::build(plain),
            compact: Matcher::build(compact),
            never_plain,
            never_compact,
        }
    }

    /// Occurrences retenues (triées, sans chevauchement), pour le contrôle de
    /// fuite et le comptage.
    pub fn matches(&self, text: &str) -> Vec<Match> {
        self.find(text, false)
            .into_iter()
            .filter_map(|(start, end, i)| {
                let e = self.entries.get(i)?;
                Some(Match {
                    tag: e.tag,
                    start,
                    end,
                })
            })
            .collect()
    }

    fn collect(
        &self,
        matcher: &Matcher,
        folded: &Folded,
        text: &str,
        protected: &[Range<usize>],
        compact: bool,
        out: &mut Vec<Candidate>,
    ) {
        let Some(ac) = &matcher.ac else {
            return;
        };
        let never = if compact {
            &self.never_compact
        } else {
            &self.never_plain
        };
        for m in ac.find_overlapping_iter(folded.text.as_str()) {
            let Some(r) = folded.origin_range(text, m.start(), m.end()) else {
                continue;
            };
            let Some(slice) = text.get(r.clone()) else {
                continue;
            };
            if !boundaries_ok(text, &r, slice)
                || protected.iter().any(|p| p.start < r.end && r.start < p.end)
            {
                continue;
            }
            let k = if compact {
                compact_key(slice)
            } else {
                key(slice)
            };
            if never.contains(&k) {
                continue;
            }
            let Some(owners) = matcher.owners.get(m.pattern().as_usize()) else {
                continue;
            };
            let mut surface: Option<String> = None;
            for &i in owners {
                let Some(e) = self.entries.get(i) else {
                    continue;
                };
                if e.case_sensitive {
                    let s = surface.get_or_insert_with(|| case_form(slice, compact));
                    if !e.forms.contains(s) {
                        continue;
                    }
                }
                out.push((r.start, r.end, i));
                break;
            }
        }
    }

    /// Sélection finale : le plus à gauche, puis le plus long, puis l'entité la
    /// plus ancienne ; sans chevauchement.
    fn find(&self, text: &str, uri_mode: bool) -> Vec<Candidate> {
        if self.entries.is_empty() || text.is_empty() {
            return Vec::new();
        }
        let protected: Vec<Range<usize>> = tags::find_tags(text, uri_mode)
            .into_iter()
            .map(|t| t.range)
            .collect();
        let folded = fold(text);
        let mut cands = Vec::new();
        self.collect(&self.plain, &folded, text, &protected, false, &mut cands);
        if self.compact.ac.is_some() {
            let c = normalize::compact(&folded);
            self.collect(&self.compact, &c, text, &protected, true, &mut cands);
        }
        cands.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        let mut out = Vec::with_capacity(cands.len());
        let mut cursor = 0usize;
        for c in cands {
            if c.0 >= cursor && c.0 < c.1 {
                cursor = c.1;
                out.push(c);
            }
        }
        out
    }

    fn to_replacements(&self, found: Vec<Candidate>, uri_mode: bool) -> Vec<Replacement> {
        found
            .into_iter()
            .filter_map(|(start, end, i)| {
                let e = self.entries.get(i)?;
                let with = if uri_mode { &e.uri_with } else { &e.with };
                Some(Replacement {
                    start,
                    end,
                    with: with.clone(),
                })
            })
            .collect()
    }

    /// Remplacements dans une cible d'URI, éventuellement codée en pourcent.
    fn uri_replacements(&self, uri: &str) -> Vec<Replacement> {
        if uri.contains('%')
            && let Some((decoded, map)) = uri::decode(uri)
        {
            let found = self.find(&decoded, true);
            let mapped = found
                .into_iter()
                .filter_map(|(s, e, i)| {
                    let s = *map.get(s)?;
                    let e = *map.get(e)?;
                    (s < e && uri.is_char_boundary(s) && uri.is_char_boundary(e))
                        .then_some((s, e, i))
                })
                .collect();
            return self.to_replacements(mapped, true);
        }
        self.to_replacements(self.find(uri, true), true)
    }
}

impl TextTransform for Replacer {
    /// Un bloc de rôle `Hyperlink` est traité comme une cible d'URI.
    fn replacements(&self, text: &str, ctx: &BlockContext) -> Vec<Replacement> {
        if ctx.role == BlockRole::Hyperlink {
            return self.uri_replacements(text);
        }
        self.to_replacements(self.find(text, false), false)
    }

    fn apply_uri<'a>(&self, uri: &'a str) -> Cow<'a, str> {
        let reps = self.uri_replacements(uri);
        apply_replacements(uri, &reps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_ext::SessionExt;
    use anon_core::{EntityKind::*, Source};

    fn ctx() -> BlockContext {
        BlockContext::new("test", BlockRole::Body)
    }

    fn anon(s: &Session, text: &str) -> String {
        Replacer::new(s).apply(text, &ctx()).into_owned()
    }

    fn session(items: &[(anon_core::EntityKind, &str)]) -> Session {
        let mut s = Session::new();
        for (k, v) in items {
            s.intern(*k, v, Source::Manual);
        }
        s
    }

    #[test]
    fn replaces_all_occurrences_case_insensitively() {
        let s = session(&[(Personne, "Jean Dupont")]);
        assert_eq!(
            anon(&s, "Jean Dupont et JEAN DUPONT, puis jean dupont."),
            "[PERSONNE_1] et [PERSONNE_1], puis [PERSONNE_1]."
        );
    }

    #[test]
    fn word_boundaries() {
        let s = session(&[(Nom, "Martin"), (Prenom, "Alice")]);
        assert_eq!(anon(&s, "La Martinique"), "La Martinique");
        assert_eq!(anon(&s, "Saint-Martin"), "Saint-[NOM_1]");
        assert_eq!(
            anon(&s, "d'Alice et d’Alice"),
            "d'[PRENOM_2] et d’[PRENOM_2]"
        );
        assert_eq!(anon(&s, "Alice2 Alicette 3Alice"), "Alice2 Alicette 3Alice");
        assert_eq!(anon(&s, "Mar\u{00AD}tinique"), "Mar\u{00AD}tinique");
    }

    #[test]
    fn surrounded_by_punctuation() {
        let s = session(&[(Prenom, "Alice")]);
        assert_eq!(
            anon(
                &s,
                "(Alice), «Alice», \"Alice\"; Alice! Alice? [Alice] …Alice…"
            ),
            "([PRENOM_1]), «[PRENOM_1]», \"[PRENOM_1]\"; [PRENOM_1]! [PRENOM_1]? [[PRENOM_1]] …[PRENOM_1]…"
        );
    }

    #[test]
    fn case_sensitive_entity_requires_exact_case() {
        let mut s = Session::new();
        let tag = s
            .intern_with_case(Prenom, "Pierre", Source::Dictionary, true)
            .unwrap();
        assert_eq!(
            anon(&s, "Pierre pose la pierre angulaire, PIERRE aussi."),
            "[PRENOM_1] pose la pierre angulaire, PIERRE aussi."
        );
        s.add_variant(tag, "PIERRE");
        assert_eq!(anon(&s, "PIERRE"), "[PRENOM_1]");
        // Accents tolérés, casse non.
        assert_eq!(anon(&s, "Piérre"), "[PRENOM_1]");
    }

    #[test]
    fn short_values_are_case_sensitive_by_default() {
        let s = session(&[(Societe, "SNCF"), (Societe, "BTP")]);
        assert_eq!(
            anon(&s, "BTP, btp, SNCF, sncf"),
            "[SOCIETE_2], btp, [SOCIETE_1], [SOCIETE_1]"
        );
    }

    #[test]
    fn non_breaking_spaces_and_nfd() {
        let s = session(&[(Personne, "Élodie Durand")]);
        assert_eq!(anon(&s, "Mme Élodie\u{00A0}Durand."), "Mme [PERSONNE_1].");
        assert_eq!(anon(&s, "Mme Élodie\u{202F}  Durand."), "Mme [PERSONNE_1].");
        assert_eq!(anon(&s, "Mme E\u{0301}lodie Durand."), "Mme [PERSONNE_1].");
        assert_eq!(anon(&s, "Mme elodie DURAND"), "Mme [PERSONNE_1]");
        let s = session(&[(Personne, "E\u{0301}lodie Durand")]);
        assert_eq!(anon(&s, "Mme Élodie Durand"), "Mme [PERSONNE_1]");
    }

    #[test]
    fn emoji_and_other_scripts_around() {
        let s = session(&[(Prenom, "Zoé"), (Lieu, "東京"), (Nom, "Иванов")]);
        let text = "🙂Zoé🙂 à 東京 avec Иванов👍🏽 et ИВАНОВ";
        let r = Replacer::new(&s);
        for rep in r.replacements(text, &ctx()) {
            assert!(text.is_char_boundary(rep.start) && text.is_char_boundary(rep.end));
        }
        assert_eq!(
            anon(&s, text),
            "🙂[PRENOM_1]🙂 à [LIEU_1] avec [NOM_2]👍🏽 et [NOM_2]"
        );
    }

    #[test]
    fn trailing_combining_mark_is_included() {
        let s = session(&[(Nom, "Durand")]);
        // Le « d » final porte un accent combinant : il est remplacé avec.
        assert_eq!(anon(&s, "Durand\u{0301} x"), "[NOM_1] x");
    }

    #[test]
    fn longest_leftmost_and_person_linking() {
        let s = session(&[(Personne, "Jean Dupont"), (Nom, "Dupont"), (Prenom, "Jean")]);
        assert_eq!(
            anon(&s, "Jean Dupont. Dupont. Jean."),
            "[PERSONNE_1]. [NOM_1]. [PRENOM_1]."
        );
    }

    #[test]
    fn shorter_match_used_when_longer_fails_boundary() {
        let s = session(&[(Societe, "Dupont S"), (Nom, "Dupont")]);
        assert_eq!(anon(&s, "Dupont SA"), "[NOM_1] SA");
    }

    #[test]
    fn variable_separators() {
        let s = session(&[
            (Tel, "06 12 34 56 78"),
            (Iban, "FR76 3000 6000 0112 3456 7890 189"),
        ]);
        for form in [
            "06 12 34 56 78",
            "0612345678",
            "06.12.34.56.78",
            "06-12-34-56-78",
            "06\u{00A0}12\u{00A0}34\u{00A0}56\u{00A0}78",
        ] {
            assert_eq!(
                anon(&s, &format!("Tél : {form}.")),
                "Tél : [TEL_1].",
                "{form}"
            );
        }
        assert_eq!(anon(&s, "06123456789"), "06123456789");
        assert_eq!(
            anon(&s, "IBAN fr7630006000011234567890189 !"),
            "IBAN [IBAN_1] !"
        );
    }

    #[test]
    fn existing_tags_are_never_replaced() {
        let s = session(&[(Societe, "Personne"), (Autre, "NOM_1")]);
        let text = "[PERSONNE_1] [personne 1] EMAIL_1@anon.invalid [NOM_1]";
        assert_eq!(anon(&s, text), text);
        assert_eq!(anon(&s, "Personne"), "[SOCIETE_1]");
    }

    #[test]
    fn disabled_and_never_are_ignored() {
        let mut s = session(&[(Prenom, "Alice"), (Prenom, "Bruno")]);
        s.set_enabled(Tag::new(Prenom, 1), false);
        assert_eq!(anon(&s, "Alice et Bruno"), "Alice et [PRENOM_2]");
        let tag = Tag::new(Prenom, 2);
        s.add_variant(tag, "Bruno le grand");
        s.add_never("Bruno le grand");
        // La variante est dans « jamais » : seule « Bruno » est remplacée.
        assert_eq!(anon(&s, "Bruno le grand"), "[PRENOM_2] le grand");
    }

    #[test]
    fn generalize_treatment() {
        let mut s = session(&[(Age, "42 ans")]);
        s.set_treatment(Tag::new(Age, 1), Treatment::Generalize("40-49 ans".into()));
        assert_eq!(anon(&s, "Il a 42 ans."), "Il a 40-49 ans.");
    }

    #[test]
    fn uri_targets() {
        let s = session(&[
            (Email, "jean@example.com"),
            (Personne, "Jean Dupont"),
            (Url, "https://example.com/profil"),
        ]);
        let r = Replacer::new(&s);
        assert_eq!(
            r.apply_uri("mailto:jean@example.com"),
            "mailto:EMAIL_1@anon.invalid"
        );
        assert_eq!(
            r.apply_uri("mailto:jean%40example.com?subject=x"),
            "mailto:EMAIL_1@anon.invalid?subject=x"
        );
        assert_eq!(
            r.apply_uri("https://example.com/?q=Jean%20Dupont&x=1"),
            "https://example.com/?q=PERSONNE_1&x=1"
        );
        assert_eq!(
            r.apply_uri("https://example.com/profil"),
            "https://anon.invalid/URL_1"
        );
        assert_eq!(r.apply_uri("https://example.org/"), "https://example.org/");
        assert_eq!(
            r.apply_uri("mailto:EMAIL_1@anon.invalid"),
            "mailto:EMAIL_1@anon.invalid"
        );
        let link = BlockContext::new("lien", BlockRole::Hyperlink);
        assert_eq!(
            r.apply("mailto:jean@example.com", &link),
            "mailto:EMAIL_1@anon.invalid"
        );
    }

    #[test]
    fn matches_report_tags_and_offsets() {
        let s = session(&[(Prenom, "Alice")]);
        let r = Replacer::new(&s);
        let text = "Alice, ALICE";
        let m = r.matches(text);
        assert_eq!(
            m,
            vec![
                Match {
                    tag: Tag::new(Prenom, 1),
                    start: 0,
                    end: 5
                },
                Match {
                    tag: Tag::new(Prenom, 1),
                    start: 7,
                    end: 12
                },
            ]
        );
    }

    #[test]
    fn empty_session_borrows() {
        let r = Replacer::new(&Session::new());
        assert!(matches!(r.apply("Alice", &ctx()), Cow::Borrowed(_)));
    }

    #[test]
    fn debug_hides_values() {
        let s = session(&[(Prenom, "Alice")]);
        assert!(!format!("{:?}", Replacer::new(&s)).contains("Alice"));
    }
}
