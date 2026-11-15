//! Bilan **provisoire** de l'étape Bilan (lot 7a). Il compte les valeurs
//! masquées, celles à confirmer et les indices encore visibles, et reprend
//! l'estimation du risque de la V1 (seuils `RISK_*` d'`anon-engine`). Il ne
//! dit pas si une personne reste reconnaissable : le lot 8 le remplace par
//! l'évaluation des trois critères (`anon-assess`). Sans egui.

use super::anonymize::{AnonymizeState, needs_confirmation};
use anon_core::{EntityKind, Treatment};
use anon_engine::{RISK_HIGH_KINDS, RISK_HIGH_TOTAL, RISK_MODERATE_TOTAL, RiskLevel};
use std::collections::BTreeMap;

/// Résumé chiffré du dossier avant envoi.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    /// Documents analysés.
    pub documents: usize,
    /// Valeurs cochées : masquées par une balise ou rendues moins précises.
    pub masked: usize,
    /// Dont rendues moins précises.
    pub less_precise: usize,
    /// Valeurs courtes ou incertaines, à confirmer.
    pub to_confirm: usize,
    /// Indices (quasi-identifiants) décochés, donc visibles : type et nombre de fois.
    pub clues: Vec<(EntityKind, usize)>,
    /// Zones tracées à la main sur les images.
    pub zones: usize,
}

impl Summary {
    /// Nombre total d'apparitions d'indices encore visibles.
    pub fn clue_total(&self) -> usize {
        self.clues.iter().map(|(_, n)| n).sum()
    }

    /// Niveau de risque estimé avec les seuils de la V1.
    pub fn level(&self) -> RiskLevel {
        let total = self.clue_total();
        if total >= RISK_HIGH_TOTAL || self.clues.len() >= RISK_HIGH_KINDS {
            RiskLevel::Eleve
        } else if total >= RISK_MODERATE_TOTAL {
            RiskLevel::Modere
        } else {
            RiskLevel::Faible
        }
    }
}

/// Résumé de l'analyse courante.
pub fn summarize(st: &AnonymizeState) -> Summary {
    let Some(a) = st.analysis.as_ref() else {
        return Summary::default();
    };
    let mut clues: BTreeMap<EntityKind, usize> = BTreeMap::new();
    let mut s = Summary {
        documents: a.files.iter().filter(|f| f.error.is_none()).count(),
        zones: st.masks.len(),
        ..Summary::default()
    };
    for c in &a.candidates {
        if needs_confirmation(c) {
            s.to_confirm += 1;
        }
        if c.selected {
            s.masked += 1;
            if matches!(c.treatment, Treatment::Generalize(_)) {
                s.less_precise += 1;
            }
        } else if c.kind.is_quasi_identifier() {
            *clues.entry(c.kind).or_insert(0) += c.occurrences;
        }
    }
    s.clues = clues.into_iter().filter(|(_, n)| *n > 0).collect();
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_follows_the_v1_thresholds() {
        let mut s = Summary::default();
        assert_eq!(s.level(), RiskLevel::Faible);
        s.clues = vec![(EntityKind::Age, 1)];
        assert_eq!(s.level(), RiskLevel::Modere);
        s.clues = vec![(EntityKind::Date, RISK_HIGH_TOTAL)];
        assert_eq!(s.level(), RiskLevel::Eleve);
        s.clues = vec![
            (EntityKind::Date, 1),
            (EntityKind::Age, 1),
            (EntityKind::Lieu, 1),
        ];
        assert_eq!(s.clue_total(), 3);
        assert_eq!(s.level(), RiskLevel::Eleve, "trois types d'indices");
    }

    #[test]
    fn empty_state_has_an_empty_summary() {
        assert_eq!(summarize(&AnonymizeState::default()), Summary::default());
    }
}
