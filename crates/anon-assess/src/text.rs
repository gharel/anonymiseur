//! Évaluation des textes : une personne mentionnée est un sujet (profil
//! d'indices, estimation de groupe, déduction). À remplir au lot 4 de la V2 ;
//! pour l'instant aucun sujet, le verdict n'en dépend pas.

use anon_core::{Failure, SubjectReport, TextUnit};

/// Évalue les textes : (bilans, échecs). Aucun sujet pour l'instant.
pub(crate) fn assess_texts(units: &[TextUnit], k_min: u32) -> (Vec<SubjectReport>, Vec<Failure>) {
    let _ = (units, k_min);
    (Vec::new(), Vec::new())
}
