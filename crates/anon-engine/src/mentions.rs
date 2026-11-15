//! Recherche des occurrences des valeurs de l'analyse dans un texte, partagée
//! par le comptage ([`Analysis::recount`]), la vue du document (module
//! `document`) et l'évaluation (module `evaluation`). Toutes passent par le
//! même `Replacer` que l'anonymisation : mêmes frontières de mots, même casse,
//! mêmes variantes, donc les mêmes occurrences.
//!
//! - [`ValueFinder`] : les valeurs de l'analyse (toutes, ou un sous-ensemble),
//!   enregistrées sur une session vierge dans l'ordre de `apply_selection`.
//! - [`MaskedFinder`] : ce que l'anonymisation remplacerait avec la sélection
//!   courante, enregistrée sur une session de départ.

use crate::analysis::{Analysis, Candidate};
use anon_core::{Session, Tag, Treatment};
use anon_replace::{Match, Replacer, SessionExt};
use std::collections::HashMap;

/// Occurrence d'une valeur de l'analyse dans un texte. Offsets en octets, sur
/// des frontières de caractères.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Found {
    pub start: usize,
    pub end: usize,
    /// Identifiant du candidat (`Candidate::id`).
    pub id: usize,
}

/// Valeurs de l'analyse sous forme de `Replacer`, et la valeur (identifiant de
/// candidat) de chaque balise.
pub(crate) struct ValueFinder {
    replacer: Replacer,
    owner: HashMap<Tag, usize>,
}

impl ValueFinder {
    /// Les valeurs que `keep` retient, enregistrées dans l'ordre de
    /// `apply_selection` (personnes complètes, autres types, noms et prénoms
    /// isolés) sur une session vierge qui porte la liste « jamais » de l'analyse.
    /// Deux valeurs qui reçoivent la même balise désignent la première enregistrée.
    pub(crate) fn new(a: &Analysis, keep: impl Fn(&Candidate) -> bool) -> Self {
        let mut s = Session::new();
        for n in &a.never {
            s.add_never(n);
        }
        let mut owner = HashMap::new();
        for i in a.ordered_indices() {
            let Some(c) = a.candidates.get(i) else {
                continue;
            };
            if !keep(c) {
                continue;
            }
            let Some(tag) = s.intern_with_case(c.kind, &c.value, c.source, c.case_sensitive) else {
                continue;
            };
            for v in &c.variants {
                s.add_variant(tag, v);
            }
            owner.entry(tag).or_insert(c.id);
        }
        Self {
            replacer: Replacer::new(&s),
            owner,
        }
    }

    /// Occurrences des valeurs dans `text`, dans l'ordre du texte, sans chevauchement.
    pub(crate) fn find(&self, text: &str) -> Vec<Found> {
        self.replacer
            .matches(text)
            .into_iter()
            .filter_map(|m| {
                self.owner.get(&m.tag).map(|&id| Found {
                    start: m.start,
                    end: m.end,
                    id,
                })
            })
            .collect()
    }
}

/// Ce que l'anonymisation remplacerait avec la sélection courante : `Replacer`
/// de la session de départ complétée par les valeurs cochées (`apply_to` sur
/// une copie), valeur et traitement de chaque balise.
pub(crate) struct MaskedFinder {
    pub(crate) replacer: Replacer,
    owner: HashMap<Tag, usize>,
    treatments: HashMap<Tag, Treatment>,
}

impl MaskedFinder {
    /// `base` n'est pas modifiée.
    pub(crate) fn new(a: &Analysis, base: &Session) -> Self {
        let mut s = base.clone();
        a.apply_to(&mut s);
        let mut owner = HashMap::new();
        for c in a.candidates.iter().filter(|c| c.selected) {
            if let Some(tag) = s.tag_for(c.kind, &c.value) {
                owner.entry(tag).or_insert(c.id);
            }
        }
        let treatments = s
            .entities
            .iter()
            .map(|e| (e.tag, e.treatment.clone()))
            .collect();
        Self {
            replacer: Replacer::new(&s),
            owner,
            treatments,
        }
    }

    /// Occurrences remplacées dans `text` (avec leur balise), dans l'ordre du texte.
    pub(crate) fn matches(&self, text: &str) -> Vec<Match> {
        self.replacer.matches(text)
    }

    /// Valeur cochée qui porte cette balise (`None` : balise d'une valeur de la
    /// session de départ absente de l'analyse).
    pub(crate) fn owner(&self, tag: Tag) -> Option<usize> {
        self.owner.get(&tag).copied()
    }

    /// Traitement de la balise dans la session complétée.
    pub(crate) fn treatment(&self, tag: Tag) -> Option<&Treatment> {
        self.treatments.get(&tag)
    }
}
