//! État de l'onglet « Texte libre » : deux zones, original et anonymisé.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreeTextNotice {
    Anonymized(usize),
    Restored(usize),
    Copied,
    NothingToAnonymize,
    NothingToRestore,
}

impl FreeTextNotice {
    pub fn message(self) -> String {
        match self {
            FreeTextNotice::Anonymized(0) => "Aucune donnée sensible trouvée.".to_owned(),
            FreeTextNotice::Anonymized(n) => format!("{n} valeurs remplacées par des balises."),
            FreeTextNotice::Restored(n) => format!("{n} balises restaurées."),
            FreeTextNotice::Copied => "Texte anonymisé copié dans le presse-papiers.".to_owned(),
            FreeTextNotice::NothingToAnonymize => {
                "Saisissez ou collez d'abord un texte.".to_owned()
            }
            FreeTextNotice::NothingToRestore => {
                "Collez d'abord la réponse de l'IA dans la zone de droite.".to_owned()
            }
        }
    }

    pub fn is_problem(self) -> bool {
        matches!(
            self,
            FreeTextNotice::NothingToAnonymize | FreeTextNotice::NothingToRestore
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FreeTextState {
    pub original: String,
    pub anonymized: String,
    pub notice: Option<FreeTextNotice>,
}

impl FreeTextState {
    /// Anonymise `original` vers `anonymized` avec `f` (texte → (texte, remplacements)).
    pub fn anonymize(&mut self, f: impl FnOnce(&str) -> (String, usize)) {
        if self.original.trim().is_empty() {
            self.notice = Some(FreeTextNotice::NothingToAnonymize);
            return;
        }
        let (out, n) = f(&self.original);
        self.anonymized = out;
        self.notice = Some(FreeTextNotice::Anonymized(n));
    }

    /// Restaure `anonymized` vers `original` avec `f`.
    pub fn restore(&mut self, f: impl FnOnce(&str) -> (String, usize)) {
        if self.anonymized.trim().is_empty() {
            self.notice = Some(FreeTextNotice::NothingToRestore);
            return;
        }
        let (out, n) = f(&self.anonymized);
        self.original = out;
        self.notice = Some(FreeTextNotice::Restored(n));
    }

    /// Texte à copier ; `None` s'il n'y a rien.
    pub fn copy(&mut self) -> Option<String> {
        if self.anonymized.is_empty() {
            self.notice = Some(FreeTextNotice::NothingToAnonymize);
            None
        } else {
            self.notice = Some(FreeTextNotice::Copied);
            Some(self.anonymized.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::mock;

    #[test]
    fn anonymize_then_restore_round_trips() {
        let mut s = FreeTextState {
            original: mock::FREE_TEXT.to_owned(),
            ..Default::default()
        };
        s.anonymize(mock::anonymize_text);
        assert!(s.anonymized.contains("[PERSONNE_1]"));
        assert!(!s.anonymized.contains("Claire Martin"));
        assert!(matches!(s.notice, Some(FreeTextNotice::Anonymized(n)) if n > 0));
        s.original.clear();
        s.restore(mock::restore_text);
        assert_eq!(s.original, mock::FREE_TEXT);
    }

    #[test]
    fn empty_inputs_give_a_notice() {
        let mut s = FreeTextState::default();
        s.anonymize(mock::anonymize_text);
        assert_eq!(s.notice, Some(FreeTextNotice::NothingToAnonymize));
        s.restore(mock::restore_text);
        assert_eq!(s.notice, Some(FreeTextNotice::NothingToRestore));
        assert_eq!(s.copy(), None);
        assert!(s.notice.is_some_and(FreeTextNotice::is_problem));
    }

    #[test]
    fn copy_returns_anonymized_text() {
        let mut s = FreeTextState {
            anonymized: "[PERSONNE_1]".into(),
            ..Default::default()
        };
        assert_eq!(s.copy().as_deref(), Some("[PERSONNE_1]"));
        assert_eq!(s.notice, Some(FreeTextNotice::Copied));
    }
}
