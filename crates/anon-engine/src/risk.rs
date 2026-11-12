//! Risque de réidentification résiduel : quasi-identifiants encore visibles
//! dans le fichier produit.

use anon_core::EntityKind;
use std::collections::BTreeMap;

/// Nombre total d'occurrences de quasi-identifiants à partir duquel le risque est élevé.
pub const RISK_HIGH_TOTAL: usize = 10;
/// Nombre de types distincts de quasi-identifiants à partir duquel le risque est élevé.
pub const RISK_HIGH_KINDS: usize = 3;
/// Nombre d'occurrences à partir duquel le risque est modéré.
pub const RISK_MODERATE_TOTAL: usize = 1;

/// Niveau de risque résiduel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RiskLevel {
    Faible,
    Modere,
    Eleve,
}

impl RiskLevel {
    /// Libellé français.
    pub fn label(self) -> &'static str {
        match self {
            RiskLevel::Faible => "Faible",
            RiskLevel::Modere => "Modéré",
            RiskLevel::Eleve => "Élevé",
        }
    }
}

/// Bilan du risque résiduel d'un fichier produit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskReport {
    pub level: RiskLevel,
    /// Quasi-identifiants encore visibles : type et nombre d'occurrences.
    pub signals: Vec<(EntityKind, usize)>,
    /// Conseils en français.
    pub advice: Vec<String>,
}

/// Rappel ajouté à tout bilan.
pub const RISK_REMINDER: &str = "L'anonymisation réduit le risque de réidentification, \
    elle ne l'annule jamais : relisez le document avant de le partager.";

fn advice_for(kind: EntityKind) -> Option<&'static str> {
    Some(match kind {
        EntityKind::Date => {
            "Des dates restent visibles : remplacez-les par une balise ou ne gardez que l'année."
        }
        EntityKind::Age => "Des âges restent visibles : préférez une tranche d'âge (30-39 ans).",
        EntityKind::Fonction => {
            "Des fonctions restent visibles : une fonction rare peut suffire à désigner une personne."
        }
        EntityKind::Lieu => {
            "Des lieux restent visibles : une petite commune peut suffire à désigner une personne."
        }
        EntityKind::Montant => {
            "Des montants restent visibles : arrondissez-les ou remplacez-les par une balise."
        }
        EntityKind::Matricule => "Des matricules restent visibles : remplacez-les par une balise.",
        _ => return None,
    })
}

/// Calcule le bilan à partir du nombre d'occurrences par type.
pub(crate) fn assess(counts: &BTreeMap<EntityKind, usize>) -> RiskReport {
    let signals: Vec<(EntityKind, usize)> = counts
        .iter()
        .filter(|(k, n)| k.is_quasi_identifier() && **n > 0)
        .map(|(k, n)| (*k, *n))
        .collect();
    let total: usize = signals.iter().map(|(_, n)| n).sum();
    let level = if total >= RISK_HIGH_TOTAL || signals.len() >= RISK_HIGH_KINDS {
        RiskLevel::Eleve
    } else if total >= RISK_MODERATE_TOTAL {
        RiskLevel::Modere
    } else {
        RiskLevel::Faible
    };
    let mut advice: Vec<String> = signals
        .iter()
        .filter_map(|(k, _)| advice_for(*k))
        .map(str::to_string)
        .collect();
    if level == RiskLevel::Eleve {
        advice.push(
            "Plusieurs informations indirectes se combinent : utilisez le profil Renforcé \
             et vérifiez la liste avant d'anonymiser."
                .into(),
        );
    }
    advice.push(RISK_REMINDER.into());
    RiskReport {
        level,
        signals,
        advice,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        let none = assess(&BTreeMap::new());
        assert_eq!(none.level, RiskLevel::Faible);
        assert_eq!(none.advice, vec![RISK_REMINDER.to_string()]);

        let one = assess(&BTreeMap::from([(EntityKind::Date, 2)]));
        assert_eq!(one.level, RiskLevel::Modere);
        assert_eq!(one.signals, vec![(EntityKind::Date, 2)]);

        let many = assess(&BTreeMap::from([
            (EntityKind::Date, 1),
            (EntityKind::Age, 1),
            (EntityKind::Lieu, 1),
        ]));
        assert_eq!(many.level, RiskLevel::Eleve);
        assert!(many.advice.last().is_some_and(|a| a == RISK_REMINDER));

        // Les identifiants directs ne sont pas des signaux de risque résiduel.
        let direct = assess(&BTreeMap::from([(EntityKind::Email, 4)]));
        assert_eq!(direct.level, RiskLevel::Faible);
    }
}
