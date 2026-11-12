//! Restauration : remet les vraies valeurs à la place des balises.

use crate::{tags, uri};
use anon_core::text::apply_replacements;
use anon_core::{BlockContext, BlockRole, Replacement, Session, Tag, TextTransform, Treatment};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Mutex, MutexGuard};

/// Bilan cumulé des restaurations depuis la création ou le dernier `reset`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RestoreReport {
    /// Nombre de balises remplacées par leur valeur.
    pub restored: usize,
    /// Balises de type valide mais d'indice inconnu, laissées telles quelles
    /// (forme normalisée `[TYPE_N]`, sans doublon, dans l'ordre d'apparition).
    pub unknown: Vec<String>,
    /// Balises de la session (traitement `Tag`, activées) jamais rencontrées.
    pub unused: Vec<Tag>,
}

#[derive(Default)]
struct State {
    restored: usize,
    unknown: Vec<String>,
    used: HashSet<Tag>,
}

/// Désanonymiseur. Reconnaît les balises sous forme tolérante (voir le module
/// `tags`). Indépendant de la session après construction.
pub struct Restorer {
    values: HashMap<Tag, String>,
    expected: Vec<Tag>,
    state: Mutex<State>,
}

impl fmt::Debug for Restorer {
    /// N'affiche aucune valeur.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Restorer")
            .field("values", &self.values.len())
            .finish_non_exhaustive()
    }
}

impl Restorer {
    pub fn new(session: &Session) -> Self {
        let mut values = HashMap::new();
        for e in &session.entities {
            values.entry(e.tag).or_insert_with(|| e.value.clone());
        }
        let expected = session
            .entities
            .iter()
            .filter(|e| e.enabled && e.treatment == Treatment::Tag)
            .map(|e| e.tag)
            .collect();
        Self {
            values,
            expected,
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Bilan cumulé de tous les appels.
    pub fn report(&self) -> RestoreReport {
        let st = self.state();
        RestoreReport {
            restored: st.restored,
            unknown: st.unknown.clone(),
            unused: self
                .expected
                .iter()
                .copied()
                .filter(|t| !st.used.contains(t))
                .collect(),
        }
    }

    /// Remet le bilan à zéro.
    pub fn reset(&self) {
        *self.state() = State::default();
    }

    fn compute(&self, text: &str, uri_mode: bool) -> Vec<Replacement> {
        let found = tags::find_tags(text, uri_mode);
        if found.is_empty() {
            return Vec::new();
        }
        let mut reps = Vec::with_capacity(found.len());
        let mut st = self.state();
        for f in found {
            match self.values.get(&f.tag) {
                Some(v) => {
                    let with = if uri_mode { uri::encode(v) } else { v.clone() };
                    reps.push(Replacement {
                        start: f.range.start,
                        end: f.range.end,
                        with,
                    });
                    st.restored = st.restored.saturating_add(1);
                    st.used.insert(f.tag);
                }
                None => {
                    let s = f.tag.to_string();
                    if !st.unknown.contains(&s) {
                        st.unknown.push(s);
                    }
                }
            }
        }
        reps
    }
}

impl TextTransform for Restorer {
    /// Un bloc de rôle `Hyperlink` est traité comme une cible d'URI. Chaque appel
    /// alimente le bilan.
    fn replacements(&self, text: &str, ctx: &BlockContext) -> Vec<Replacement> {
        self.compute(text, ctx.role == BlockRole::Hyperlink)
    }

    fn apply_uri<'a>(&self, uri: &'a str) -> Cow<'a, str> {
        let reps = self.compute(uri, true);
        apply_replacements(uri, &reps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replace::Replacer;
    use crate::session_ext::SessionExt;
    use anon_core::{EntityKind::*, Source};

    fn ctx() -> BlockContext {
        BlockContext::new("test", BlockRole::Body)
    }

    fn session() -> Session {
        let mut s = Session::new();
        s.intern(Personne, "Jean Dupont", Source::Manual);
        s.intern(Nom, "Dupont", Source::Manual);
        s.intern(Prenom, "Élodie", Source::Manual);
        s.intern(Societe, "Acme Conseil", Source::Manual);
        s.intern(Societe, "Globex", Source::Manual);
        s.intern(DateNaiss, "1er mars 1980", Source::Manual);
        s.intern(DateNaiss, "2 mai 1990", Source::Manual);
        s.intern(DateNaiss, "3 juin 2001", Source::Manual);
        s.intern(Email, "jean@example.com", Source::Manual);
        s.intern(Url, "https://example.com/profil", Source::Manual);
        s
    }

    fn restore(text: &str) -> String {
        Restorer::new(&session()).apply(text, &ctx()).into_owned()
    }

    #[test]
    fn tolerant_forms() {
        for form in [
            "[PERSONNE_1]",
            "[personne_1]",
            "[Personne 1]",
            "[PERSONNE-1]",
            "[PERSONNE\\_1]",
            "\\[PERSONNE_1\\]",
            "【PERSONNE_1】",
            "［PERSONNE_1］",
        ] {
            assert_eq!(
                restore(&format!("M. {form} écrit.")),
                "M. Jean Dupont écrit.",
                "{form}"
            );
        }
        assert_eq!(restore("[PRÉNOM_2]"), "Élodie");
        assert_eq!(restore("[SOCIÉTÉ_2]"), "Globex");
        assert_eq!(restore("[DATE NAISS 3]"), "3 juin 2001");
        assert_eq!(restore("[date\\_naiss\\_2]"), "2 mai 1990");
        assert_eq!(
            restore("**[SOCIETE_1]** et _[NOM_1]_"),
            "**Acme Conseil** et _Dupont_"
        );
    }

    #[test]
    fn uri_forms_in_text_and_uri() {
        assert_eq!(
            restore("Écrire à EMAIL_1@anon.invalid."),
            "Écrire à jean@example.com."
        );
        assert_eq!(
            restore("Voir https://anon.invalid/URL_1."),
            "Voir https://example.com/profil."
        );
        // Forme nue : seulement dans une URI.
        assert_eq!(restore("PERSONNE_1"), "PERSONNE_1");
        let r = Restorer::new(&session());
        assert_eq!(
            r.apply_uri("mailto:EMAIL_1@anon.invalid"),
            "mailto:jean@example.com"
        );
        assert_eq!(
            r.apply_uri("https://example.com/?q=PERSONNE_1"),
            "https://example.com/?q=Jean%20Dupont"
        );
        assert_eq!(
            r.apply_uri("https://anon.invalid/URL_1"),
            "https://example.com/profil"
        );
    }

    #[test]
    fn unknown_index_is_left_and_reported() {
        let r = Restorer::new(&session());
        let text = "[PERSONNE_9] et [personne 9] et [NOTE_1] et [PERSONNE_1]";
        assert_eq!(
            r.apply(text, &ctx()),
            "[PERSONNE_9] et [personne 9] et [NOTE_1] et Jean Dupont"
        );
        let rep = r.report();
        assert_eq!(rep.restored, 1);
        assert_eq!(rep.unknown, vec!["[PERSONNE_9]".to_string()]);
    }

    #[test]
    fn report_is_cumulative_and_resettable() {
        let s = session();
        let r = Restorer::new(&s);
        r.apply("[NOM_1]", &ctx());
        r.apply("[NOM_1] [SOCIETE_2]", &ctx());
        let rep = r.report();
        assert_eq!(rep.restored, 3);
        assert!(rep.unknown.is_empty());
        assert!(!rep.unused.contains(&Tag::new(Nom, 1)));
        assert!(rep.unused.contains(&Tag::new(Personne, 1)));
        assert_eq!(rep.unused.len(), s.entities.len() - 2);
        r.reset();
        assert_eq!(r.report().restored, 0);
        assert_eq!(r.report().unused.len(), s.entities.len());
    }

    #[test]
    fn generalized_values_are_not_restored() {
        let mut s = Session::new();
        let tag = s.intern(Age, "42 ans", Source::Manual).unwrap();
        s.set_treatment(tag, Treatment::Generalize("40-49 ans".into()));
        let anonymized = Replacer::new(&s).apply("Il a 42 ans.", &ctx()).into_owned();
        assert_eq!(anonymized, "Il a 40-49 ans.");
        let r = Restorer::new(&s);
        assert_eq!(r.apply(&anonymized, &ctx()), "Il a 40-49 ans.");
        assert!(r.report().unused.is_empty());
    }

    #[test]
    fn variant_is_restored_in_canonical_form() {
        // Comportement assumé : une occurrence écrite sous une autre forme
        // revient sous la forme canonique de l'entité.
        let s = session();
        let anonymized = Replacer::new(&s).apply("M. DUPONT", &ctx()).into_owned();
        assert_eq!(anonymized, "M. [NOM_1]");
        assert_eq!(restore(&anonymized), "M. Dupont");
    }

    #[test]
    fn no_tag_borrows() {
        let r = Restorer::new(&session());
        assert!(matches!(r.apply("rien ici", &ctx()), Cow::Borrowed(_)));
    }

    #[test]
    fn hyperlink_block_uses_uri_forms() {
        let r = Restorer::new(&session());
        let link = BlockContext::new("lien", BlockRole::Hyperlink);
        assert_eq!(
            r.apply("mailto:EMAIL_1@anon.invalid", &link),
            "mailto:jean@example.com"
        );
    }
}
