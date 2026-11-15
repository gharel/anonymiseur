//! Risque de réidentification résiduel : quasi-identifiants encore visibles
//! dans le fichier produit.
//!
//! **Obsolète (V1).** Ce rapport compte des occurrences d'indices et ne dit pas
//! si une personne reste reconnaissable (audit de la V2, constat n° 9).
//! L'évaluation des trois critères ([`crate::Engine::assess`]) le remplace ; il
//! reste calculé tant que l'interface de la V1 l'affiche, puis sera retiré.

use anon_core::{EntityKind, Profile};
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

/// Conseil d'un risque élevé en profil Standard : ce profil ne cherche pas les
/// informations indirectes.
pub const RISK_COMBINED_STANDARD: &str = "Plusieurs informations indirectes se combinent : \
    utilisez le profil Renforcé et vérifiez la liste avant d'anonymiser.";

/// Conseil d'un risque élevé en profil Renforcé : les informations indirectes
/// restées visibles ont été décochées ou n'ont pas été trouvées.
pub const RISK_COMBINED_RENFORCE: &str = "Plusieurs informations indirectes se combinent \
    encore : relancez l'anonymisation en les cochant dans la liste, ou retirez-les du \
    document avant de le partager.";

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
        EntityKind::Evenement => {
            "Des événements personnels restent visibles (licenciement, divorce, procédure) : \
             avec d'autres indices, ils peuvent désigner une personne."
        }
        _ => return None,
    })
}

/// Calcule le bilan à partir du nombre d'occurrences par type, pour le profil
/// qui a servi à l'analyse.
pub(crate) fn assess(counts: &BTreeMap<EntityKind, usize>, profile: Profile) -> RiskReport {
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
            match profile {
                Profile::Standard => RISK_COMBINED_STANDARD,
                Profile::Renforce => RISK_COMBINED_RENFORCE,
            }
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
        let none = assess(&BTreeMap::new(), Profile::Standard);
        assert_eq!(none.level, RiskLevel::Faible);
        assert_eq!(none.advice, vec![RISK_REMINDER.to_string()]);

        let one = assess(&BTreeMap::from([(EntityKind::Date, 2)]), Profile::Standard);
        assert_eq!(one.level, RiskLevel::Modere);
        assert_eq!(one.signals, vec![(EntityKind::Date, 2)]);

        let many = assess(
            &BTreeMap::from([
                (EntityKind::Date, 1),
                (EntityKind::Age, 1),
                (EntityKind::Lieu, 1),
            ]),
            Profile::Standard,
        );
        assert_eq!(many.level, RiskLevel::Eleve);
        assert!(many.advice.last().is_some_and(|a| a == RISK_REMINDER));

        // Les identifiants directs ne sont pas des signaux de risque résiduel.
        let direct = assess(&BTreeMap::from([(EntityKind::Email, 4)]), Profile::Standard);
        assert_eq!(direct.level, RiskLevel::Faible);
    }

    /// Chaque quasi-identifiant a son conseil.
    #[test]
    fn every_quasi_identifier_has_advice() {
        for kind in EntityKind::ALL.iter().filter(|k| k.is_quasi_identifier()) {
            assert!(advice_for(*kind).is_some(), "{kind:?}");
        }
        let ev = assess(
            &BTreeMap::from([(EntityKind::Evenement, 1)]),
            Profile::Renforce,
        );
        assert_eq!(ev.level, RiskLevel::Modere);
        assert!(
            ev.advice
                .iter()
                .any(|a| a.contains("événements personnels"))
        );
    }

    /// Le profil Renforcé n'est conseillé qu'à qui ne l'utilise pas encore.
    #[test]
    fn combined_advice_follows_profile() {
        let counts = BTreeMap::from([
            (EntityKind::Date, 1),
            (EntityKind::Age, 1),
            (EntityKind::Lieu, 1),
        ]);
        let standard = assess(&counts, Profile::Standard);
        assert!(standard.advice.iter().any(|a| a == RISK_COMBINED_STANDARD));
        assert!(!standard.advice.iter().any(|a| a == RISK_COMBINED_RENFORCE));

        let renforce = assess(&counts, Profile::Renforce);
        assert_eq!(renforce.level, RiskLevel::Eleve);
        assert!(renforce.advice.iter().any(|a| a == RISK_COMBINED_RENFORCE));
        assert!(
            !renforce
                .advice
                .iter()
                .any(|a| a.contains("profil Renforcé"))
        );

        // Sous le seuil élevé, aucun des deux conseils.
        let low = assess(&BTreeMap::from([(EntityKind::Date, 1)]), Profile::Standard);
        assert!(!low.advice.iter().any(|a| a == RISK_COMBINED_STANDARD));
    }
}
