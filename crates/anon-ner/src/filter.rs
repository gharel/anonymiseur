//! Tri des propositions brutes du modèle avant de les rendre au moteur :
//! fragments et morceaux d'adresse email écartés, communes reclassées en lieux,
//! titres, libellés et mots courants rendus « à confirmer », confiance calibrée.
//!
//! Le moteur coche par défaut les lignes dont la confiance atteint
//! `anon_engine::SELECT_MIN_CONFIDENCE` (0,6). Une proposition sûre de l'IA garde sa
//! probabilité (≥ [`SURE_CONFIDENCE`]) ; une proposition incertaine est ramenée à
//! [`UNSURE_CONFIDENCE`], sous ce seuil : elle reste visible, décochée.

use crate::aggregate::Span;
use anon_core::entities::EntityKind;

/// Probabilité moyenne à partir de laquelle une proposition de l'IA seule est sûre.
pub const SURE_CONFIDENCE: f32 = 0.85;
/// Confiance rendue pour une proposition incertaine (sous le seuil de sélection du moteur).
pub const UNSURE_CONFIDENCE: f32 = 0.45;
/// Nombre minimal de lettres d'une entité.
const MIN_LETTERS: usize = 2;
/// Libellés d'identifiants que le modèle prend parfois pour des noms.
const IDENTIFIER_LABELS: &[&str] = &[
    "RIDET", "SIRET", "SIREN", "IBAN", "BIC", "RIB", "TVA", "RCS", "NIR", "CAFAT", "KBIS", "APE",
    "NAF", "URSSAF", "SWIFT",
];

#[derive(Debug, PartialEq)]
enum Verdict {
    Drop,
    Keep { kind: EntityKind, sure: bool },
}

/// Applique le tri à des entités trouvées dans `text`.
pub fn refine(text: &str, spans: Vec<Span>) -> Vec<Span> {
    spans
        .into_iter()
        .filter_map(|s| {
            let surface = text.get(s.start..s.end)?;
            match judge(text, &s, surface) {
                Verdict::Drop => None,
                Verdict::Keep { kind, sure } => Some(Span {
                    kind,
                    confidence: if sure && s.confidence >= SURE_CONFIDENCE {
                        s.confidence
                    } else {
                        s.confidence.min(UNSURE_CONFIDENCE)
                    },
                    ..s
                }),
            }
        })
        .collect()
}

fn judge(text: &str, s: &Span, surface: &str) -> Verdict {
    let letters = surface.chars().filter(|c| c.is_alphabetic()).count();
    if letters < MIN_LETTERS {
        return Verdict::Drop;
    }
    if inside_email_or_url(text, s.start, s.end) {
        return Verdict::Drop;
    }
    let named = matches!(s.kind, EntityKind::Personne | EntityKind::Societe);
    let cased = surface.chars().any(char::is_lowercase);
    if named && cased && !surface.chars().any(char::is_uppercase) {
        // « heeft », « garnier » : un nom propre porte une majuscule (écritures à
        // casse seulement : l'arabe, le chinois… n'en ont pas).
        return Verdict::Keep {
            kind: s.kind,
            sure: false,
        };
    }
    let words: Vec<&str> = surface
        .split(|c: char| c.is_whitespace())
        .filter(|w| w.chars().any(char::is_alphabetic))
        .collect();
    // « RIDET », « SIRET », « IBAN » : le libellé d'un identifiant, pas un nom
    // (certains existent pourtant comme noms de famille).
    if let [seul] = words.as_slice() {
        let libelle = seul
            .trim_matches(|c: char| !c.is_alphabetic())
            .to_uppercase();
        if IDENTIFIER_LABELS.contains(&libelle.as_str()) {
            return Verdict::Drop;
        }
    }
    if s.kind != EntityKind::Personne {
        // Un en-tête de colonne seul dans son bloc (« Ville ») n'est pas un lieu.
        let whole_block = text.trim() == surface;
        if whole_block && words.iter().all(|w| anon_dict::is_common_word(w)) {
            return Verdict::Drop;
        }
        return Verdict::Keep {
            kind: s.kind,
            sure: true,
        };
    }
    let has_first_name = words.iter().any(|w| anon_dict::first_name(w).is_some());
    let has_name = has_first_name || words.iter().any(|w| anon_dict::surname(w).is_some());
    // Une commune connue n'est pas une personne (« Païta », « Dumbéa »), sauf prénom
    // ou nom de famille très courant (« Fontaine »).
    let very_common_surname =
        anon_dict::surname(surface).is_some_and(|t| t >= anon_dict::Tier::VeryCommon);
    if !has_first_name && !very_common_surname && anon_dict::commune(surface) {
        return Verdict::Keep {
            kind: EntityKind::Lieu,
            sure: true,
        };
    }
    let all_common = words.iter().all(|w| anon_dict::is_common_word(w));
    let all_caps = surface
        .chars()
        .filter(|c| c.is_alphabetic())
        .all(char::is_uppercase);
    let whole_block = text.trim() == surface;
    if all_common && !has_first_name {
        // Titre en capitales (« DONNÉES FICTIVES ») ou libellé seul (« Prénom »).
        if (all_caps && words.len() > 1) || whole_block {
            return Verdict::Drop;
        }
        return Verdict::Keep {
            kind: EntityKind::Personne,
            sure: false,
        };
    }
    if all_caps && !has_name {
        return Verdict::Keep {
            kind: EntityKind::Personne,
            sure: false,
        };
    }
    Verdict::Keep {
        kind: EntityKind::Personne,
        sure: true,
    }
}

/// Vrai si l'entité fait partie d'un mot sans espace contenant `@` ou `://`
/// (adresse email, lien) : les règles à clé s'en chargent.
fn inside_email_or_url(text: &str, start: usize, end: usize) -> bool {
    let before = text.get(..start).unwrap_or("");
    let after = text.get(end..).unwrap_or("");
    let left = before
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or_default();
    let right = after.split(char::is_whitespace).next().unwrap_or_default();
    let run = [left, right];
    run.iter()
        .any(|p| p.contains('@') || p.contains("://") || p.contains("www."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, part: &str, kind: EntityKind, confidence: f32) -> Span {
        let start = text.find(part).unwrap();
        Span {
            kind,
            start,
            end: start + part.len(),
            confidence,
        }
    }

    fn one(text: &str, part: &str, kind: EntityKind, conf: f32) -> Option<Span> {
        refine(text, vec![span(text, part, kind, conf)])
            .into_iter()
            .next()
    }

    #[test]
    fn keeps_real_names_with_their_confidence() {
        let s = one(
            "Signé par Hinatea Teriitahi.",
            "Hinatea Teriitahi",
            EntityKind::Personne,
            0.97,
        )
        .unwrap();
        assert_eq!(s.kind, EntityKind::Personne);
        assert!((s.confidence - 0.97).abs() < 1e-6);
        let s = one(
            "Le stagiaire Hoàng Đức Anh arrive.",
            "Hoàng Đức Anh",
            EntityKind::Personne,
            0.9,
        )
        .unwrap();
        assert!(s.confidence >= SURE_CONFIDENCE);
    }

    #[test]
    fn low_probability_becomes_unsure() {
        let s = one(
            "Vu avec Wanyatro Xoxuedr.",
            "Wanyatro Xoxuedr",
            EntityKind::Personne,
            0.7,
        )
        .unwrap();
        assert_eq!(s.confidence, UNSURE_CONFIDENCE);
    }

    #[test]
    fn drops_fragments_and_email_parts() {
        assert!(one("M. le directeur", "M", EntityKind::Personne, 0.99).is_none());
        let t = "Écrire à jean.dupont@example.com demain.";
        assert!(one(t, "jean.dupont", EntityKind::Personne, 0.99).is_none());
        let t = "Contact : olivier.nguyen@example.org";
        assert!(one(t, "olivier", EntityKind::Personne, 0.99).is_none());
    }

    #[test]
    fn communes_become_places() {
        let s = one(
            "Réunion du 14 mars à Païta, chez le client.",
            "Païta",
            EntityKind::Personne,
            0.95,
        )
        .unwrap();
        assert_eq!(s.kind, EntityKind::Lieu);
        let s = one(
            "Rendez-vous à Dumbéa.",
            "Dumbéa",
            EntityKind::Personne,
            0.95,
        )
        .unwrap();
        assert_eq!(s.kind, EntityKind::Lieu);
        // Nom de famille très courant qui est aussi une commune : reste une personne.
        let s = one(
            "Isabelle Ngo, épouse Fontaine, trésorière.",
            "Fontaine",
            EntityKind::Personne,
            0.95,
        )
        .unwrap();
        assert_eq!(s.kind, EntityKind::Personne);
    }

    #[test]
    fn headings_and_labels_are_dropped_common_words_unsure() {
        assert!(
            one(
                "DONNÉES FICTIVES — exemple",
                "DONNÉES FICTIVES",
                EntityKind::Personne,
                0.95
            )
            .is_none()
        );
        assert!(one("Prénom", "Prénom", EntityKind::Personne, 0.95).is_none());
        let s = one(
            "Vue sur la Martinique depuis le bateau.",
            "Martinique",
            EntityKind::Personne,
            0.95,
        )
        .unwrap();
        assert_eq!(s.confidence, UNSURE_CONFIDENCE);
        let s = one(
            "Mevrouw Sanne heeft goedgekeurd.",
            "heeft",
            EntityKind::Personne,
            0.95,
        )
        .unwrap();
        assert_eq!(s.confidence, UNSURE_CONFIDENCE);
        // Écriture sans casse : pas de rétrogradation.
        let t = "وقّع السيد خالد المنصوري العقد";
        let s = one(t, "خالد المنصوري", EntityKind::Personne, 0.95).unwrap();
        assert!(s.confidence >= SURE_CONFIDENCE);
    }

    #[test]
    fn identifier_labels_and_lone_headers_are_dropped() {
        // « Ridet » existe comme nom de famille : c'est pourtant un libellé ici.
        let t = "Nautilus Pacifique SARL, RIDET 1 234 567.001";
        assert!(one(t, "RIDET", EntityKind::Personne, 0.97).is_none());
        assert!(
            one(
                "SIRET : 412 345 670 00019",
                "SIRET",
                EntityKind::Societe,
                0.9
            )
            .is_none()
        );
        // Un en-tête de colonne seul dans sa cellule n'est pas un lieu…
        assert!(one("Ville", "Ville", EntityKind::Lieu, 0.95).is_none());
        // …mais un vrai lieu seul dans sa cellule le reste.
        assert!(one("Nouméa", "Nouméa", EntityKind::Lieu, 0.95).is_some());
    }

    #[test]
    fn capitalised_names_stay_sure() {
        let s = one(
            "Présents : DUPONT Jean, gérant.",
            "DUPONT Jean",
            EntityKind::Personne,
            0.95,
        )
        .unwrap();
        assert!(s.confidence >= SURE_CONFIDENCE);
        assert_eq!(s.kind, EntityKind::Personne);
    }
}
