//! Contrat des détecteurs. CONTRAT partagé.

use crate::entities::{Detection, EntityKind};
use crate::text::TextBlock;
use std::collections::BTreeSet;

/// Niveau de protection choisi par l'utilisateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// Identifiants directs : personnes, sociétés, contacts, identifiants, finances.
    #[default]
    Standard,
    /// Standard + quasi-identifiants (dates, âges, fonctions, lieux, montants, matricules).
    Renforce,
}

/// Réglages de détection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectOptions {
    pub profile: Profile,
    /// Types désactivés par l'utilisateur.
    pub disabled: BTreeSet<EntityKind>,
}

impl Default for DetectOptions {
    fn default() -> Self {
        Self {
            profile: Profile::Standard,
            disabled: BTreeSet::new(),
        }
    }
}

impl DetectOptions {
    /// Vrai si ce type doit être détecté avec ces réglages. Les catégories
    /// particulières (santé, religion…) sont voulues dans les deux profils.
    pub fn wants(&self, kind: EntityKind) -> bool {
        if self.disabled.contains(&kind) {
            return false;
        }
        match self.profile {
            Profile::Renforce => true,
            Profile::Standard => kind.is_sensitive() || !kind.is_quasi_identifier(),
        }
    }
}

/// Un détecteur trouve des occurrences dans un bloc. Sans état, sans effet de bord.
/// Les détections renvoyées peuvent se chevaucher : la fusion se fait dans
/// `anon_detect::pipeline`.
pub trait Detector: Sync {
    /// Nom court pour les journaux et les tests (`"rules"`, `"dictionary"`, `"ner"`).
    fn name(&self) -> &'static str;

    fn detect(&self, block: &TextBlock, opts: &DetectOptions) -> Vec<Detection>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_profile_skips_quasi_identifiers() {
        let opts = DetectOptions::default();
        assert!(opts.wants(EntityKind::Email));
        assert!(!opts.wants(EntityKind::Date));
        let renforce = DetectOptions {
            profile: Profile::Renforce,
            ..Default::default()
        };
        assert!(renforce.wants(EntityKind::Date));
    }

    #[test]
    fn sensitive_kinds_are_wanted_in_both_profiles() {
        let renforce = DetectOptions {
            profile: Profile::Renforce,
            ..Default::default()
        };
        for kind in EntityKind::SENSITIVE {
            assert!(DetectOptions::default().wants(*kind), "{kind:?}");
            assert!(renforce.wants(*kind), "{kind:?}");
        }
        // L'événement personnel est un indice : profil « Renforcé » seulement.
        assert!(!DetectOptions::default().wants(EntityKind::Evenement));
        assert!(renforce.wants(EntityKind::Evenement));
        let mut off = renforce.clone();
        off.disabled.insert(EntityKind::Sante);
        assert!(!off.wants(EntityKind::Sante));
    }

    #[test]
    fn disabled_kind_is_never_wanted() {
        let mut opts = DetectOptions {
            profile: Profile::Renforce,
            ..Default::default()
        };
        opts.disabled.insert(EntityKind::Montant);
        assert!(!opts.wants(EntityKind::Montant));
    }
}
