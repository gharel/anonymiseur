//! Données factices de la maquette. Toutes inventées (invariant 5) : noms
//! fictifs, emails en `example.com`, numéros sans titulaire.
//!
//! Ce module disparaîtra quand l'interface sera branchée sur `anon-engine`.

use super::state::anonymize::{Candidate, FileReport, LeakCheck, Risk};
use super::state::deanonymize::RestoreReport;
use super::state::files::FileEntry;
use anon_core::{EntityKind as K, Tag};

/// Fichiers d'exemple.
pub fn files() -> Vec<FileEntry> {
    vec![
        FileEntry::new(
            "C:/Dossiers/Contrat de prestation - Atelier Horizon.docx".into(),
            Some(48_213),
        ),
        FileEntry::new(
            "C:/Dossiers/Suivi des stagiaires 2026.xlsx".into(),
            Some(122_880),
        ),
        FileEntry::new(
            "C:/Dossiers/Compte rendu comité.pdf".into(),
            Some(1_468_006),
        ),
    ]
}

fn cand(
    kind: K,
    index: u32,
    value: &str,
    context: &str,
    occurrences: u32,
    to_confirm: bool,
) -> Candidate {
    Candidate {
        kind,
        value: value.to_owned(),
        context: context.to_owned(),
        occurrences,
        tag: Tag::new(kind, index),
        selected: !to_confirm,
        to_confirm,
    }
}

/// Détections d'exemple, dont quelques quasi-identifiants (profil Renforcé)
/// et des lignes à confirmer.
pub fn candidates() -> Vec<Candidate> {
    vec![
        cand(
            K::Personne,
            1,
            "Claire Martin",
            "… signé par Claire Martin, gérante …",
            7,
            false,
        ),
        cand(
            K::Personne,
            2,
            "Julien Moreau",
            "… le stagiaire Julien Moreau a validé …",
            4,
            false,
        ),
        cand(
            K::Personne,
            3,
            "Inès Lefèvre",
            "… copie à Inès Lefèvre, comptable …",
            2,
            false,
        ),
        cand(K::Nom, 4, "Duval", "… M. Duval était absent …", 1, true),
        cand(
            K::Societe,
            1,
            "Atelier Horizon SARL",
            "… entre Atelier Horizon SARL et …",
            5,
            false,
        ),
        cand(
            K::Societe,
            2,
            "Les Jardins de Brume",
            "… intervention chez Les Jardins de Brume …",
            1,
            true,
        ),
        cand(
            K::Adresse,
            1,
            "12 rue des Tilleuls, 44000 Nantes",
            "… domiciliée 12 rue des Tilleuls …",
            2,
            false,
        ),
        cand(
            K::Lieu,
            1,
            "Saint-Aubin-des-Prés",
            "… session tenue à Saint-Aubin-des-Prés …",
            3,
            false,
        ),
        cand(
            K::Email,
            1,
            "claire.martin@example.com",
            "… écrire à claire.martin@example.com …",
            3,
            false,
        ),
        cand(
            K::Email,
            2,
            "contact@atelier-horizon.example.com",
            "… contact@atelier-horizon.example.com …",
            1,
            false,
        ),
        cand(
            K::Tel,
            1,
            "06 00 00 00 01",
            "… joignable au 06 00 00 00 01 …",
            2,
            false,
        ),
        cand(
            K::Url,
            1,
            "https://intranet.example.com/rh",
            "… voir https://intranet.example.com/rh …",
            1,
            true,
        ),
        cand(
            K::Nir,
            1,
            "1 99 01 00 000 000 00",
            "… n° de sécurité sociale 1 99 01 …",
            1,
            false,
        ),
        cand(
            K::Siret,
            1,
            "000 000 000 00000",
            "… SIRET 000 000 000 00000 …",
            1,
            false,
        ),
        cand(
            K::Matricule,
            1,
            "STG-0042",
            "… matricule STG-0042 …",
            6,
            false,
        ),
        cand(
            K::Iban,
            1,
            "FR00 0000 0000 0000 0000 0000 000",
            "… IBAN FR00 0000 0000 …",
            1,
            false,
        ),
        cand(
            K::Montant,
            1,
            "4 850 €",
            "… pour un montant de 4 850 € HT …",
            2,
            false,
        ),
        cand(
            K::DateNaiss,
            1,
            "03/04/1990",
            "… née le 03/04/1990 …",
            1,
            false,
        ),
        cand(
            K::Date,
            1,
            "14 mars 2026",
            "… réunion du 14 mars 2026 …",
            3,
            false,
        ),
        cand(K::Age, 1, "36 ans", "… âgée de 36 ans …", 1, true),
        cand(
            K::Fonction,
            1,
            "directrice des ventes",
            "… nommée directrice des ventes …",
            2,
            false,
        ),
    ]
}

/// Rapports d'exemple, un par fichier.
pub fn reports() -> Vec<FileReport> {
    vec![
        FileReport {
            name: "Contrat de prestation - Atelier Horizon.docx".into(),
            output_name: "Contrat de prestation - Atelier Horizon (anonymisé).docx".into(),
            replacements: 42,
            warnings: vec![],
            leak_check: LeakCheck::Passed,
            risk: Risk::Low,
            risk_reason: None,
        },
        FileReport {
            name: "Suivi des stagiaires 2026.xlsx".into(),
            output_name: "Suivi des stagiaires 2026 (anonymisé).xlsx".into(),
            replacements: 118,
            warnings: vec![
                "Feuille « Archives » masquée : traitée comme les autres.".into(),
                "Commentaire de cellule D14 anonymisé.".into(),
            ],
            leak_check: LeakCheck::Passed,
            risk: Risk::Medium,
            risk_reason: Some(
                "Âge, fonction et commune réunis dans une même ligne : une personne reste reconnaissable par recoupement.".into(),
            ),
        },
        FileReport {
            name: "Compte rendu comité.pdf".into(),
            output_name: "Compte rendu comité (anonymisé).pdf".into(),
            replacements: 17,
            warnings: vec!["Page 3 : texte scanné, lu par reconnaissance de caractères.".into()],
            leak_check: LeakCheck::Blocked(1),
            risk: Risk::High,
            risk_reason: Some("Une valeur validée subsiste dans une image : fichier non livré.".into()),
        },
    ]
}

/// Correspondance balise → valeur de la maquette.
const MAP: &[(&str, &str)] = &[
    ("[PERSONNE_1]", "Claire Martin"),
    ("[PERSONNE_2]", "Julien Moreau"),
    ("[SOCIETE_1]", "Atelier Horizon SARL"),
    ("[EMAIL_1]", "claire.martin@example.com"),
    ("[TEL_1]", "06 00 00 00 01"),
    ("[LIEU_1]", "Saint-Aubin-des-Prés"),
];

/// Texte d'exemple de l'onglet « Texte libre ».
pub const FREE_TEXT: &str = "Bonjour,\n\nClaire Martin (claire.martin@example.com, 06 00 00 00 01) \
souhaite inscrire Julien Moreau à la session de Saint-Aubin-des-Prés. \
La facture est à établir au nom d'Atelier Horizon SARL.\n\nMerci de confirmer les dates.";

/// Réponse d'IA d'exemple pour l'onglet « Désanonymiser ».
pub const AI_RESPONSE: &str = "Voici une proposition de courriel :\n\nBonjour [PERSONNE_1],\n\n\
Nous confirmons l'inscription de [PERSONNE_2] à la session de [LIEU_1]. \
La facture sera adressée à [SOCIETE_1]. Pour toute question, [PERSONNE_3] reste disponible.\n\n\
Bien cordialement.";

/// Anonymisation factice : remplace les valeurs connues par leurs balises.
pub fn anonymize_text(text: &str) -> (String, usize) {
    let mut out = text.to_owned();
    let mut n = 0;
    for (tag, value) in MAP {
        let count = out.matches(value).count();
        if count > 0 {
            n += count;
            out = out.replace(value, tag);
        }
    }
    (out, n)
}

/// Restauration factice : remplace les balises connues par leurs valeurs.
pub fn restore_text(text: &str) -> (String, usize) {
    let mut out = text.to_owned();
    let mut n = 0;
    for (tag, value) in MAP {
        let count = out.matches(tag).count();
        if count > 0 {
            n += count;
            out = out.replace(tag, value);
        }
    }
    (out, n)
}

/// Balises `[XXX_N]` présentes dans un texte.
fn tags_in(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        let after = rest.get(start..).unwrap_or_default();
        match after.find(']') {
            Some(end) => {
                let candidate = after.get(..=end).unwrap_or_default();
                let inner = candidate.trim_start_matches('[').trim_end_matches(']');
                let valid = inner.rsplit_once('_').is_some_and(|(prefix, num)| {
                    K::from_prefix(prefix).is_some() && num.parse::<u32>().is_ok()
                });
                if valid && !tags.iter().any(|t| t == candidate) {
                    tags.push(candidate.to_owned());
                }
                rest = after.get(end + 1..).unwrap_or_default();
            }
            None => break,
        }
    }
    tags
}

/// Restauration factice avec rapport.
pub fn restore(text: &str) -> RestoreReport {
    let (restored_text, restored) = restore_text(text);
    let present = tags_in(text);
    let unknown_tags = present
        .iter()
        .filter(|t| !MAP.iter().any(|(tag, _)| tag == t))
        .cloned()
        .collect();
    let missing_tags = MAP
        .iter()
        .filter(|(tag, _)| !present.iter().any(|t| t == tag))
        .map(|(tag, _)| (*tag).to_owned())
        .collect();
    RestoreReport {
        text: restored_text,
        restored: restored as u32,
        unknown_tags,
        missing_tags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_example_com() {
        for c in candidates() {
            if c.kind == K::Email {
                assert!(c.value.ends_with("example.com"), "{}", c.value);
            }
        }
    }

    #[test]
    fn restore_report_lists_unknown_and_missing_tags() {
        let r = restore(AI_RESPONSE);
        assert_eq!(r.unknown_tags, vec!["[PERSONNE_3]".to_owned()]);
        assert!(r.missing_tags.contains(&"[EMAIL_1]".to_owned()));
        assert!(r.text.contains("Bonjour Claire Martin"));
        assert_eq!(r.restored, 4);
    }

    #[test]
    fn tag_scanner_ignores_non_tags() {
        assert_eq!(
            tags_in("[a] [PERSONNE_x] [FOO_1] [EMAIL_2] [EMAIL_2] ["),
            vec!["[EMAIL_2]".to_owned()]
        );
    }
}
