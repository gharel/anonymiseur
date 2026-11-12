//! État de l'onglet « Désanonymiser » : table `.anonmap` + mot de passe, puis
//! fichier ou texte collé, puis résultat et rapport.

use super::files::FileEntry;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    File,
    Paste,
}

/// État de la table de correspondance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapStatus {
    None,
    /// Fichier choisi, pas encore déverrouillé.
    Locked {
        path: PathBuf,
        name: String,
    },
    Unlocked {
        name: String,
        entries: usize,
        created: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockError {
    NoMap,
    EmptyPassword,
    WrongPassword,
    NotAMap,
}

impl UnlockError {
    pub fn message(self) -> &'static str {
        match self {
            UnlockError::NoMap => "Choisissez d'abord une table de correspondance (.anonmap).",
            UnlockError::EmptyPassword => "Saisissez le mot de passe de la table.",
            UnlockError::WrongPassword => "Mot de passe incorrect.",
            UnlockError::NotAMap => "Ce fichier n'est pas une table de correspondance .anonmap.",
        }
    }
}

/// Ce que renvoie la restauration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub text: String,
    pub restored: u32,
    /// Balises présentes dans la réponse mais absentes de la table.
    pub unknown_tags: Vec<String>,
    /// Balises de la table que la réponse ne contient plus.
    pub missing_tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreError {
    Locked,
    NoInput,
}

impl RestoreError {
    pub fn message(self) -> &'static str {
        match self {
            RestoreError::Locked => "Déverrouillez la table de correspondance avant de restaurer.",
            RestoreError::NoInput => "Déposez un fichier ou collez le texte renvoyé par l'IA.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeanonymizeState {
    pub map: MapStatus,
    pub password: String,
    pub unlock_error: Option<UnlockError>,
    pub mode: InputMode,
    pub input_file: Option<FileEntry>,
    pub pasted: String,
    pub result: Option<RestoreReport>,
    pub restore_error: Option<RestoreError>,
}

impl Default for DeanonymizeState {
    fn default() -> Self {
        Self {
            map: MapStatus::None,
            password: String::new(),
            unlock_error: None,
            mode: InputMode::Paste,
            input_file: None,
            pasted: String::new(),
            result: None,
            restore_error: None,
        }
    }
}

impl DeanonymizeState {
    /// Choix du fichier de table. Refuse une autre extension que `.anonmap`.
    pub fn choose_map(&mut self, path: PathBuf) -> Result<(), UnlockError> {
        let is_map = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("anonmap"));
        let r = if is_map {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.map = MapStatus::Locked { path, name };
            self.result = None;
            Ok(())
        } else {
            Err(UnlockError::NotAMap)
        };
        self.unlock_error = r.err();
        r
    }

    /// Déverrouille avec `open`, qui reçoit le chemin et le mot de passe et
    /// renvoie (nombre d'entrées, date de création) ou une erreur.
    pub fn unlock(
        &mut self,
        open: impl FnOnce(&PathBuf, &str) -> Result<(usize, String), UnlockError>,
    ) -> Result<(), UnlockError> {
        let r = match &self.map {
            MapStatus::None => Err(UnlockError::NoMap),
            MapStatus::Unlocked { .. } => Ok(()),
            MapStatus::Locked { .. } if self.password.is_empty() => Err(UnlockError::EmptyPassword),
            MapStatus::Locked { path, name } => match open(path, &self.password) {
                Ok((entries, created)) => {
                    self.map = MapStatus::Unlocked {
                        name: name.clone(),
                        entries,
                        created,
                    };
                    Ok(())
                }
                Err(e) => Err(e),
            },
        };
        // Le mot de passe ne reste pas en mémoire de l'écran.
        self.password.clear();
        self.unlock_error = r.err();
        r
    }

    pub fn is_unlocked(&self) -> bool {
        matches!(self.map, MapStatus::Unlocked { .. })
    }

    pub fn set_input_file(&mut self, file: FileEntry) {
        self.input_file = Some(file);
        self.mode = InputMode::File;
        self.restore_error = None;
    }

    pub fn has_input(&self) -> bool {
        match self.mode {
            InputMode::File => self.input_file.is_some(),
            InputMode::Paste => !self.pasted.trim().is_empty(),
        }
    }

    /// Lance la restauration avec `restore` (texte collé → rapport).
    pub fn restore(
        &mut self,
        restore: impl FnOnce(&str) -> RestoreReport,
    ) -> Result<(), RestoreError> {
        let r = if !self.is_unlocked() {
            Err(RestoreError::Locked)
        } else if !self.has_input() {
            Err(RestoreError::NoInput)
        } else {
            let source = match self.mode {
                InputMode::Paste => self.pasted.clone(),
                InputMode::File => String::new(),
            };
            self.result = Some(restore(&source));
            Ok(())
        };
        self.restore_error = r.err();
        r
    }

    /// Referme la table et oublie le résultat.
    pub fn lock(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::mock;

    fn ok_open(_: &PathBuf, pw: &str) -> Result<(usize, String), UnlockError> {
        if pw == "bon" {
            Ok((12, "12/11/2026".into()))
        } else {
            Err(UnlockError::WrongPassword)
        }
    }

    #[test]
    fn map_must_be_anonmap() {
        let mut s = DeanonymizeState::default();
        assert_eq!(s.choose_map("x.docx".into()), Err(UnlockError::NotAMap));
        assert_eq!(s.map, MapStatus::None);
        assert_eq!(s.choose_map("table.ANONMAP".into()), Ok(()));
        assert!(matches!(s.map, MapStatus::Locked { .. }));
    }

    #[test]
    fn unlock_flow() {
        let mut s = DeanonymizeState::default();
        assert_eq!(s.unlock(ok_open), Err(UnlockError::NoMap));
        let _ = s.choose_map("t.anonmap".into());
        assert_eq!(s.unlock(ok_open), Err(UnlockError::EmptyPassword));
        s.password = "mauvais".into();
        assert_eq!(s.unlock(ok_open), Err(UnlockError::WrongPassword));
        assert!(s.password.is_empty(), "mot de passe effacé après essai");
        s.password = "bon".into();
        assert_eq!(s.unlock(ok_open), Ok(()));
        assert!(s.is_unlocked());
        assert_eq!(s.unlock_error, None);
    }

    #[test]
    fn restore_requires_unlock_and_input() {
        let mut s = DeanonymizeState::default();
        assert_eq!(s.restore(mock::restore), Err(RestoreError::Locked));
        let _ = s.choose_map("t.anonmap".into());
        s.password = "bon".into();
        let _ = s.unlock(ok_open);
        assert_eq!(s.restore(mock::restore), Err(RestoreError::NoInput));
        s.pasted = "Bonjour [PERSONNE_1], [PERSONNE_9]".into();
        assert_eq!(s.restore(mock::restore), Ok(()));
        let r = s.result.clone();
        assert!(r.is_some());
        if let Some(r) = r {
            assert!(r.text.contains("Claire Martin"));
            assert_eq!(r.unknown_tags, vec!["[PERSONNE_9]".to_owned()]);
        }
        s.lock();
        assert_eq!(s, DeanonymizeState::default());
    }

    #[test]
    fn file_input_switches_mode() {
        let mut s = DeanonymizeState::default();
        assert!(!s.has_input());
        s.set_input_file(FileEntry::new("reponse.docx".into(), Some(100)));
        assert_eq!(s.mode, InputMode::File);
        assert!(s.has_input());
    }
}
