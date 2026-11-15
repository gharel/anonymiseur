//! Évaluation de l'anonymat selon le projet de lignes directrices 02/2026 du
//! CEPD : isolement, recoupement, déduction, sujet par sujet, avec une
//! correction proposée pour chaque échec.
//!
//! - Tableaux (`table`) : un sujet par groupe de lignes qui partagent les mêmes
//!   indices affichés ; k-anonymat, identifiants directs et stables,
//!   l-diversité ; correction par généralisation de type Datafly et suppression
//!   bornée par [`MAX_SUPPRESSION_RATE`].
//! - Images (`image`) : visages visibles ou non vérifiés.
//! - Textes (`text`) : un sujet par personne mentionnée ; identifiants directs,
//!   estimation de la taille du groupe à partir des indices (lieu, âge, métier,
//!   fonction dans une organisation nommée), identifiants stables, données
//!   sensibles liées à une personne isolable. Approche simplifiée en V2.0,
//!   quelles que soient les réponses de `AssessSettings::answers`.
//!
//! Exigence envers le moteur qui construit l'[`AssessInput`] : chaque cellule
//! non vide d'une colonne de tableau évaluée (rôle autre que `Other`) porte un
//! `value_id`. Sans lui, l'échec est signalé mais aucune correction n'est
//! proposée.
//!
//! Le verdict est `Anonymous` seulement si chaque sujet passe les trois critères
//! (§ 36) ; `Inconclusive` si des visages n'ont pas été vérifiés et que rien
//! d'autre n'échoue.

mod image;
mod table;
mod text;

pub use anon_core::assess::{K_MIN_DEFAULT, MAX_SUPPRESSION_RATE};
use anon_core::assess::{K_MIN_HIGHEST, K_MIN_LOWEST};
use anon_core::{AssessInput, AssessSettings, Assessment, Failure, FailureReason, Verdict};
use std::collections::HashSet;

/// Évalue un dossier pour un destinataire.
pub fn assess(input: &AssessInput, settings: &AssessSettings) -> Assessment {
    let k_min = settings.k_min.clamp(K_MIN_LOWEST, K_MIN_HIGHEST);
    let settings = AssessSettings { k_min, ..*settings };
    let mut subjects = Vec::new();
    let mut failures = Vec::new();

    let (s, f) = text::assess_texts(&input.texts, k_min);
    subjects.extend(s);
    failures.extend(f);
    for (i, t) in input.tables.iter().enumerate() {
        let (s, f) = table::assess_table(i, t, k_min);
        subjects.extend(s);
        failures.extend(f);
    }
    let (s, f) = image::assess_images(&input.images);
    subjects.extend(s);
    failures.extend(f);

    Assessment {
        settings,
        approach: settings.answers.approach(),
        verdict: verdict(&failures),
        subjects,
        failures,
    }
}

/// Verdict : un seul sujet en échec suffit (§ 36) ; les visages non vérifiés
/// rendent l'évaluation non concluante quand rien d'autre n'échoue.
fn verdict(failures: &[Failure]) -> Verdict {
    let failing: HashSet<_> = failures
        .iter()
        .filter(|f| f.reason != FailureReason::FacesNotChecked)
        .map(|f| &f.subject)
        .collect();
    if !failing.is_empty() {
        Verdict::Reidentifiable {
            subjects: failing.len(),
        }
    } else if failures
        .iter()
        .any(|f| f.reason == FailureReason::FacesNotChecked)
    {
        Verdict::Inconclusive
    } else {
        Verdict::Anonymous
    }
}

#[cfg(test)]
mod tests;
