//! État de l'onglet « Texte libre » : deux zones, original et anonymisé, sur
//! la session courante.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreeTextNotice {
    Anonymized(usize),
    Restored { restored: usize, unknown: usize },
    Copied,
    NothingToAnonymize,
    NothingToRestore,
    EngineLoading,
}

impl FreeTextNotice {
    pub fn message(self) -> String {
        match self {
            FreeTextNotice::Anonymized(0) => "Aucune donnée sensible trouvée.".to_owned(),
            FreeTextNotice::Anonymized(n) => {
                format!("{n} valeurs remplacées par des balises.")
            }
            FreeTextNotice::Restored {
                restored,
                unknown: 0,
            } => format!("{restored} balises restaurées."),
            FreeTextNotice::Restored { restored, unknown } => format!(
                "{restored} balises restaurées, {unknown} balises inconnues de la session laissées telles quelles."
            ),
            FreeTextNotice::Copied => "Texte anonymisé copié dans le presse-papiers.".to_owned(),
            FreeTextNotice::NothingToAnonymize => {
                "Saisissez ou collez d'abord un texte.".to_owned()
            }
            FreeTextNotice::NothingToRestore => {
                "Collez d'abord la réponse de l'IA dans la zone de droite.".to_owned()
            }
            FreeTextNotice::EngineLoading => {
                "Le moteur termine son chargement : réessayez dans un instant.".to_owned()
            }
        }
    }

    pub fn is_problem(self) -> bool {
        matches!(
            self,
            FreeTextNotice::NothingToAnonymize
                | FreeTextNotice::NothingToRestore
                | FreeTextNotice::EngineLoading
                | FreeTextNotice::Restored { unknown: 1.., .. }
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
    /// Anonymise `original` vers `anonymized` avec `f` (texte → (texte, valeurs)).
    pub fn anonymize(&mut self, f: impl FnOnce(&str) -> (String, usize)) {
        if self.original.trim().is_empty() {
            self.notice = Some(FreeTextNotice::NothingToAnonymize);
            return;
        }
        let (out, n) = f(&self.original);
        self.anonymized = out;
        self.notice = Some(FreeTextNotice::Anonymized(n));
    }

    /// Restaure `anonymized` vers `original` avec `f` (texte → (texte, restaurées, inconnues)).
    pub fn restore(&mut self, f: impl FnOnce(&str) -> (String, usize, usize)) {
        if self.anonymized.trim().is_empty() {
            self.notice = Some(FreeTextNotice::NothingToRestore);
            return;
        }
        let (out, restored, unknown) = f(&self.anonymized);
        self.original = out;
        self.notice = Some(FreeTextNotice::Restored { restored, unknown });
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
    use anon_core::Session;
    use anon_engine::{AnalyzeOptions, Engine};

    const TEXTE: &str = "Bonjour, Hélène Kaloï (helene.kaloi@example.com) vous écrit.";

    #[test]
    fn anonymize_then_restore_with_the_engine() {
        let engine = Engine::standard();
        let mut session = Session::new();
        let mut s = FreeTextState {
            original: TEXTE.to_owned(),
            ..Default::default()
        };
        s.anonymize(|t| {
            let a = engine.analyze_text(t, &AnalyzeOptions::default(), &session);
            engine.apply_selection(&a, &mut session);
            (engine.anonymize_text(t, &session), a.selected().count())
        });
        assert!(s.anonymized.contains("[EMAIL_1]"));
        assert!(!s.anonymized.contains("helene.kaloi"));
        assert!(matches!(s.notice, Some(FreeTextNotice::Anonymized(n)) if n > 0));
        s.original.clear();
        s.anonymized.push_str(" Et [PERSONNE_9].");
        s.restore(|t| {
            let (out, r) = engine.restore_text(t, &session);
            (out, r.restored, r.unknown.len())
        });
        assert!(s.original.starts_with(TEXTE));
        assert!(s.notice.is_some_and(FreeTextNotice::is_problem));
    }

    #[test]
    fn empty_inputs_give_a_notice() {
        let mut s = FreeTextState::default();
        s.anonymize(|_| (String::new(), 0));
        assert_eq!(s.notice, Some(FreeTextNotice::NothingToAnonymize));
        s.restore(|_| (String::new(), 0, 0));
        assert_eq!(s.notice, Some(FreeTextNotice::NothingToRestore));
        assert_eq!(s.copy(), None);
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
