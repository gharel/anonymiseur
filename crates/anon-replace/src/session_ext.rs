//! Enregistrement des valeurs validées dans la session : attribution des
//! balises, liaison PERSONNE/NOM/PRENOM, variantes, liste « jamais ».

use crate::normalize::{self, compact_key, fold_keep_case, key};
use anon_core::{Entity, EntityKind, Session, Source, Tag, Treatment};

/// Une valeur dont la clé fait au plus ce nombre de caractères est, par défaut,
/// sensible à la casse (sigles, initiales, mots courts).
pub const CASE_SENSITIVE_MAX_CHARS: usize = 3;

/// Types dont la valeur correspond quels que soient les espaces, points ou
/// tirets entre ses caractères (`06 12 34 56 78` = `06.12.34.56.78`).
pub fn has_variable_separators(kind: EntityKind) -> bool {
    matches!(
        kind,
        EntityKind::Tel
            | EntityKind::Iban
            | EntityKind::Nir
            | EntityKind::Siret
            | EntityKind::Ridet
            | EntityKind::Cb
    )
}

/// Clé de comparaison selon le type : sans séparateurs pour les types à
/// séparateurs variables, [`key`] sinon.
pub fn match_key(kind: EntityKind, value: &str) -> String {
    if has_variable_separators(kind) {
        compact_key(value)
    } else {
        key(value)
    }
}

/// Opérations d'enregistrement sur une [`Session`].
pub trait SessionExt {
    /// Balise de la valeur, créée au besoin. `None` si la valeur est vide ou
    /// dans la liste « jamais ». Sensibilité à la casse par défaut (voir
    /// [`CASE_SENSITIVE_MAX_CHARS`]).
    fn intern(&mut self, kind: EntityKind, value: &str, source: Source) -> Option<Tag>;

    /// Comme [`intern`](Self::intern), en imposant la sensibilité à la casse
    /// (y compris sur une entité existante).
    fn intern_with_case(
        &mut self,
        kind: EntityKind,
        value: &str,
        source: Source,
        case_sensitive: bool,
    ) -> Option<Tag>;

    /// Ajoute une forme de surface à l'entité. Faux si l'entité n'existe pas,
    /// si la forme est vide, dans la liste « jamais » ou déjà connue.
    fn add_variant(&mut self, tag: Tag, variant: &str) -> bool;

    /// Ajoute une valeur à la liste « ne jamais anonymiser ». Faux si vide ou déjà présente.
    fn add_never(&mut self, value: &str) -> bool;

    fn set_enabled(&mut self, tag: Tag, enabled: bool) -> bool;
    fn set_case_sensitive(&mut self, tag: Tag, case_sensitive: bool) -> bool;
    fn set_treatment(&mut self, tag: Tag, treatment: Treatment) -> bool;

    /// Balise d'une valeur déjà enregistrée (valeur canonique ou variante).
    fn tag_for(&self, kind: EntityKind, value: &str) -> Option<Tag>;

    /// Vrai si la clé de la valeur est dans la liste « jamais ».
    fn is_never(&self, value: &str) -> bool;
}

fn find_entity(session: &Session, kind: EntityKind, mk: &str) -> Option<usize> {
    session.entities.iter().position(|e| {
        e.tag.kind == kind
            && std::iter::once(&e.value)
                .chain(e.variants.iter())
                .any(|v| match_key(kind, v) == mk)
    })
}

fn tokens(k: &str) -> Vec<&str> {
    k.split(' ').filter(|t| !t.is_empty()).collect()
}

/// Vrai si les mots de `part` forment une suite contiguë des mots de `whole`.
fn contains_words(whole: &str, part: &str) -> bool {
    let w = tokens(whole);
    let p = tokens(part);
    !p.is_empty() && w.windows(p.len()).any(|win| win == p.as_slice())
}

fn slot_free(session: &Session, kind: EntityKind, index: u32) -> bool {
    session.entity(Tag::new(kind, index)).is_none()
}

/// Indice d'une personne déjà connue à laquelle rattacher cette partie de nom
/// (ou ce nom complet). La première entité candidate gagne.
fn linked_person_index(session: &Session, kind: EntityKind, k: &str) -> Option<u32> {
    session
        .entities
        .iter()
        .filter(|e| e.tag.kind.is_person_part() && slot_free(session, kind, e.tag.index))
        .find(|e| {
            let ek = key(&e.value);
            match (kind, e.tag.kind) {
                (EntityKind::Personne, EntityKind::Nom | EntityKind::Prenom) => {
                    contains_words(k, &ek)
                }
                (EntityKind::Nom | EntityKind::Prenom, EntityKind::Personne) => {
                    contains_words(&ek, k)
                }
                _ => false,
            }
        })
        .map(|e| e.tag.index)
}

fn next_index(session: &Session, kind: EntityKind) -> u32 {
    session
        .entities
        .iter()
        .filter(|e| e.tag.kind == kind || (kind.is_person_part() && e.tag.kind.is_person_part()))
        .map(|e| e.tag.index)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn keep_case(value: &str) -> String {
    fold_keep_case(value).text.trim_matches(' ').to_string()
}

fn intern_impl(
    session: &mut Session,
    kind: EntityKind,
    value: &str,
    source: Source,
    forced_case: Option<bool>,
) -> Option<Tag> {
    let mk = match_key(kind, value);
    if mk.is_empty() || session.is_never(value) {
        return None;
    }
    let surface = value.trim();
    if let Some(i) = find_entity(session, kind, &mk) {
        let entity = session.entities.get_mut(i)?;
        if let Some(cs) = forced_case {
            entity.case_sensitive = cs;
        }
        // Une forme de casse différente d'une entité sensible à la casse ne serait
        // jamais remplacée : on l'ajoute comme variante.
        if entity.case_sensitive {
            let form = keep_case(surface);
            let known = std::iter::once(&entity.value)
                .chain(entity.variants.iter())
                .any(|v| keep_case(v) == form);
            if !known {
                entity.variants.push(surface.to_string());
            }
        }
        return Some(entity.tag);
    }
    let index = if kind.is_person_part() {
        linked_person_index(session, kind, &normalize::key(value))
            .unwrap_or_else(|| next_index(session, kind))
    } else {
        next_index(session, kind)
    };
    let tag = Tag::new(kind, index);
    let case_sensitive =
        forced_case.unwrap_or_else(|| key(value).chars().count() <= CASE_SENSITIVE_MAX_CHARS);
    session.entities.push(Entity {
        tag,
        value: surface.to_string(),
        variants: Vec::new(),
        case_sensitive,
        enabled: true,
        treatment: Treatment::Tag,
        source,
    });
    Some(tag)
}

impl SessionExt for Session {
    fn intern(&mut self, kind: EntityKind, value: &str, source: Source) -> Option<Tag> {
        intern_impl(self, kind, value, source, None)
    }

    fn intern_with_case(
        &mut self,
        kind: EntityKind,
        value: &str,
        source: Source,
        case_sensitive: bool,
    ) -> Option<Tag> {
        intern_impl(self, kind, value, source, Some(case_sensitive))
    }

    fn add_variant(&mut self, tag: Tag, variant: &str) -> bool {
        let v = variant.trim();
        if key(v).is_empty() || self.is_never(v) {
            return false;
        }
        let Some(entity) = self.entity_mut(tag) else {
            return false;
        };
        if entity.value == v || entity.variants.iter().any(|x| x == v) {
            return false;
        }
        entity.variants.push(v.to_string());
        true
    }

    fn add_never(&mut self, value: &str) -> bool {
        let k = key(value);
        if k.is_empty() || self.never.iter().any(|n| key(n) == k) {
            return false;
        }
        self.never.push(value.trim().to_string());
        true
    }

    fn set_enabled(&mut self, tag: Tag, enabled: bool) -> bool {
        self.entity_mut(tag).map(|e| e.enabled = enabled).is_some()
    }

    fn set_case_sensitive(&mut self, tag: Tag, case_sensitive: bool) -> bool {
        self.entity_mut(tag)
            .map(|e| e.case_sensitive = case_sensitive)
            .is_some()
    }

    fn set_treatment(&mut self, tag: Tag, treatment: Treatment) -> bool {
        self.entity_mut(tag)
            .map(|e| e.treatment = treatment)
            .is_some()
    }

    fn tag_for(&self, kind: EntityKind, value: &str) -> Option<Tag> {
        let mk = match_key(kind, value);
        if mk.is_empty() {
            return None;
        }
        let i = find_entity(self, kind, &mk)?;
        self.entities.get(i).map(|e| e.tag)
    }

    fn is_never(&self, value: &str) -> bool {
        let k = key(value);
        !k.is_empty() && self.never.iter().any(|n| key(n) == k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use EntityKind::*;

    fn t(kind: EntityKind, i: u32) -> Option<Tag> {
        Some(Tag::new(kind, i))
    }

    #[test]
    fn same_key_same_tag() {
        let mut s = Session::new();
        assert_eq!(
            s.intern(Societe, "Acmé Conseil", Source::Manual),
            t(Societe, 1)
        );
        assert_eq!(
            s.intern(Societe, " ACME  conseil ", Source::Rule),
            t(Societe, 1)
        );
        assert_eq!(s.intern(Societe, "Globex", Source::Rule), t(Societe, 2));
        assert_eq!(s.entities.len(), 2);
        assert_eq!(s.intern(Lieu, "Globex", Source::Rule), t(Lieu, 1));
    }

    #[test]
    fn empty_or_never_is_refused() {
        let mut s = Session::new();
        assert_eq!(s.intern(Nom, "  \u{200B} ", Source::Manual), None);
        assert!(s.add_never("Pierre"));
        assert!(!s.add_never("PIERRE"));
        assert_eq!(s.intern(Prenom, "pierre", Source::Dictionary), None);
        assert!(s.entities.is_empty());
    }

    #[test]
    fn variants_resolve_to_entity() {
        let mut s = Session::new();
        let tag = s.intern(Personne, "Jean Dupont", Source::Ner).unwrap();
        assert!(s.add_variant(tag, "J. Dupont"));
        assert!(!s.add_variant(tag, "J. Dupont"));
        assert!(!s.add_variant(Tag::new(Personne, 9), "x"));
        assert_eq!(s.tag_for(Personne, "j. dupont"), Some(tag));
        assert_eq!(s.intern(Personne, "J. DUPONT", Source::Ner), Some(tag));
        assert_eq!(s.tag_for(Nom, "Jean Dupont"), None);
    }

    #[test]
    fn person_parts_share_counter_and_link() {
        let mut s = Session::new();
        assert_eq!(
            s.intern(Personne, "Jean Dupont", Source::Ner),
            t(Personne, 1)
        );
        assert_eq!(s.intern(Societe, "Acme", Source::Ner), t(Societe, 1));
        assert_eq!(s.intern(Nom, "Dupont", Source::Ner), t(Nom, 1));
        assert_eq!(s.intern(Prenom, "Jean", Source::Ner), t(Prenom, 1));
        // Nom isolé : compteur partagé.
        assert_eq!(s.intern(Nom, "Lefort", Source::Ner), t(Nom, 2));
        assert_eq!(s.intern(Prenom, "Zoé", Source::Ner), t(Prenom, 3));
        // Une personne créée après son nom reprend son indice.
        assert_eq!(
            s.intern(Personne, "Marie Lefort", Source::Ner),
            t(Personne, 2)
        );
        // Prénom d'une personne existante : il reprend son indice.
        assert_eq!(s.intern(Prenom, "Marie", Source::Ner), t(Prenom, 2));
    }

    #[test]
    fn ambiguous_surname_first_person_wins() {
        let mut s = Session::new();
        assert_eq!(
            s.intern(Personne, "Jean Dupont", Source::Ner),
            t(Personne, 1)
        );
        assert_eq!(
            s.intern(Personne, "Marie Dupont", Source::Ner),
            t(Personne, 2)
        );
        assert_eq!(s.intern(Nom, "Dupont", Source::Ner), t(Nom, 1));
        assert_eq!(s.intern(Prenom, "Marie", Source::Ner), t(Prenom, 2));
    }

    #[test]
    fn part_must_be_whole_words() {
        let mut s = Session::new();
        s.intern(Personne, "Jean-Pierre Martinez", Source::Ner);
        assert_eq!(s.intern(Prenom, "Jean", Source::Ner), t(Prenom, 2));
        assert_eq!(s.intern(Nom, "Martin", Source::Ner), t(Nom, 3));
        assert_eq!(s.intern(Prenom, "Jean-Pierre", Source::Ner), t(Prenom, 1));
    }

    #[test]
    fn case_sensitivity_default_and_forced() {
        let mut s = Session::new();
        let short = s.intern(Societe, "ABC", Source::Rule).unwrap();
        let long = s.intern(Societe, "Pierre", Source::Rule).unwrap();
        assert!(s.entity(short).unwrap().case_sensitive);
        assert!(!s.entity(long).unwrap().case_sensitive);
        let forced = s
            .intern_with_case(Prenom, "Pierre", Source::Dictionary, true)
            .unwrap();
        assert!(s.entity(forced).unwrap().case_sensitive);
        assert!(s.set_case_sensitive(forced, false));
        assert!(!s.entity(forced).unwrap().case_sensitive);
        // Autre casse d'une entité sensible à la casse : ajoutée en variante.
        assert_eq!(s.intern(Societe, "Abc", Source::Rule), Some(short));
        assert_eq!(s.entity(short).unwrap().variants, vec!["Abc".to_string()]);
        assert_eq!(s.intern(Societe, "ABC", Source::Rule), Some(short));
        assert_eq!(s.entity(short).unwrap().variants.len(), 1);
    }

    #[test]
    fn variable_separator_kinds_share_key() {
        let mut s = Session::new();
        let tag = s.intern(Tel, "06 12 34 56 78", Source::Rule);
        assert_eq!(s.intern(Tel, "06.12.34.56.78", Source::Rule), tag);
        assert_eq!(s.intern(Tel, "0612345678", Source::Rule), tag);
        assert_eq!(s.tag_for(Tel, "06-12-34-56-78"), tag);
    }

    #[test]
    fn setters() {
        let mut s = Session::new();
        let tag = s.intern(Age, "42 ans", Source::Rule).unwrap();
        assert!(s.set_enabled(tag, false));
        assert!(!s.entity(tag).unwrap().enabled);
        assert!(s.set_treatment(tag, Treatment::Generalize("40-49 ans".into())));
        assert_eq!(
            s.entity(tag).unwrap().treatment,
            Treatment::Generalize("40-49 ans".into())
        );
        assert!(!s.set_enabled(Tag::new(Age, 7), true));
    }
}
