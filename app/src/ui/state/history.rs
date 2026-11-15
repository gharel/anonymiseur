//! Historique d'annulation des décisions de vérification : deux piles
//! d'instantanés (annuler, rétablir), bornées. Chaque entrée garde l'état
//! *avant* la décision et la nature de la décision, pour la notification
//! « Annulé : valeur décochée ». Sans egui.

/// Décisions gardées au plus dans la pile d'annulation.
pub const HISTORY_LIMIT: usize = 100;

/// Nature d'une décision de vérification (libellé de la notification).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Check,
    Uncheck,
    CheckAll,
    UncheckAll,
    Kind,
    Treatment,
    Case,
    AddValue,
    RemoveValue,
    AddZone,
    RemoveZone,
    /// Valeur masquée dans tout le dossier (M, clic sur un surlignage).
    Mask,
    /// Valeur gardée visible dans tout le dossier (G, clic sur un surlignage).
    Keep,
    /// Texte sélectionné, masqué partout.
    MaskEverywhere,
    /// Texte sélectionné, ajouté à la liste « toujours masquer ».
    AlwaysMask,
    /// Correction proposée par le bilan, appliquée (« Appliquer »).
    Fix,
    /// Toutes les corrections proposées (« Tout corriger »).
    FixAll,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Check => "valeur cochée",
            Action::Uncheck => "valeur décochée",
            Action::CheckAll => "valeurs cochées",
            Action::UncheckAll => "valeurs décochées",
            Action::Kind => "type modifié",
            Action::Treatment => "remplacement modifié",
            Action::Case => "respect des majuscules modifié",
            Action::AddValue => "valeur ajoutée",
            Action::RemoveValue => "valeur retirée",
            Action::AddZone => "zone ajoutée",
            Action::RemoveZone => "zone retirée",
            Action::Mask => "valeur masquée",
            Action::Keep => "valeur gardée",
            Action::MaskEverywhere => "texte masqué partout",
            Action::AlwaysMask => "texte toujours masqué",
            Action::Fix => "correction appliquée",
            Action::FixAll => "corrections appliquées",
        }
    }
}

/// Piles « annuler » et « rétablir ».
#[derive(Debug, Clone, PartialEq)]
pub struct History<T> {
    undo: Vec<(Action, T)>,
    redo: Vec<(Action, T)>,
    limit: usize,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self::with_limit(HISTORY_LIMIT)
    }
}

impl<T> History<T> {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Enregistre l'état `before`, pris juste avant la décision `action`. Une
    /// nouvelle décision efface ce qui pouvait être rétabli.
    pub fn record(&mut self, action: Action, before: T) {
        self.undo.push((action, before));
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Annule : rend l'état d'avant la dernière décision ; `current` passe dans
    /// la pile « rétablir ».
    pub fn undo(&mut self, current: T) -> Option<(Action, T)> {
        let (action, before) = self.undo.pop()?;
        self.redo.push((action, current));
        Some((action, before))
    }

    /// Rétablit : rend l'état d'après la décision annulée ; `current` revient
    /// dans la pile « annuler ».
    pub fn redo(&mut self, current: T) -> Option<(Action, T)> {
        let (action, after) = self.redo.pop()?;
        self.undo.push((action, current));
        Some((action, after))
    }

    /// Retire la dernière décision de la pile « annuler », sans rien empiler :
    /// l'appelant défait la décision puis empile dans « rétablir » de quoi la
    /// refaire ([`History::push_redo`]).
    pub fn pop_undo(&mut self) -> Option<(Action, T)> {
        self.undo.pop()
    }

    /// Empile de quoi refaire une décision qui vient d'être annulée.
    pub fn push_redo(&mut self, action: Action, entry: T) {
        self.redo.push((action, entry));
    }

    /// Retire la dernière décision annulée de la pile « rétablir ».
    pub fn pop_redo(&mut self) -> Option<(Action, T)> {
        self.redo.pop()
    }

    /// Empile de quoi annuler une décision rétablie (la pile « rétablir » est
    /// gardée, contrairement à [`History::record`]).
    pub fn push_undo(&mut self, action: Action, entry: T) {
        self.undo.push((action, entry));
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Décision que « annuler » défera.
    pub fn next_undo(&self) -> Option<Action> {
        self.undo.last().map(|(a, _)| *a)
    }

    /// Décision que « rétablir » refera.
    pub fn next_redo(&self) -> Option<Action> {
        self.redo.last().map(|(a, _)| *a)
    }

    pub fn len(&self) -> usize {
        self.undo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_then_redo_round_trip() {
        let mut h = History::default();
        let mut state = 0;
        h.record(Action::Check, state);
        state = 1;
        h.record(Action::Uncheck, state);
        state = 2;
        assert_eq!(h.next_undo(), Some(Action::Uncheck));
        let (a, before) = h.undo(state).unwrap_or((Action::Check, -1));
        assert_eq!((a, before), (Action::Uncheck, 1));
        state = before;
        let (_, before) = h.undo(state).unwrap_or((Action::Check, -1));
        assert_eq!(before, 0);
        state = before;
        assert!(!h.can_undo());
        assert_eq!(h.undo(state), None);
        let (a, after) = h.redo(state).unwrap_or((Action::Kind, -1));
        assert_eq!((a, after), (Action::Check, 1));
        state = after;
        let (_, after) = h.redo(state).unwrap_or((Action::Kind, -1));
        assert_eq!(after, 2);
        assert!(!h.can_redo());
    }

    #[test]
    fn a_new_decision_clears_redo() {
        let mut h = History::default();
        h.record(Action::Check, 0);
        let _ = h.undo(1);
        assert!(h.can_redo());
        h.record(Action::Kind, 0);
        assert!(!h.can_redo());
        assert_eq!(h.next_redo(), None);
    }

    #[test]
    fn history_is_bounded() {
        let mut h = History::with_limit(3);
        for i in 0..10 {
            h.record(Action::AddZone, i);
        }
        assert_eq!(h.len(), 3);
        // Les plus anciennes décisions sont oubliées.
        assert_eq!(h.undo(10).map(|(_, s)| s), Some(9));
        assert_eq!(h.undo(9).map(|(_, s)| s), Some(8));
        assert_eq!(h.undo(8).map(|(_, s)| s), Some(7));
        assert_eq!(h.undo(7), None);
        h.clear();
        assert!(h.is_empty() && !h.can_redo());
    }

    #[test]
    fn every_action_has_a_label() {
        for a in [
            Action::Check,
            Action::Uncheck,
            Action::CheckAll,
            Action::UncheckAll,
            Action::Kind,
            Action::Treatment,
            Action::Case,
            Action::AddValue,
            Action::RemoveValue,
            Action::AddZone,
            Action::RemoveZone,
            Action::Mask,
            Action::Keep,
            Action::MaskEverywhere,
            Action::AlwaysMask,
            Action::Fix,
            Action::FixAll,
        ] {
            assert!(!a.label().is_empty());
        }
    }

    #[test]
    fn pop_and_push_keep_both_stacks() {
        let mut h = History::default();
        h.record(Action::Fix, 'a');
        h.record(Action::FixAll, 'b');
        assert_eq!(h.pop_undo(), Some((Action::FixAll, 'b')));
        h.push_redo(Action::FixAll, 'B');
        assert_eq!(h.next_redo(), Some(Action::FixAll));
        assert_eq!(h.pop_redo(), Some((Action::FixAll, 'B')));
        h.push_redo(Action::FixAll, 'B');
        // Empiler pour annuler ne vide pas « rétablir ».
        h.push_undo(Action::Fix, 'c');
        assert!(h.can_redo());
        assert_eq!(h.len(), 2);
    }
}
